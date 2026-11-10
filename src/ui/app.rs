//! Application state and the actions the view triggers.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use chrono::{DateTime, Local};
use cpal::HostId;
use eframe::egui;

use crate::audio::devices::{self, DeviceInfo};
use crate::audio::stream::InputSession;
use crate::config::{self, AppConfig};
use crate::meter::MeterDisplay;
use crate::platform::SleepGuard;
use crate::recorder::{RecorderEvent, TakeRequest, TrackRequest};
use crate::take::{self, TakeEntry};
use crate::track::{self, Track};

const CONFIG_SAVE_DELAY: Duration = Duration::from_secs(1);
const FREE_SPACE_INTERVAL: Duration = Duration::from_secs(2);
const MAX_MESSAGES: usize = 50;
const TAKE_LIST_LIMIT: usize = 50;
pub(super) const REPAINT_INTERVAL: Duration = Duration::from_millis(33);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Level {
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone)]
pub(super) struct Message {
    pub level: Level,
    pub text: String,
    pub at: DateTime<Local>,
}

pub struct RecrecApp {
    pub(super) config: AppConfig,
    config_path: Option<PathBuf>,
    config_dirty_since: Option<Instant>,
    pub(super) hosts: Vec<HostId>,
    pub(super) host: HostId,
    pub(super) devices: Vec<DeviceInfo>,
    pub(super) device: Option<String>,
    pub(super) session: Option<InputSession>,
    pub(super) session_error: Option<String>,
    pub(super) tracks: Vec<Track>,
    pub(super) meters: Vec<MeterDisplay>,
    last_meter_time: Option<f64>,
    pub(super) take_label: String,
    pub(super) takes: Vec<TakeEntry>,
    pub(super) recording_dir: Option<PathBuf>,
    start_pending: bool,
    pub(super) dropped_at_take_start: u64,
    sleep_guard: Option<SleepGuard>,
    pub(super) messages: Vec<Message>,
    pub(super) free_space: Option<u64>,
    last_space_check: Option<Instant>,
    pub(super) pending_drag: Option<Vec<PathBuf>>,
}

impl RecrecApp {
    /// Loads settings, repairs crashed takes and opens the last used device.
    pub fn launch() -> Self {
        let config_path = config::config_path();
        let (config, warning) = match &config_path {
            Some(path) => AppConfig::load(path),
            None => (
                AppConfig::default(),
                Some("設定の保存場所が見つかりません".to_owned()),
            ),
        };
        let host = devices::preferred_host(config.host.as_deref());
        let mut app = Self {
            hosts: devices::host_ids(),
            host,
            config,
            config_path,
            config_dirty_since: None,
            devices: Vec::new(),
            device: None,
            session: None,
            session_error: None,
            tracks: Vec::new(),
            meters: Vec::new(),
            last_meter_time: None,
            take_label: String::new(),
            takes: Vec::new(),
            recording_dir: None,
            start_pending: false,
            dropped_at_take_start: 0,
            sleep_guard: None,
            messages: Vec::new(),
            free_space: None,
            last_space_check: None,
            pending_drag: None,
        };
        if let Some(w) = warning {
            app.push(Level::Warn, w);
        }
        app.recover_takes();
        app.refresh_takes();
        app.reload_devices();
        app.open_session();
        app
    }

    pub(super) fn push(&mut self, level: Level, text: impl Into<String>) {
        let text = text.into();
        match level {
            Level::Info => log::info!("{text}"),
            Level::Warn => log::warn!("{text}"),
            Level::Error => log::error!("{text}"),
        }
        self.messages.push(Message {
            level,
            text,
            at: Local::now(),
        });
        if self.messages.len() > MAX_MESSAGES {
            self.messages.remove(0);
        }
    }

    fn recover_takes(&mut self) {
        let root = self.config.output_dir();
        match take::recover_takes(&root) {
            Ok(reports) => {
                for r in reports {
                    let name = r.dir.display().to_string();
                    if r.errors.is_empty() {
                        self.push(
                            Level::Warn,
                            format!(
                                "前回の異常終了で残ったテイクを修復しました: {name} ({} ファイル)",
                                r.repaired_files
                            ),
                        );
                    } else {
                        self.push(
                            Level::Error,
                            format!(
                                "テイク {name} の修復に失敗しました: {}",
                                r.errors.join(" / ")
                            ),
                        );
                    }
                }
            }
            Err(e) => self.push(Level::Error, format!("出力先を確認できません: {e}")),
        }
    }

