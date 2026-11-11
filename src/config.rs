//! Persisted user settings.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use directories::{ProjectDirs, UserDirs};
use serde::{Deserialize, Serialize};

use crate::track::Track;
use crate::wav::SampleFormat;

const APP_NAME: &str = "recrec";
const CONFIG_FILE: &str = "config.json";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub host: Option<String>,
    pub device: Option<String>,
    pub sample_rate: Option<u32>,
    /// `None` keeps the driver's buffer size.
    pub buffer_size: Option<u32>,
    pub format: SampleFormat,
    pub preroll_secs: u32,
    pub output_dir: Option<PathBuf>,
    /// Track layouts keyed by [`device_key`], so each interface keeps its own.
    pub tracks: BTreeMap<String, Vec<Track>>,
    /// `true` turns off the update check at launch.
    pub skip_update_check: bool,
    /// A release the user chose not to be told about again.
    pub skipped_version: Option<String>,
    pub metronome: MetronomeConfig,
    /// Master level of everything the app plays (monitor mix and click), linear 0..1.
    pub output_volume: f32,
    /// Shifts the start of a counted-in take to make up for the interface's round-trip
    /// latency, in milliseconds (positive = start later).
    pub latency_comp_ms: f32,
    /// Do not keep the files of tracks that stayed silent for a whole take.
    pub skip_silent_tracks: bool,
    /// Whether the take list is folded away to give the tracks more room.
    pub takes_collapsed: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MetronomeConfig {
    /// Master switch: off means no click and no count-in.
    pub enabled: bool,
    pub bpm: f64,
    pub beats_per_bar: u32,
    /// Note value of a beat: 4 or 8.
    pub beat_unit: u32,
    /// Bars counted in before recording starts (0 = start at once).
    pub count_in_bars: u32,
    /// Keep clicking while recording (the count-in always clicks).
    pub click_while_recording: bool,
    /// Click while idle, for practising.
    pub click_while_idle: bool,
    /// Click level, linear 0..1.
    pub volume: f32,
}

impl Default for MetronomeConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            bpm: 120.0,
            beats_per_bar: 4,
            beat_unit: 4,
            count_in_bars: 1,
            click_while_recording: true,
            click_while_idle: false,
            volume: 0.6,
        }
    }
}

pub fn device_key(host: &str, device: &str) -> String {
    format!("{host}/{device}")
}

fn project_dirs() -> Option<ProjectDirs> {
    ProjectDirs::from("", "", APP_NAME)
}

pub fn config_path() -> Option<PathBuf> {
    project_dirs().map(|d| d.config_dir().join(CONFIG_FILE))
}

pub fn log_dir() -> Option<PathBuf> {
    project_dirs().map(|d| d.data_local_dir().join("logs"))
}

/// Where a downloaded installer is kept until it runs.
pub fn update_dir() -> Option<PathBuf> {
    project_dirs().map(|d| d.data_local_dir().join("updates"))
}

/// `<Music>/recrec`, falling back to the home directory and then the working directory.
pub fn default_output_dir() -> PathBuf {
    let user = UserDirs::new();
    let base = user
        .as_ref()
        .and_then(|u| u.audio_dir().map(Path::to_path_buf))
        .or_else(|| user.as_ref().map(|u| u.home_dir().to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join(APP_NAME)
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            host: None,
            device: None,
            sample_rate: None,
            buffer_size: None,
            format: SampleFormat::default(),
            preroll_secs: 0,
            output_dir: None,
            tracks: BTreeMap::new(),
            skip_update_check: false,
            skipped_version: None,
            metronome: MetronomeConfig::default(),
            output_volume: 1.0,
            latency_comp_ms: 0.0,
            skip_silent_tracks: true,
            takes_collapsed: false,
        }
    }
}

impl AppConfig {
    pub fn output_dir(&self) -> PathBuf {
        self.output_dir.clone().unwrap_or_else(default_output_dir)
    }

