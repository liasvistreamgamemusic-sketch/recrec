//! Take playback: a reader thread decodes the take's WAV files, mixes them to stereo and
//! feeds the output callback through a ring. The callback never touches a file.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender};

use crate::wav::WavReader;

/// Frames the ring holds: short, so gain changes and seeks are heard almost at once.
const RING_FRAMES: usize = 4096;
const CHUNK_FRAMES: usize = 1024;
const IDLE_POLL: Duration = Duration::from_millis(3);
const DISCARD_TIMEOUT: Duration = Duration::from_millis(200);

/// State shared with the output callback.
#[derive(Debug, Default)]
pub struct PlayState {
    playing: AtomicBool,
    /// Take frame the callback will play next.
    position: AtomicU64,
    /// Set by the reader thread; the callback empties the ring and clears it.
    discard: AtomicBool,
    /// Output-timeline frame from which the callback may consume (for scheduled starts).
    gate_out: AtomicU64,
}

impl PlayState {
    pub fn is_playing(&self) -> bool {
        self.playing.load(Ordering::Relaxed)
    }

    pub fn position(&self) -> u64 {
        self.position.load(Ordering::Relaxed)
    }
}

/// Callback-side end of the playback ring. Real-time safe.
pub struct PlaybackTap {
    ring: rtrb::Consumer<f32>,
    state: Arc<PlayState>,
}

impl PlaybackTap {
    /// Called once per output buffer, before any frame is taken.
    pub fn begin_buffer(&mut self) {
        if self.state.discard.load(Ordering::Acquire) {
            let n = self.ring.slots();
            if let Ok(chunk) = self.ring.read_chunk(n) {
                chunk.commit_all();
            }
            self.state.discard.store(false, Ordering::Release);
        }
    }

    /// The next stereo frame for output frame `out_frame`, if playback is running.
    #[inline]
    pub fn next_frame(&mut self, out_frame: u64) -> Option<(f32, f32)> {
        if !self.state.playing.load(Ordering::Relaxed)
            || out_frame < self.state.gate_out.load(Ordering::Relaxed)
            || self.ring.slots() < 2
        {
            return None;
        }
        let left = self.ring.pop().ok()?;
        let right = self.ring.pop().ok()?;
        self.state.position.fetch_add(1, Ordering::Relaxed);
        Some((left, right))
    }
}

enum Command {
    Load(Vec<PathBuf>),
    Play {
        from: u64,
        until: Option<u64>,
        gate_out: u64,
    },
    Stop,
    Shutdown,
}

/// Left/right gain of each take track during playback.
pub type Gains = Arc<Mutex<Vec<(f32, f32)>>>;

/// UI-side handle of the playback thread.
pub struct Player {
    commands: Sender<Command>,
    state: Arc<PlayState>,
    gains: Gains,
    thread: Option<JoinHandle<()>>,
}

impl Player {
    /// Spawns the reader thread and returns the handle plus the callback's end of the ring.
    pub fn spawn() -> std::io::Result<(Self, PlaybackTap)> {
        let (producer, consumer) = rtrb::RingBuffer::new(RING_FRAMES * 2);
        let state = Arc::new(PlayState::default());
        let gains: Gains = Arc::default();
        let (tx, rx) = crossbeam_channel::unbounded();
        let worker = Worker {
            ring: producer,
            commands: rx,
            state: state.clone(),
            gains: gains.clone(),
            readers: Vec::new(),
            read_pos: 0,
            end: 0,
            scratch: Vec::new(),
            mix: Vec::new(),
        };
        let thread = thread::Builder::new()
            .name("recrec-player".into())
            .spawn(move || worker.run())?;
        Ok((
            Self {
                commands: tx,
                state: state.clone(),
                gains,
                thread: Some(thread),
            },
            PlaybackTap {
                ring: consumer,
                state,
            },
        ))
    }

    fn send(&self, cmd: Command) {
        if self.commands.send(cmd).is_err() {
            log::error!("player thread is gone; command dropped");
        }
    }

    /// Selects the files to play (one per take track). Stops any playback.
    pub fn load(&self, files: Vec<PathBuf>) {
        self.send(Command::Load(files));
    }

    /// Plays from take frame `from` (to `until`, or the end). The callback starts consuming
    /// at output frame `gate_out` (0 = immediately).
    pub fn play(&self, from: u64, until: Option<u64>, gate_out: u64) {
        self.send(Command::Play {
            from,
            until,
            gate_out,
        });
    }