    pub(super) fn refresh_takes(&mut self) {
        match take::list_takes(&self.config.output_dir(), TAKE_LIST_LIMIT) {
            Ok(t) => self.takes = t,
            Err(e) => self.push(Level::Error, format!("テイク一覧を読めません: {e}")),
        }
    }

    pub(super) fn is_busy(&self) -> bool {
        self.start_pending || self.recording_dir.is_some()
    }

    pub(super) fn current_device_info(&self) -> Option<&DeviceInfo> {
        let name = self.device.as_deref()?;
        self.devices.iter().find(|d| d.name == name)
    }

    fn device_key(&self) -> Option<String> {
        self.device
            .as_deref()
            .map(|d| config::device_key(self.host.name(), d))
    }

    fn close_session(&mut self) {
        if let Some(session) = self.session.take() {
            let events = session.close();
            for e in events {
                self.handle_recorder_event(e);
            }
        }
        self.meters.clear();
    }

    /// Re-enumerates devices of the current host. Closes the stream first (ASIO needs that).
    pub(super) fn reload_devices(&mut self) {
        self.close_session();
        match devices::list_input_devices(self.host) {
            Ok(list) => self.devices = list,
            Err(e) => {
                self.devices.clear();
                self.push(Level::Error, format!("{e:#}"));
            }
        }
        let saved = self.config.device.clone();
        let default = devices::default_input_device_name(self.host);
        let has = |name: &Option<String>| {
            name.as_ref()
                .is_some_and(|n| self.devices.iter().any(|d| &d.name == n))
        };
        self.device = if has(&saved) {
            saved
        } else if has(&default) {
            default
        } else {
            self.devices.first().map(|d| d.name.clone())
        };
    }

    pub(super) fn open_session(&mut self) {
        self.close_session();
        self.session_error = None;
        let Some(info) = self.current_device_info().cloned() else {
            self.session_error = Some("入力デバイスがありません".into());
            self.tracks.clear();
            return;
        };
        let rate = info.pick_rate(self.config.sample_rate);
        let buffer = self
            .config
            .buffer_size
            .filter(|b| info.buffer_choices().contains(b));
        match InputSession::open(self.host, &info.name, rate, buffer) {
            Ok(session) => {
                session.recorder().set_preroll(self.config.preroll_secs);
                self.meters = vec![MeterDisplay::default(); usize::from(session.info.channels)];
                self.session = Some(session);
            }
            Err(e) => {
                let msg = format!("{e:#}");
                self.push(Level::Error, msg.clone());
                self.session_error = Some(msg);
            }
        }
        self.load_tracks(info.channels);
    }

    fn load_tracks(&mut self, channels: u16) {
        let saved = self
            .device_key()
            .and_then(|k| self.config.tracks.get(&k).cloned())
            .unwrap_or_default();
        let valid = track::retain_valid(&saved, channels);
        if valid.len() < saved.len() {
            self.push(
                Level::Warn,
                "保存されていたトラックの一部がこのデバイスに無い入力を指していたため外しました",
            );
        }
        self.tracks = if valid.is_empty() {
            track::default_tracks(channels)
        } else {
            valid
        };
    }

    pub(super) fn select_host(&mut self, host: HostId) {
        if host == self.host {
            return;
        }
        self.host = host;
        self.config.host = Some(host.name().to_owned());
        self.mark_dirty();
        self.reload_devices();
        self.open_session();
    }

    pub(super) fn select_device(&mut self, name: String) {
        if self.device.as_ref() == Some(&name) {
            return;
        }
        self.config.device = Some(name.clone());
        self.device = Some(name);
        self.mark_dirty();
        self.open_session();
    }

    pub(super) fn select_rate(&mut self, rate: u32) {
        self.config.sample_rate = Some(rate);
        self.mark_dirty();
        self.open_session();
    }

    pub(super) fn select_buffer(&mut self, buffer: Option<u32>) {
        self.config.buffer_size = buffer;
        self.mark_dirty();
        self.open_session();
    }

    pub(super) fn reconnect(&mut self) {
        self.reload_devices();
        self.open_session();
    }

    pub(super) fn set_preroll(&mut self, secs: u32) {
        self.config.preroll_secs = secs;
        if let Some(s) = &self.session {
            s.recorder().set_preroll(secs);
        }
        self.mark_dirty();
    }

    pub(super) fn set_output_dir(&mut self, dir: PathBuf) {
        self.config.output_dir = Some(dir);
        self.mark_dirty();
        self.free_space = None;
        self.last_space_check = None;
        self.recover_takes();
        self.refresh_takes();
    }