    /// Loads settings. A missing file gives defaults; an unreadable one is moved to
    /// `config.json.bak` and the returned warning explains what happened.
    pub fn load(path: &Path) -> (Self, Option<String>) {
        let bytes = match fs::read(path) {
            Ok(b) => b,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return (Self::default(), None),
            Err(e) => {
                return (
                    Self::default(),
                    Some(format!(
                        "設定ファイルを読めません ({}): {e}",
                        path.display()
                    )),
                );
            }
        };
        match serde_json::from_slice(&bytes) {
            Ok(cfg) => (cfg, None),
            Err(e) => {
                let backup = path.with_extension("json.bak");
                let moved = fs::rename(path, &backup);
                let warning = match moved {
                    Ok(()) => format!(
                        "設定ファイルが壊れていたため既定値で起動しました (退避先: {}): {e}",
                        backup.display()
                    ),
                    Err(mv) => format!(
                        "設定ファイルが壊れていたため既定値で起動しました ({e})。退避にも失敗しました: {mv}"
                    ),
                };
                (Self::default(), Some(warning))
            }
        }
    }

    /// Writes atomically (temp file + rename) so a crash never leaves half a file.
    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_vec_pretty(self).map_err(io::Error::other)?;
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, json)?;
        fs::rename(tmp, path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::track::{Source, Track};

    #[test]
    fn save_then_load_roundtrips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub").join(CONFIG_FILE);
        let mut cfg = AppConfig {
            host: Some("ASIO".into()),
            device: Some("Audio Interface".into()),
            sample_rate: Some(96_000),
            buffer_size: Some(256),
            format: SampleFormat::Float32,
            preroll_secs: 5,
            output_dir: Some(dir.path().join("out")),
            tracks: BTreeMap::new(),
            skip_update_check: true,
            skipped_version: Some("0.2.0".into()),
            metronome: MetronomeConfig {
                enabled: true,
                bpm: 96.5,
                beats_per_bar: 6,
                beat_unit: 8,
                count_in_bars: 2,
                click_while_recording: false,
                click_while_idle: true,
                volume: 0.4,
            },
            output_volume: 0.8,
            latency_comp_ms: 12.5,
            skip_silent_tracks: false,
            takes_collapsed: true,
        };
        cfg.tracks.insert(
            device_key("ASIO", "Audio Interface"),
            vec![Track {
                armed: false,
                monitor: true,
                volume_db: -3.0,
                ..Track::new("Synth".into(), Source::Stereo(2, 3))
            }],
        );
        cfg.save(&path).unwrap();
        let (loaded, warning) = AppConfig::load(&path);
        assert_eq!(warning, None);
        assert_eq!(loaded, cfg);
    }

    #[test]
    fn missing_file_gives_defaults_silently() {
        let dir = tempfile::tempdir().unwrap();
        let (cfg, warning) = AppConfig::load(&dir.path().join(CONFIG_FILE));
        assert_eq!(cfg, AppConfig::default());
        assert_eq!(cfg.format, SampleFormat::Pcm24);
        assert_eq!(warning, None);
    }

    #[test]
    fn corrupt_file_is_backed_up_and_reported() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CONFIG_FILE);
        fs::write(&path, b"{ not json").unwrap();
        let (cfg, warning) = AppConfig::load(&path);
        assert_eq!(cfg, AppConfig::default());
        assert!(warning.is_some());
        assert!(!path.exists());
        assert_eq!(
            fs::read(dir.path().join("config.json.bak")).unwrap(),
            b"{ not json"
        );
    }

    #[test]
    fn unknown_and_missing_fields_are_tolerated() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CONFIG_FILE);
        fs::write(&path, br#"{"preroll_secs": 3, "future_key": true}"#).unwrap();
        let (cfg, warning) = AppConfig::load(&path);
        assert_eq!(warning, None);
        assert_eq!(cfg.preroll_secs, 3);
        assert_eq!(cfg.format, SampleFormat::Pcm24);
        assert!(!cfg.skip_update_check);
        assert_eq!(cfg.skipped_version, None);
    }
}