    pub fn stop(&self) {
        self.send(Command::Stop);
    }

    pub fn is_playing(&self) -> bool {
        self.state.is_playing()
    }

    /// Take frame being heard right now.
    pub fn position(&self) -> u64 {
        self.state.position()
    }

    pub fn set_gains(&self, gains: &[(f32, f32)]) {
        match self.gains.lock() {
            Ok(mut g) => {
                g.clear();
                g.extend_from_slice(gains);
            }
            Err(_) => log::error!("playback gains lock poisoned"),
        }
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        if let Some(thread) = self.thread.take() {
            // A closed channel means the thread already exited; joining is all that is left.
            let _ = self.commands.send(Command::Shutdown);
            if thread.join().is_err() {
                log::error!("player thread panicked");
            }
        }
    }
}

struct Worker {
    ring: rtrb::Producer<f32>,
    commands: Receiver<Command>,
    state: Arc<PlayState>,
    gains: Gains,
    readers: Vec<WavReader>,
    read_pos: u64,
    end: u64,
    scratch: Vec<f32>,
    mix: Vec<f32>,
}

impl Worker {
    fn run(mut self) {
        loop {
            let wait = if self.state.is_playing() {
                self.fill();
                IDLE_POLL
            } else {
                Duration::from_millis(50)
            };
            match self.commands.recv_timeout(wait) {
                Ok(Command::Load(files)) => self.load(files),
                Ok(Command::Play {
                    from,
                    until,
                    gate_out,
                }) => self.play(from, until, gate_out),
                Ok(Command::Stop) => self.halt(),
                Ok(Command::Shutdown) | Err(RecvTimeoutError::Disconnected) => {
                    self.halt();
                    return;
                }
                Err(RecvTimeoutError::Timeout) => {}
            }
        }
    }

    /// Stops playback and has the callback throw away what is still queued.
    fn halt(&mut self) {
        self.state.playing.store(false, Ordering::Relaxed);
        self.state.discard.store(true, Ordering::Release);
    }

    fn load(&mut self, files: Vec<PathBuf>) {
        self.halt();
        self.readers.clear();
        for file in files {
            match WavReader::open(&file) {
                Ok(r) => self.readers.push(r),
                Err(e) => log::error!("cannot open {} for playback: {e}", file.display()),
            }
        }
    }