    pub(super) fn tracks_changed(&mut self) {
        if let Some(key) = self.device_key() {
            self.config.tracks.insert(key, self.tracks.clone());
            self.mark_dirty();
        }
    }

    pub(super) fn reset_tracks(&mut self) {
        if let Some(s) = &self.session {
            self.tracks = track::default_tracks(s.info.channels);
            self.tracks_changed();
        }
    }

    pub(super) fn add_track(&mut self) {
        let Some(s) = &self.session else { return };
        let used: Vec<usize> = self
            .tracks
            .iter()
            .flat_map(|t| t.source.channels())
            .collect();
        let channel = (0..s.info.channels)
            .find(|c| !used.contains(&usize::from(*c)))
            .unwrap_or(0);
        let mut t = Track::mono(channel);
        t.name = format!("Track{}", self.tracks.len() + 1);
        self.tracks.push(t);
        self.tracks_changed();
    }

    pub(super) fn armed_bytes_per_second(&self) -> u64 {
        let Some(s) = &self.session else { return 0 };
        let per_channel =
            u64::from(s.info.sample_rate) * u64::from(self.config.format.bytes_per_sample());
        self.tracks
            .iter()
            .filter(|t| t.armed)
            .map(|t| u64::from(t.source.width()) * per_channel)
            .sum()
    }

    pub(super) fn can_record(&self) -> bool {
        self.session.is_some()
            && self.session_error.is_none()
            && self.tracks.iter().any(|t| t.armed)
    }

    pub(super) fn toggle_recording(&mut self) {
        if self.is_busy() {
            self.stop_recording();
        } else {
            self.start_recording();
        }
    }

    fn start_recording(&mut self) {
        if !self.can_record() {
            return;
        }
        let Some(session) = &self.session else { return };
        let request = TakeRequest {
            root: self.config.output_dir(),
            label: self.take_label.trim().to_owned(),
            tracks: self
                .tracks
                .iter()
                .filter(|t| t.armed)
                .map(|t| TrackRequest {
                    name: t.name.clone(),
                    channels: t.source.channels(),
                })
                .collect(),
            format: self.config.format,
            host: session.info.host.clone(),
            device: session.info.device.clone(),
        };
        self.dropped_at_take_start = session.dropped_frames();
        session.recorder().start(request);
        self.start_pending = true;
    }

    fn stop_recording(&mut self) {
        if let Some(s) = &self.session {
            s.recorder().stop();
        }
    }

    fn handle_recorder_event(&mut self, event: RecorderEvent) {
        match event {
            RecorderEvent::Started { dir } => {
                self.start_pending = false;
                self.sleep_guard = Some(SleepGuard::new());
                self.push(Level::Info, format!("録音開始: {}", dir.display()));
                self.recording_dir = Some(dir);
            }
            RecorderEvent::Stopped(summary) => {
                self.start_pending = false;
                self.recording_dir = None;
                self.sleep_guard = None;
                let secs = self.session.as_ref().map_or(0.0, |s| {
                    summary.frames as f64 / f64::from(s.info.sample_rate)
                });
                let mut text = format!(
                    "録音を保存しました: {} ({} ファイル, {})",
                    summary.dir.display(),
                    summary.files.len(),
                    format_duration(secs)
                );
                if summary.dropped_frames > 0 {
                    text.push_str(&format!(
                        " — 取りこぼし {} フレーム",
                        summary.dropped_frames
                    ));
                }
                let level = if summary.errors.is_empty() && summary.dropped_frames == 0 {
                    Level::Info
                } else {
                    Level::Warn
                };
                self.push(level, text);
                for e in summary.errors {
                    self.push(Level::Error, e);
                }
                self.refresh_takes();
            }
            RecorderEvent::Error(msg) => {
                self.start_pending = false;
                self.push(Level::Error, msg);
            }
        }
    }

