//! The writer thread: drains the capture ring, keeps the pre-roll, and writes takes to disk.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use chrono::{Local, NaiveDateTime, TimeDelta};
use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, TryRecvError};

use crate::take::{self, TakeMeta, Tempo, TrackMeta};
use crate::wav::{Bext, SampleFormat, WavSpec, WavWriter};

pub const MAX_PREROLL_SECS: u32 = 30;
/// A track whose peak never reaches this level (-60 dBFS) counts as silent: far below any
/// connected source, above the idle noise of an unused input.
pub const SILENCE_PEAK: f32 = 0.001;

/// Timing and safety thresholds of the writer thread.
#[derive(Debug, Clone)]
pub struct RecorderConfig {
    pub channels: usize,
    pub sample_rate: u32,
    pub header_interval: Duration,
    pub sync_interval: Duration,
    pub disk_check_interval: Duration,
    pub min_free_bytes: u64,
    pub poll_interval: Duration,
}

impl RecorderConfig {
    pub fn new(channels: usize, sample_rate: u32) -> Self {
        Self {
            channels,
            sample_rate,
            header_interval: Duration::from_secs(1),
            sync_interval: Duration::from_secs(5),
            disk_check_interval: Duration::from_secs(2),
            min_free_bytes: 256 * 1024 * 1024,
            poll_interval: Duration::from_millis(5),
        }
    }
}

/// One output file of a take.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackRequest {
    pub name: String,
    /// 0-based device channels, in file channel order.
    pub channels: Vec<usize>,
}