    fn play(&mut self, from: u64, until: Option<u64>, gate_out: u64) {
        self.halt();
        // Wait for the callback to empty the ring, so stale audio is never heard. Without a
        // running output stream nobody clears the flag; give up after a moment.
        let deadline = Instant::now() + DISCARD_TIMEOUT;
        while self.state.discard.load(Ordering::Acquire) && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(1));
        }
        // Never leave the request pending: it would throw away the audio queued below.
        self.state.discard.store(false, Ordering::Release);
        let total = self
            .readers
            .iter()
            .map(WavReader::frames)
            .max()
            .unwrap_or(0);
        self.end = until.map_or(total, |u| u.min(total));
        self.read_pos = from.min(self.end);
        self.state.position.store(self.read_pos, Ordering::Relaxed);
        self.state.gate_out.store(gate_out, Ordering::Relaxed);
        if self.read_pos < self.end {
            self.fill();
            self.state.playing.store(true, Ordering::Relaxed);
        }
    }

    /// Decodes and mixes as much as fits into the ring.
    fn fill(&mut self) {
        while self.read_pos < self.end && self.ring.slots() >= CHUNK_FRAMES * 2 {
            let frames = ((self.end - self.read_pos) as usize).min(CHUNK_FRAMES);
            self.mix.clear();
            self.mix.resize(frames * 2, 0.0);
            let gains = match self.gains.lock() {
                Ok(g) => g.clone(),
                Err(_) => Vec::new(),
            };
            for (i, reader) in self.readers.iter_mut().enumerate() {
                let (gl, gr) = gains.get(i).copied().unwrap_or((1.0, 1.0));
                if gl == 0.0 && gr == 0.0 {
                    continue;
                }
                self.scratch.clear();
                let channels = usize::from(reader.spec().channels);
                if let Err(e) = reader.read(self.read_pos, frames, &mut self.scratch) {
                    log::error!("playback read failed: {e}");
                    continue;
                }
                for (out, frame) in self
                    .mix
                    .as_chunks_mut::<2>()
                    .0
                    .iter_mut()
                    .zip(self.scratch.chunks_exact(channels))
                {
                    // Mono files go to both sides; stereo files keep their sides.
                    let (l, r) = match frame {
                        [l, r, ..] => (*l, *r),
                        [m] => (*m, *m),
                        [] => (0.0, 0.0),
                    };
                    out[0] += l * gl;
                    out[1] += r * gr;
                }
            }
            if let Ok(chunk) = self.ring.write_chunk_uninit(frames * 2) {
                chunk.fill_from_iter(self.mix.iter().copied());
            }
            self.read_pos += frames as u64;
        }
        // Everything was queued and the callback has played it all: the take is over.
        if self.read_pos >= self.end && self.ring.slots() == RING_FRAMES * 2 {
            self.state.playing.store(false, Ordering::Relaxed);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wav::{Bext, SampleFormat, WavSpec, WavWriter};
    use chrono::NaiveDate;
    use std::path::Path;

    fn write_wav(path: &Path, channels: u16, samples: &[f32]) {
        let t = NaiveDate::from_ymd_opt(2026, 1, 1)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap();
        let spec = WavSpec {
            channels,
            sample_rate: 48_000,
            format: SampleFormat::Float32,
        };
        let mut w = WavWriter::create(path, spec, &Bext::at(t, 48_000, String::new()), 0).unwrap();
        w.write_samples(samples).unwrap();
        w.finalize().unwrap();
    }

    /// Plays the role of the output callback until playback ends.
    fn consume(tap: &mut PlaybackTap, player: &Player, out_frame: u64) -> Vec<(f32, f32)> {
        let mut heard = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut started = false;
        while Instant::now() < deadline {
            tap.begin_buffer();
            let mut got = false;
            while let Some(f) = tap.next_frame(out_frame) {
                heard.push(f);
                got = true;
            }
            started |= got || player.is_playing();
            if started && !player.is_playing() && !got {
                break;
            }
            thread::sleep(Duration::from_millis(1));
        }
        heard
    }

    #[test]
    fn plays_a_range_mixed_with_the_track_gains() {
        let dir = tempfile::tempdir().unwrap();
        let mono = dir.path().join("mono.wav");
        let stereo = dir.path().join("stereo.wav");
        let n = 3000;
        write_wav(
            &mono,
            1,
            &(0..n).map(|i| i as f32 / 10_000.0).collect::<Vec<_>>(),
        );
        write_wav(
            &stereo,
            2,
            &(0..n).flat_map(|_| [0.25, -0.25]).collect::<Vec<_>>(),
        );
        let (player, mut tap) = Player::spawn().unwrap();
        player.load(vec![mono, stereo]);
        player.set_gains(&[(1.0, 0.5), (1.0, 1.0)]);
        player.play(1000, Some(2500), 0);
        let heard = consume(&mut tap, &player, 0);

        assert_eq!(heard.len(), 1500);
        let (l, r) = heard[10];
        assert!((l - (0.1010 + 0.25)).abs() < 1e-5, "{l}");
        assert!((r - (0.1010 * 0.5 - 0.25)).abs() < 1e-5, "{r}");
        assert_eq!(player.position(), 2500);
        assert!(!player.is_playing());
    }

    #[test]
    fn nothing_is_consumed_before_the_gate() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.wav");
        write_wav(&file, 1, &[0.5; 100]);
        let (player, mut tap) = Player::spawn().unwrap();
        player.load(vec![file]);
        player.play(0, None, 1000);
        let deadline = Instant::now() + Duration::from_secs(5);
        while !player.is_playing() {
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(1));
        }
        tap.begin_buffer();
        assert_eq!(tap.next_frame(999), None);
        assert_eq!(player.position(), 0);
        assert_eq!(tap.next_frame(1000), Some((0.5, 0.5)));
        assert_eq!(player.position(), 1);
    }

    #[test]
    fn stop_discards_queued_audio() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.wav");
        write_wav(&file, 1, &vec![0.5; 20_000]);
        let (player, mut tap) = Player::spawn().unwrap();
        player.load(vec![file]);
        player.play(0, None, 0);
        let deadline = Instant::now() + Duration::from_secs(5);
        while !player.is_playing() {
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(1));
        }
        player.stop();
        while player.is_playing() {
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(1));
        }
        tap.begin_buffer();
        assert_eq!(tap.next_frame(0), None);
        assert_eq!(tap.ring.slots(), 0);
    }
}