    /// Pulls recorder events, stream errors and meter peaks. Called once per frame.
    pub(super) fn poll(&mut self, now: f64) {
        let mut events = Vec::new();
        let mut stream_errors = Vec::new();
        if let Some(s) = &self.session {
            while let Some(e) = s.recorder().try_event() {
                events.push(e);
            }
            while let Some(e) = s.try_stream_error() {
                stream_errors.push(e);
            }
        }
        for e in events {
            self.handle_recorder_event(e);
        }
        if !stream_errors.is_empty() {
            self.stop_recording();
            for e in &stream_errors {
                self.push(Level::Error, format!("オーディオデバイスのエラー: {e}"));
            }
            self.session_error = Some(format!(
                "デバイスとの接続に問題があります。⟳ で再接続してください ({})",
                stream_errors.join(" / ")
            ));
        }

        let dt = self.last_meter_time.map_or(0.0, |t| (now - t) as f32);
        self.last_meter_time = Some(now);
        if let Some(s) = &self.session {
            for (ch, m) in self.meters.iter_mut().enumerate() {
                m.update(s.meters.take_peak(ch), now, dt);
            }
        }

        if self
            .last_space_check
            .is_none_or(|t| t.elapsed() >= FREE_SPACE_INTERVAL)
        {
            self.last_space_check = Some(Instant::now());
            let dir = self.config.output_dir();
            let probe = if dir.exists() {
                dir
            } else {
                dir.ancestors()
                    .find(|p| p.exists())
                    .map(|p| p.to_path_buf())
                    .unwrap_or(dir)
            };
            self.free_space = fs2::available_space(&probe).ok();
        }

        if self
            .config_dirty_since
            .is_some_and(|t| t.elapsed() >= CONFIG_SAVE_DELAY)
        {
            self.save_config();
        }
    }

    pub(super) fn mark_dirty(&mut self) {
        if self.config_dirty_since.is_none() {
            self.config_dirty_since = Some(Instant::now());
        }
    }

    fn save_config(&mut self) {
        self.config_dirty_since = None;
        if let Some(path) = self.config_path.clone()
            && let Err(e) = self.config.save(&path)
        {
            self.push(Level::Error, format!("設定を保存できません: {e}"));
        }
    }

    /// Finalizes any take, closes the device and saves settings.
    pub(super) fn shutdown(&mut self) {
        self.close_session();
        self.save_config();
    }

    pub(super) fn elapsed_secs(&self) -> f64 {
        match &self.session {
            Some(s) if self.recording_dir.is_some() => {
                s.recorder().frames_recorded() as f64 / f64::from(s.info.sample_rate)
            }
            _ => 0.0,
        }
    }

    pub(super) fn take_drag(&mut self, files: Vec<PathBuf>) {
        self.pending_drag = Some(files);
    }

    /// Runs a pending drag-out. Must be called after the UI pass (it blocks until the drop).
    pub(super) fn run_pending_drag(&mut self, frame: &eframe::Frame) {
        if let Some(files) = self.pending_drag.take()
            && let Err(e) = crate::platform::start_file_drag(frame, files)
        {
            self.push(Level::Error, format!("ドラッグを開始できません: {e}"));
        }
    }

    pub(super) fn open_folder(&mut self, path: &std::path::Path) {
        if let Err(e) = crate::platform::open_in_file_manager(path) {
            self.push(
                Level::Error,
                format!("フォルダを開けません ({}): {e}", path.display()),
            );
        }
    }
}

impl eframe::App for RecrecApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        self.draw(ui);
        self.run_pending_drag(frame);
        ui.ctx().request_repaint_after(REPAINT_INTERVAL);
    }

    fn on_exit(&mut self) {
        self.shutdown();
    }
}

impl Drop for RecrecApp {
    fn drop(&mut self) {
        // on_exit normally ran already; this covers abnormal teardown paths.
        self.close_session();
    }
}

pub(super) fn format_duration(secs: f64) -> String {
    let total = secs.max(0.0);
    let h = (total / 3600.0) as u64;
    let m = ((total % 3600.0) / 60.0) as u64;
    let s = total % 60.0;
    if h > 0 {
        format!("{h}:{m:02}:{s:04.1}")
    } else {
        format!("{m:02}:{s:04.1}")
    }
}

pub(super) fn format_bytes(bytes: u64) -> String {
    const GB: f64 = 1024.0 * 1024.0 * 1024.0;
    const MB: f64 = 1024.0 * 1024.0;
    let b = bytes as f64;
    if b >= GB {
        format!("{:.1} GB", b / GB)
    } else {
        format!("{:.0} MB", b / MB)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_format_like_a_recorder_display() {
        assert_eq!(format_duration(0.0), "00:00.0");
        assert_eq!(format_duration(201.44), "03:21.4");
        assert_eq!(format_duration(3_725.0), "1:02:05.0");
    }

    #[test]
    fn bytes_format_in_mb_or_gb() {
        assert_eq!(format_bytes(500 * 1024 * 1024), "500 MB");
        assert_eq!(format_bytes(3 * 1024 * 1024 * 1024 / 2), "1.5 GB");
    }
}