#[derive(Debug, Clone)]
pub struct TakeRequest {
    pub root: PathBuf,
    pub label: String,
    pub tracks: Vec<TrackRequest>,
    pub format: SampleFormat,
    pub host: String,
    pub device: String,
    /// Input-timeline frame (frames pushed into the ring since the stream opened) at which
    /// the files begin. `None` starts immediately and keeps the pre-roll.
    pub start_at: Option<u64>,
    pub tempo: Option<Tempo>,
    /// Do not keep the files of tracks that stayed silent for the whole take.
    pub skip_silent: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TakeSummary {
    pub dir: PathBuf,
    pub files: Vec<PathBuf>,
    /// Names of the tracks whose silent files were not kept.
    pub skipped_silent: Vec<String>,
    pub frames: u64,
    pub dropped_frames: u64,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RecorderEvent {
    Started {
        dir: PathBuf,
    },
    Stopped(TakeSummary),
    /// A scheduled start was called off before any file was created.
    Cancelled,
    /// User-facing message (Japanese).
    Error(String),
}

enum Command {
    Start(TakeRequest),
    Stop,
    Marker,
    SetPreRoll(u32),
    Shutdown,
}

#[derive(Debug, Default)]
struct Shared {
    recording: AtomicBool,
    frames: AtomicU64,
}

/// UI-side handle; dropping it stops any take and joins the thread.
pub struct Recorder {
    commands: Sender<Command>,
    events: Receiver<RecorderEvent>,
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

impl Recorder {
    pub fn spawn(
        consumer: rtrb::Consumer<f32>,
        config: RecorderConfig,
        dropped_frames: Arc<AtomicU64>,
    ) -> std::io::Result<Self> {
        let (cmd_tx, cmd_rx) = crossbeam_channel::unbounded();
        let (ev_tx, ev_rx) = crossbeam_channel::unbounded();
        let shared = Arc::new(Shared::default());
        let worker = Worker {
            consumer,
            commands: cmd_rx,
            events: ev_tx,
            shared: shared.clone(),
            dropped_frames,
            scratch: Vec::new(),
            preroll: PreRoll::new(0),
            take: None,
            pending: None,
            frames_seen: 0,
            config,
        };
        let thread = thread::Builder::new()
            .name("recrec-writer".into())
            .spawn(move || worker.run())?;
        Ok(Self {
            commands: cmd_tx,
            events: ev_rx,
            shared,
            thread: Some(thread),
        })
    }

    pub fn start(&self, request: TakeRequest) {
        self.send(Command::Start(request));
    }

    pub fn stop(&self) {
        self.send(Command::Stop);
    }

    /// Drops a marker at the current position of the active take.
    pub fn add_marker(&self) {
        self.send(Command::Marker);
    }

    pub fn set_preroll(&self, secs: u32) {
        self.send(Command::SetPreRoll(secs.min(MAX_PREROLL_SECS)));
    }

    fn send(&self, cmd: Command) {
        if self.commands.send(cmd).is_err() {
            log::error!("writer thread is gone; command dropped");
        }
    }

    pub fn try_event(&self) -> Option<RecorderEvent> {
        self.events.try_recv().ok()
    }

    pub fn is_recording(&self) -> bool {
        self.shared.recording.load(Ordering::Relaxed)
    }

    pub fn frames_recorded(&self) -> u64 {
        self.shared.frames.load(Ordering::Relaxed)
    }

    /// Stops any take, waits for the files to be closed, and returns the remaining events.
    pub fn shutdown(mut self) -> Vec<RecorderEvent> {
        self.join();
        self.events.try_iter().collect()
    }

    fn join(&mut self) {
        if let Some(thread) = self.thread.take() {
            // If the thread already exited the channel is closed; joining is all that is left.
            let _ = self.commands.send(Command::Shutdown);
            if thread.join().is_err() {
                log::error!("writer thread panicked");
            }
        }
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        self.join();
    }
}

/// Fixed-capacity history of the most recent interleaved samples.
struct PreRoll {
    samples: VecDeque<f32>,
    capacity: usize,
}

impl PreRoll {
    fn new(capacity: usize) -> Self {
        Self {
            samples: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    fn push(&mut self, interleaved: &[f32]) {
        if self.capacity == 0 {
            return;
        }
        let keep = &interleaved[interleaved.len().saturating_sub(self.capacity)..];
        let overflow = (self.samples.len() + keep.len()).saturating_sub(self.capacity);
        self.samples.drain(..overflow);
        self.samples.extend(keep);
    }

    fn take(&mut self) -> Vec<f32> {
        self.samples.drain(..).collect()
    }
}

struct OpenTrack {
    writer: WavWriter,
    channels: Vec<usize>,
    path: PathBuf,
    buf: Vec<f32>,
    /// Highest absolute sample written so far.
    peak: f32,
}

struct ActiveTake {
    skip_silent: bool,
    dir: PathBuf,
    tracks: Vec<OpenTrack>,
    meta: TakeMeta,
    markers: Vec<u64>,
    dropped_at_start: u64,
    last_header: Instant,
    last_sync: Instant,
    last_disk_check: Instant,
}

struct Worker {
    consumer: rtrb::Consumer<f32>,
    commands: Receiver<Command>,
    events: Sender<RecorderEvent>,
    shared: Arc<Shared>,
    dropped_frames: Arc<AtomicU64>,
    scratch: Vec<f32>,
    preroll: PreRoll,
    take: Option<ActiveTake>,
    /// A take waiting for its `start_at` frame (count-in).
    pending: Option<TakeRequest>,
    /// Frames read from the ring so far: the position on the input timeline.
    frames_seen: u64,
    config: RecorderConfig,
}

enum Flow {
    Continue,
    Exit,
}

impl Worker {
    fn run(mut self) {
        loop {
            loop {
                match self.commands.try_recv() {
                    Ok(cmd) => {
                        if let Flow::Exit = self.handle(cmd) {
                            return;
                        }
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        self.handle(Command::Shutdown);
                        return;
                    }
                }
            }
            let got = self.drain();
            self.periodic();
            if got == 0 {
                match self.commands.recv_timeout(self.config.poll_interval) {
                    Ok(cmd) => {
                        if let Flow::Exit = self.handle(cmd) {
                            return;
                        }
                    }
                    Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => {
                        self.handle(Command::Shutdown);
                        return;
                    }
                }
            }
        }
    }

    fn handle(&mut self, cmd: Command) -> Flow {
        match cmd {
            Command::Start(request) => {
                if self.take.is_some() || self.pending.is_some() {
                    log::warn!("start requested while already recording; ignored");
                } else if request.start_at.is_some_and(|at| at > self.frames_seen) {
                    self.pending = Some(request);
                } else {
                    self.start_or_report(request, true);
                }
            }
            Command::Stop => {
                self.cancel_pending();
                self.drain();
                self.stop(None);
            }
            Command::Marker => {
                if let Some(take) = self.take.as_mut() {
                    let at = take.tracks.first().map_or(0, |t| t.writer.frames_written());
                    if take.markers.last() != Some(&at) {
                        take.markers.push(at);
                    }
                }
            }
            Command::SetPreRoll(secs) => {
                let samples =
                    secs as usize * self.config.sample_rate as usize * self.config.channels;
                self.preroll = PreRoll::new(samples);
            }
            Command::Shutdown => {
                self.cancel_pending();
                self.drain();
                self.stop(None);
                return Flow::Exit;
            }
        }
        Flow::Continue
    }

    fn emit(&self, event: RecorderEvent) {
        if self.events.send(event).is_err() {
            log::warn!("UI is gone; recorder event dropped");
        }
    }

    fn cancel_pending(&mut self) {
        if self.pending.take().is_some() {
            self.emit(RecorderEvent::Cancelled);
        }
    }

    fn start_or_report(&mut self, request: TakeRequest, use_preroll: bool) {
        if let Err(msg) = self.start(request, use_preroll) {
            log::error!("could not start take: {msg}");
            self.emit(RecorderEvent::Error(msg));
        }
    }

    fn start(&mut self, request: TakeRequest, use_preroll: bool) -> Result<(), String> {
        if request.tracks.is_empty() {
            return Err("録音するトラックがありません".into());
        }
        if let Some(bad) = request
            .tracks
            .iter()
            .flat_map(|t| &t.channels)
            .find(|&&c| c >= self.config.channels)
        {
            return Err(format!("入力 {} はこのデバイスにありません", bad + 1));
        }
        std::fs::create_dir_all(&request.root).map_err(|e| {
            format!(
                "出力先フォルダを作れません ({}): {e}",
                request.root.display()
            )
        })?;
        check_free_space(&request.root, self.config.min_free_bytes)?;

        let mut preroll = self.preroll.take();
        if !use_preroll {
            preroll.clear();
        }
        let preroll_frames = (preroll.len() / self.config.channels) as u64;
        let now = Local::now().naive_local();
        let origin = now
            - TimeDelta::microseconds(
                (preroll_frames as f64 / f64::from(self.config.sample_rate) * 1e6) as i64,
            );

        let dir = take::create_take_dir(&request.root, now, &request.label)
            .map_err(|e| format!("テイクフォルダを作れません: {e}"))?;
        take::create_marker(&dir).map_err(|e| format!("テイクフォルダに書き込めません: {e}"))?;

        let skip_silent = request.skip_silent;
        let (tracks, meta) = match self.open_files(&dir, request, origin, preroll_frames) {
            Ok(opened) => opened,
            Err(msg) => {
                // Files already created are closed (empty) on drop; without the marker the
                // next launch does not mistake this aborted take for a crashed one.
                if let Err(e) = take::remove_marker(&dir) {
                    log::warn!("could not remove marker of aborted take: {e}");
                }
                return Err(msg);
            }
        };

        let now = Instant::now();
        self.take = Some(ActiveTake {
            skip_silent,
            dir: dir.clone(),
            tracks,
            meta,
            markers: Vec::new(),
            dropped_at_start: self.dropped_frames.load(Ordering::Relaxed),
            last_header: now,
            last_sync: now,
            last_disk_check: now,
        });
        self.shared.frames.store(0, Ordering::Relaxed);
        self.shared.recording.store(true, Ordering::Relaxed);
        log::info!("take started: {}", dir.display());
        self.emit(RecorderEvent::Started { dir });
        if !preroll.is_empty() {
            self.write(&preroll);
        }
        Ok(())
    }

    /// Creates one WAV per track plus `take.json` inside an already created take folder.
    fn open_files(
        &self,
        dir: &Path,
        request: TakeRequest,
        origin: NaiveDateTime,
        preroll_frames: u64,
    ) -> Result<(Vec<OpenTrack>, TakeMeta), String> {
        let mut tracks = Vec::with_capacity(request.tracks.len());
        let mut track_meta = Vec::with_capacity(request.tracks.len());
        for (i, t) in request.tracks.iter().enumerate() {
            let file = take::track_file_name(i, &t.name);
            let path = dir.join(&file);
            let spec = WavSpec {
                channels: t.channels.len() as u16,
                sample_rate: self.config.sample_rate,
                format: request.format,
            };
            let bext = bext_for(origin, self.config.sample_rate, dir, &t.name);
            let writer = WavWriter::create(&path, spec, &bext, i as u32)
                .map_err(|e| format!("{} を作れません: {e}", path.display()))?;
            track_meta.push(TrackMeta {
                name: t.name.clone(),
                inputs: t.channels.iter().map(|&c| c as u16 + 1).collect(),
                file,
            });
            tracks.push(OpenTrack {
                writer,
                channels: t.channels.clone(),
                path,
                buf: Vec::new(),
                peak: 0.0,
            });
        }

        let meta = TakeMeta {
            app_version: env!("CARGO_PKG_VERSION").to_owned(),
            host: request.host,
            device: request.device,
            sample_rate: self.config.sample_rate,
            format: request.format,
            started_at: origin.format("%Y-%m-%dT%H:%M:%S%.3f").to_string(),
            tracks: track_meta,
            frames: 0,
            preroll_frames,
            dropped_frames: 0,
            recovered: false,
            note: String::new(),
            tempo: request.tempo,
            markers: Vec::new(),
            skipped_silent: Vec::new(),
        };
        take::write_meta(dir, &meta).map_err(|e| format!("take.json を書けません: {e}"))?;
        Ok((tracks, meta))
    }

    /// Reads every whole frame currently in the ring. Returns the number of samples read.
    fn drain(&mut self) -> usize {
        let channels = self.config.channels;
        let available = self.consumer.slots();
        let n = available - available % channels;
        if n == 0 {
            return 0;
        }
        let Ok(chunk) = self.consumer.read_chunk(n) else {
            return 0;
        };
        let (a, b) = chunk.as_slices();
        self.scratch.clear();
        self.scratch.extend_from_slice(a);
        self.scratch.extend_from_slice(b);
        chunk.commit_all();

        let samples = std::mem::take(&mut self.scratch);
        let before = self.frames_seen;
        self.frames_seen += (n / channels) as u64;
        let due = self
            .pending
            .as_ref()
            .and_then(|p| p.start_at)
            .filter(|at| *at <= self.frames_seen);
        if let Some(at) = due
            && let Some(request) = self.pending.take()
        {
            // The take begins exactly at frame `at`: skip what came before it in this chunk.
            let skip = at.saturating_sub(before) as usize * channels;
            self.start_or_report(request, false);
            self.write(&samples[skip.min(samples.len())..]);
        } else if self.take.is_some() {
            self.write(&samples);
        } else {
            self.preroll.push(&samples);
        }
        self.scratch = samples;
        n
    }

    fn write(&mut self, interleaved: &[f32]) {
        let channels = self.config.channels;
        let Some(take) = self.take.as_mut() else {
            return;
        };
        let mut failure = None;
        for track in &mut take.tracks {
            track.buf.clear();
            for frame in interleaved.chunks_exact(channels) {
                track.buf.extend(track.channels.iter().map(|&c| frame[c]));
            }
            track.peak = track.buf.iter().fold(track.peak, |p, s| p.max(s.abs()));
            if let Err(e) = track.writer.write_samples(&track.buf) {
                failure = Some(format!(
                    "{} への書き込みに失敗しました: {e}",
                    track.path.display()
                ));
                break;
            }
        }
        if let Some(first) = take.tracks.first() {
            self.shared
                .frames
                .store(first.writer.frames_written(), Ordering::Relaxed);
        }
        if let Some(msg) = failure {
            self.stop(Some(msg));
        }
    }

    fn periodic(&mut self) {
        let Some(take) = self.take.as_mut() else {
            return;
        };
        let now = Instant::now();
        let mut failure = None;
        if now - take.last_header >= self.config.header_interval {
            take.last_header = now;
            for t in &mut take.tracks {
                if let Err(e) = t.writer.flush_header() {
                    failure = Some(format!("{} の更新に失敗しました: {e}", t.path.display()));
                }
            }
        }
        if failure.is_none() && now - take.last_sync >= self.config.sync_interval {
            take.last_sync = now;
            for t in &mut take.tracks {
                if let Err(e) = t.writer.sync() {
                    failure = Some(format!(
                        "{} の書き込みに失敗しました: {e}",
                        t.path.display()
                    ));
                }
            }
        }
        if failure.is_none() && now - take.last_disk_check >= self.config.disk_check_interval {
            take.last_disk_check = now;
            if let Err(msg) = check_free_space(&take.dir, self.config.min_free_bytes) {
                failure = Some(msg);
            }
        }
        if let Some(msg) = failure {
            self.stop(Some(msg));
        }
    }

    /// Closes the active take, if any. `failure` is reported to the UI as an error.
    fn stop(&mut self, failure: Option<String>) {
        let Some(mut take) = self.take.take() else {
            return;
        };
        self.shared.recording.store(false, Ordering::Relaxed);
        let mut errors: Vec<String> = failure.iter().cloned().collect();
        let frames = take.tracks.first().map_or(0, |t| t.writer.frames_written());
        let mut files = Vec::with_capacity(take.tracks.len());
        for t in &mut take.tracks {
            if let Err(e) = t.writer.finalize_with_cues(&take.markers) {
                errors.push(format!("{} を閉じられません: {e}", t.path.display()));
            }
            files.push(t.path.clone());
        }
        let dropped = self
            .dropped_frames
            .load(Ordering::Relaxed)
            .saturating_sub(take.dropped_at_start);
        // Unused inputs would otherwise leave a silent file per take. Only when the take
        // closed cleanly, and never all of them (an all-silent take is kept as recorded).
        let silent: Vec<usize> = take
            .tracks
            .iter()
            .enumerate()
            .filter(|(_, t)| t.peak < SILENCE_PEAK)
            .map(|(i, _)| i)
            .collect();
        let mut skipped_silent = Vec::new();
        if take.skip_silent && errors.is_empty() && silent.len() < take.tracks.len() {
            for i in silent.into_iter().rev() {
                match std::fs::remove_file(&files[i]) {
                    Ok(()) => {
                        files.remove(i);
                        skipped_silent.insert(0, take.meta.tracks.remove(i).name);
                    }
                    Err(e) => errors.push(format!(
                        "無音のファイル {} を消せません: {e}",
                        files[i].display()
                    )),
                }
            }
        }
        take.meta.skipped_silent = skipped_silent.clone();
        take.meta.frames = frames;
        take.meta.markers = std::mem::take(&mut take.markers);
        take.meta.dropped_frames = dropped;
        if let Err(e) = take::write_meta(&take.dir, &take.meta) {
            errors.push(format!("take.json を書けません: {e}"));
        }
        // Keep the marker on failure so the next launch repairs the files.
        if errors.is_empty()
            && let Err(e) = take::remove_marker(&take.dir)
        {
            errors.push(format!("録音中マーカーを消せません: {e}"));
        }
        log::info!(
            "take stopped: {} ({frames} frames, {dropped} dropped, {} errors)",
            take.dir.display(),
            errors.len()
        );
        for e in &errors {
            log::error!("{e}");
        }
        if let Some(msg) = failure {
            self.emit(RecorderEvent::Error(msg));
        }
        self.emit(RecorderEvent::Stopped(TakeSummary {
            dir: take.dir,
            files,
            skipped_silent,
            frames,
            dropped_frames: dropped,
            errors,
        }));
    }
}

fn bext_for(origin: NaiveDateTime, sample_rate: u32, dir: &Path, track: &str) -> Bext {
    let take_name = dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    Bext::at(origin, sample_rate, format!("{take_name} {track}"))
}

fn check_free_space(path: &Path, min_free: u64) -> Result<(), String> {
    match fs2::available_space(path) {
        Ok(free) if free < min_free => Err(format!(
            "ディスクの空きが {} MB を下回ったため録音できません",
            min_free / (1024 * 1024)
        )),
        Ok(_) => Ok(()),
        // Some network drives cannot report free space; recording can still work there.
        Err(e) => {
            log::warn!("could not read free space of {}: {e}", path.display());
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    const CH: usize = 4;
    const RATE: u32 = 1000;

    struct Rig {
        producer: rtrb::Producer<f32>,
        recorder: Recorder,
        dropped: Arc<AtomicU64>,
        root: tempfile::TempDir,
    }

    fn rig() -> Rig {
        let (producer, consumer) = rtrb::RingBuffer::new(CH * 10_000);
        let dropped = Arc::new(AtomicU64::new(0));
        let mut config = RecorderConfig::new(CH, RATE);
        config.header_interval = Duration::from_millis(10);
        config.min_free_bytes = 0;
        let recorder = Recorder::spawn(consumer, config, dropped.clone()).unwrap();
        Rig {
            producer,
            recorder,
            dropped,
            root: tempfile::tempdir().unwrap(),
        }
    }

    /// Frame `n`, channel `c` carries a value identifying both.
    fn sample(n: usize, c: usize) -> f32 {
        ((n % 1000) as f32 * 4.0 + c as f32) / 8192.0
    }

    fn feed(p: &mut rtrb::Producer<f32>, from: usize, frames: usize) {
        for n in from..from + frames {
            for c in 0..CH {
                p.push(sample(n, c)).unwrap();
            }
        }
    }

    fn request(root: &Path, tracks: Vec<TrackRequest>) -> TakeRequest {
        TakeRequest {
            root: root.to_owned(),
            label: String::new(),
            tracks,
            format: SampleFormat::Float32,
            host: "Test".into(),
            device: "Fake".into(),
            start_at: None,
            tempo: None,
            skip_silent: false,
        }
    }

    fn wait_event(r: &Recorder) -> RecorderEvent {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if let Some(e) = r.try_event() {
                return e;
            }
            thread::sleep(Duration::from_millis(2));
        }
        panic!("no recorder event");
    }

    fn wait_until(cond: impl Fn() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !cond() {
            assert!(Instant::now() < deadline, "timed out");
            thread::sleep(Duration::from_millis(2));
        }
    }

    /// Decodes float samples from the data chunk. Nothing before it contains "data"
    /// (a cue chunk after it does, hence the search from the front).
    fn read_float_wav(path: &Path) -> Vec<f32> {
        let bytes = fs::read(path).unwrap();
        let at = bytes.windows(4).position(|w| w == b"data").unwrap();
        let size = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().unwrap()) as usize;
        bytes[at + 8..at + 8 + size]
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| f32::from_le_bytes(*b))
            .collect()
    }

    fn stopped(r: &Recorder) -> TakeSummary {
        loop {
            match wait_event(r) {
                RecorderEvent::Stopped(s) => return s,
                RecorderEvent::Error(e) => panic!("unexpected error: {e}"),
                RecorderEvent::Started { .. } | RecorderEvent::Cancelled => {}
            }
        }
    }

    #[test]
    fn writes_each_track_from_its_source_channels() {
        let mut rig = rig();
        let tracks = vec![
            TrackRequest {
                name: "Vocal".into(),
                channels: vec![0],
            },
            TrackRequest {
                name: "Guitar".into(),
                channels: vec![1],
            },
            TrackRequest {
                name: "Synth".into(),
                channels: vec![2, 3],
            },
        ];
        rig.recorder.start(request(rig.root.path(), tracks));
        let RecorderEvent::Started { dir } = wait_event(&rig.recorder) else {
            panic!("expected Started");
        };
        assert!(dir.join(take::MARKER_FILE).exists());
        feed(&mut rig.producer, 0, 500);
        wait_until(|| rig.recorder.frames_recorded() == 500);
        rig.recorder.stop();
        let summary = stopped(&rig.recorder);
        assert!(summary.errors.is_empty(), "{:?}", summary.errors);
        assert_eq!(summary.frames, 500);
        assert!(!rig.recorder.is_recording());
        assert!(!dir.join(take::MARKER_FILE).exists());

        let vocal = read_float_wav(&dir.join("01_Vocal.wav"));
        let guitar = read_float_wav(&dir.join("02_Guitar.wav"));
        let synth = read_float_wav(&dir.join("03_Synth.wav"));
        assert_eq!(vocal, (0..500).map(|n| sample(n, 0)).collect::<Vec<_>>());
        assert_eq!(guitar, (0..500).map(|n| sample(n, 1)).collect::<Vec<_>>());
        let expected_synth: Vec<f32> = (0..500)
            .flat_map(|n| [sample(n, 2), sample(n, 3)])
            .collect();
        assert_eq!(synth, expected_synth);

        let meta = take::read_meta(&dir).unwrap();
        assert_eq!(meta.frames, 500);
        assert_eq!(meta.tracks[2].inputs, vec![3, 4]);
    }

    #[test]
    fn preroll_includes_audio_from_before_start() {
        let mut rig = rig();
        rig.recorder.set_preroll(1); // 1 s = 1000 frames at RATE
        feed(&mut rig.producer, 0, 1500);
        wait_until(|| rig.producer.slots() == CH * 10_000);
        rig.recorder.start(request(
            rig.root.path(),
            vec![TrackRequest {
                name: "A".into(),
                channels: vec![0],
            }],
        ));
        let RecorderEvent::Started { dir } = wait_event(&rig.recorder) else {
            panic!("expected Started");
        };
        feed(&mut rig.producer, 1500, 100);
        wait_until(|| rig.recorder.frames_recorded() == 1100);
        rig.recorder.stop();
        stopped(&rig.recorder);
        let a = read_float_wav(&dir.join("01_A.wav"));
        assert_eq!(a, (500..1600).map(|n| sample(n, 0)).collect::<Vec<_>>());
        assert_eq!(take::read_meta(&dir).unwrap().preroll_frames, 1000);
    }

    #[test]
    fn header_is_updated_while_recording() {
        let mut rig = rig();
        rig.recorder.start(request(
            rig.root.path(),
            vec![TrackRequest {
                name: "A".into(),
                channels: vec![0],
            }],
        ));
        let RecorderEvent::Started { dir } = wait_event(&rig.recorder) else {
            panic!("expected Started");
        };
        feed(&mut rig.producer, 0, 200);
        let path = dir.join("01_A.wav");
        wait_until(|| {
            fs::read(&path)
                .map(|_| read_float_wav(&path).len() == 200)
                .unwrap_or(false)
        });
        assert!(rig.recorder.is_recording());
    }

    #[test]
    fn dropped_frames_during_take_are_counted() {
        let mut rig = rig();
        rig.dropped.store(7, Ordering::Relaxed);
        rig.recorder.start(request(
            rig.root.path(),
            vec![TrackRequest {
                name: "A".into(),
                channels: vec![0],
            }],
        ));
        let RecorderEvent::Started { dir } = wait_event(&rig.recorder) else {
            panic!("expected Started");
        };
        rig.dropped.fetch_add(3, Ordering::Relaxed);
        feed(&mut rig.producer, 0, 10);
        rig.recorder.stop();
        let summary = stopped(&rig.recorder);
        assert_eq!(summary.dropped_frames, 3);
        assert_eq!(take::read_meta(&dir).unwrap().dropped_frames, 3);
    }

    #[test]
    fn rejects_channels_the_device_lacks() {
        let rig = rig();
        rig.recorder.start(request(
            rig.root.path(),
            vec![TrackRequest {
                name: "A".into(),
                channels: vec![CH],
            }],
        ));
        assert!(matches!(wait_event(&rig.recorder), RecorderEvent::Error(_)));
        assert!(!rig.recorder.is_recording());
    }

    #[test]
    fn shutdown_finalizes_an_active_take() {
        let mut rig = rig();
        rig.recorder.start(request(
            rig.root.path(),
            vec![TrackRequest {
                name: "A".into(),
                channels: vec![0],
            }],
        ));
        let RecorderEvent::Started { dir } = wait_event(&rig.recorder) else {
            panic!("expected Started");
        };
        feed(&mut rig.producer, 0, 50);
        let events = rig.recorder.shutdown();
        assert!(
            events
                .iter()
                .any(|e| matches!(e, RecorderEvent::Stopped(_)))
        );
        assert_eq!(read_float_wav(&dir.join("01_A.wav")).len(), 50);
        assert!(!dir.join(take::MARKER_FILE).exists());
    }

    #[test]
    fn scheduled_start_begins_exactly_at_the_requested_frame() {
        let mut rig = rig();
        rig.recorder.set_preroll(1);
        feed(&mut rig.producer, 0, 300);
        wait_until(|| rig.producer.slots() == CH * 10_000);
        let mut req = request(
            rig.root.path(),
            vec![TrackRequest {
                name: "A".into(),
                channels: vec![1],
            }],
        );
        req.start_at = Some(450);
        rig.recorder.start(req);
        thread::sleep(Duration::from_millis(30));
        assert!(!rig.recorder.is_recording(), "waits for the start frame");
        feed(&mut rig.producer, 300, 400);
        let RecorderEvent::Started { dir } = wait_event(&rig.recorder) else {
            panic!("expected Started");
        };
        wait_until(|| rig.recorder.frames_recorded() == 250);
        rig.recorder.stop();
        stopped(&rig.recorder);
        let a = read_float_wav(&dir.join("01_A.wav"));
        assert_eq!(a, (450..700).map(|n| sample(n, 1)).collect::<Vec<_>>());
        assert_eq!(take::read_meta(&dir).unwrap().preroll_frames, 0);
    }

    #[test]
    fn stopping_before_a_scheduled_start_cancels_it() {
        let rig = rig();
        let mut req = request(
            rig.root.path(),
            vec![TrackRequest {
                name: "A".into(),
                channels: vec![0],
            }],
        );
        req.start_at = Some(1_000_000);
        rig.recorder.start(req);
        rig.recorder.stop();
        assert_eq!(wait_event(&rig.recorder), RecorderEvent::Cancelled);
        assert!(fs::read_dir(rig.root.path()).unwrap().next().is_none());
    }

    #[test]
    fn markers_are_saved_in_meta_and_as_cue_points() {
        let mut rig = rig();
        rig.recorder.start(request(
            rig.root.path(),
            vec![TrackRequest {
                name: "A".into(),
                channels: vec![0],
            }],
        ));
        let RecorderEvent::Started { dir } = wait_event(&rig.recorder) else {
            panic!("expected Started");
        };
        feed(&mut rig.producer, 0, 120);
        wait_until(|| rig.recorder.frames_recorded() == 120);
        rig.recorder.add_marker();
        feed(&mut rig.producer, 120, 30);
        wait_until(|| rig.recorder.frames_recorded() == 150);
        rig.recorder.stop();
        stopped(&rig.recorder);
        assert_eq!(take::read_meta(&dir).unwrap().markers, vec![120]);
        let bytes = fs::read(dir.join("01_A.wav")).unwrap();
        let cue = bytes.windows(4).rposition(|w| w == b"cue ").unwrap();
        assert_eq!(
            u32::from_le_bytes(bytes[cue + 16..cue + 20].try_into().unwrap()),
            120
        );
        // The audio itself is untouched by the trailing chunk.
        assert_eq!(read_float_wav(&dir.join("01_A.wav")).len(), 150);
    }

    fn feed_with_silent_channels(p: &mut rtrb::Producer<f32>, frames: usize, loud: &[usize]) {
        for n in 0..frames {
            for c in 0..CH {
                let v = if loud.contains(&c) {
                    sample(n + 1, c)
                } else {
                    0.0
                };
                p.push(v).unwrap();
            }
        }
    }

    fn three_tracks(root: &Path, skip_silent: bool) -> TakeRequest {
        let track = |name: &str, c: usize| TrackRequest {
            name: name.into(),
            channels: vec![c],
        };
        let mut req = request(root, vec![track("A", 0), track("B", 1), track("C", 2)]);
        req.skip_silent = skip_silent;
        req
    }

    #[test]
    fn silent_tracks_are_not_kept_when_asked() {
        let mut rig = rig();
        rig.recorder.start(three_tracks(rig.root.path(), true));
        let RecorderEvent::Started { dir } = wait_event(&rig.recorder) else {
            panic!("expected Started");
        };
        feed_with_silent_channels(&mut rig.producer, 200, &[1]);
        wait_until(|| rig.recorder.frames_recorded() == 200);
        rig.recorder.stop();
        let summary = stopped(&rig.recorder);
        assert!(summary.errors.is_empty(), "{:?}", summary.errors);
        assert_eq!(summary.skipped_silent, ["A", "C"]);
        assert_eq!(summary.files, [dir.join("02_B.wav")]);
        assert!(!dir.join("01_A.wav").exists() && !dir.join("03_C.wav").exists());
        let meta = take::read_meta(&dir).unwrap();
        assert_eq!(meta.tracks.len(), 1);
        assert_eq!(meta.tracks[0].file, "02_B.wav");
        assert_eq!(meta.skipped_silent, ["A", "C"]);
    }

    #[test]
    fn silent_tracks_are_kept_by_default_and_when_everything_is_silent() {
        let mut rig = rig();
        rig.recorder.start(three_tracks(rig.root.path(), false));
        let RecorderEvent::Started { dir } = wait_event(&rig.recorder) else {
            panic!("expected Started");
        };
        feed_with_silent_channels(&mut rig.producer, 50, &[1]);
        wait_until(|| rig.recorder.frames_recorded() == 50);
        rig.recorder.stop();
        assert_eq!(stopped(&rig.recorder).files.len(), 3);
        assert!(dir.join("01_A.wav").exists());

        rig.recorder.start(three_tracks(rig.root.path(), true));
        let RecorderEvent::Started { dir } = wait_event(&rig.recorder) else {
            panic!("expected Started");
        };
        feed_with_silent_channels(&mut rig.producer, 50, &[]);
        wait_until(|| rig.recorder.frames_recorded() == 50);
        rig.recorder.stop();
        let summary = stopped(&rig.recorder);
        assert_eq!(summary.files.len(), 3);
        assert!(summary.skipped_silent.is_empty());
        assert!(dir.join("01_A.wav").exists());
    }

    #[test]
    fn preroll_keeps_only_the_newest_samples() {
        let mut p = PreRoll::new(4);
        p.push(&[1.0, 2.0, 3.0]);
        p.push(&[4.0, 5.0]);
        assert_eq!(p.take(), vec![2.0, 3.0, 4.0, 5.0]);
        p.push(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
        assert_eq!(p.take(), vec![3.0, 4.0, 5.0, 6.0]);
        let mut off = PreRoll::new(0);
        off.push(&[1.0]);
        assert!(off.take().is_empty());
    }
}
