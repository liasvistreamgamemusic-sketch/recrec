//! Take folders on disk: naming, metadata, the in-progress marker, crash recovery and listing.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

use crate::wav::{self, SampleFormat};

pub const META_FILE: &str = "take.json";
/// Exists while a take is being written; left behind if the app dies mid-take.
pub const MARKER_FILE: &str = ".recording";
const MAX_NAME_CHARS: usize = 60;
const WINDOWS_RESERVED: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Makes `name` safe as a single path component on Windows (and everywhere else).
pub fn sanitize_component(name: &str) -> String {
    let replaced: String = name
        .chars()
        .map(|c| match c {
            '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .take(MAX_NAME_CHARS)
        .collect();
    let trimmed = replaced.trim().trim_end_matches(['.', ' ']).to_owned();
    let stem = trimmed.split('.').next().unwrap_or_default();
    if WINDOWS_RESERVED
        .iter()
        .any(|r| r.eq_ignore_ascii_case(stem))
    {
        format!("_{trimmed}")
    } else {
        trimmed
    }
}

/// `YYYY-MM-DD_HHMMSS` plus `_<label>` when a take name was given.
pub fn take_dir_name(start: NaiveDateTime, label: &str) -> String {
    let stamp = start.format("%Y-%m-%d_%H%M%S").to_string();
    let label = sanitize_component(label);
    if label.is_empty() {
        stamp
    } else {
        format!("{stamp}_{label}")
    }
}

/// `NN_<track name>.wav`, numbered from 1.
pub fn track_file_name(index: usize, track_name: &str) -> String {
    let name = sanitize_component(track_name);
    let name = if name.is_empty() {
        "Track".to_owned()
    } else {
        name
    };
    format!("{:02}_{name}.wav", index + 1)
}

/// Creates a fresh take folder under `root`, adding `_2`, `_3`… if the name is taken.
pub fn create_take_dir(root: &Path, start: NaiveDateTime, label: &str) -> io::Result<PathBuf> {
    fs::create_dir_all(root)?;
    let base = take_dir_name(start, label);
    for n in 1u32.. {
        let name = if n == 1 {
            base.clone()
        } else {
            format!("{base}_{n}")
        };
        let dir = root.join(name);
        match fs::create_dir(&dir) {
            Ok(()) => return Ok(dir),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    unreachable!("u32 suffixes exhausted")
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrackMeta {
    pub name: String,
    /// 1-based input channel numbers, as printed on the interface.
    pub inputs: Vec<u16>,
    pub file: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TakeMeta {
    pub app_version: String,
    pub host: String,
    pub device: String,
    pub sample_rate: u32,
    pub format: SampleFormat,
    /// Local time of the first sample (pre-roll included), ISO 8601.
    pub started_at: String,
    pub tracks: Vec<TrackMeta>,
    pub frames: u64,
    pub preroll_frames: u64,
    pub dropped_frames: u64,
    pub recovered: bool,
}

impl TakeMeta {
    pub fn duration_secs(&self) -> f64 {
        if self.sample_rate == 0 {
            0.0
        } else {
            self.frames as f64 / f64::from(self.sample_rate)
        }
    }
}

pub fn write_meta(dir: &Path, meta: &TakeMeta) -> io::Result<()> {
    let json = serde_json::to_vec_pretty(meta).map_err(io::Error::other)?;
    let tmp = dir.join(format!("{META_FILE}.tmp"));
    fs::write(&tmp, json)?;
    fs::rename(tmp, dir.join(META_FILE))
}

pub fn read_meta(dir: &Path) -> io::Result<TakeMeta> {
    let bytes = fs::read(dir.join(META_FILE))?;
    serde_json::from_slice(&bytes).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

pub fn create_marker(dir: &Path) -> io::Result<()> {
    fs::write(dir.join(MARKER_FILE), b"")
}

pub fn remove_marker(dir: &Path) -> io::Result<()> {
    fs::remove_file(dir.join(MARKER_FILE))
}

fn wav_files(dir: &Path) -> io::Result<Vec<PathBuf>> {
    let mut files: Vec<PathBuf> = fs::read_dir(dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.is_file()
                && p.extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("wav"))
        })
        .collect();
    files.sort();
    Ok(files)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recovery {
    pub dir: PathBuf,
    pub repaired_files: usize,
    pub errors: Vec<String>,
}

/// Repairs every take under `root` that still has the in-progress marker.
pub fn recover_takes(root: &Path) -> io::Result<Vec<Recovery>> {
    let mut out = Vec::new();
    if !root.is_dir() {
        return Ok(out);
    }
    for entry in fs::read_dir(root)? {
        let dir = entry?.path();
        if dir.is_dir() && dir.join(MARKER_FILE).exists() {
            out.push(recover_take(&dir));
        }
    }
    out.sort_by(|a, b| a.dir.cmp(&b.dir));
    Ok(out)
}

fn recover_take(dir: &Path) -> Recovery {
    let mut errors = Vec::new();
    let mut repaired_files = 0;
    let mut frames: Option<u64> = None;
    match wav_files(dir) {
        Ok(files) => {
            for file in files {
                match wav::repair(&file) {
                    Ok(r) => {
                        repaired_files += 1;
                        frames = Some(frames.map_or(r.frames, |f| f.min(r.frames)));
                    }
                    Err(e) => errors.push(format!("{}: {e}", file.display())),
                }
            }
        }
        Err(e) => errors.push(format!("{}: {e}", dir.display())),
    }
    match read_meta(dir) {
        Ok(mut meta) => {
            meta.recovered = true;
            meta.frames = frames.unwrap_or(0);
            if let Err(e) = write_meta(dir, &meta) {
                errors.push(format!("{META_FILE}: {e}"));
            }
        }
        Err(e) => errors.push(format!("{META_FILE}: {e}")),
    }
    if errors.is_empty()
        && let Err(e) = remove_marker(dir)
    {
        errors.push(format!("{MARKER_FILE}: {e}"));
    }
    Recovery {
        dir: dir.to_owned(),
        repaired_files,
        errors,
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TakeEntry {
    pub dir: PathBuf,
    pub name: String,
    pub files: Vec<PathBuf>,
    pub meta: Option<TakeMeta>,
    pub in_progress: bool,
}

/// Take folders under `root` (those holding WAV files), newest first.
pub fn list_takes(root: &Path, limit: usize) -> io::Result<Vec<TakeEntry>> {
    if !root.is_dir() {
        return Ok(Vec::new());
    }
    let mut dirs: Vec<PathBuf> = fs::read_dir(root)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_dir())
        .collect();
    // Names start with a timestamp, so reverse lexical order is newest first.
    dirs.sort_by(|a, b| b.file_name().cmp(&a.file_name()));
    let mut takes = Vec::new();
    for dir in dirs {
        if takes.len() >= limit {
            break;
        }
        let files = wav_files(&dir)?;
        if files.is_empty() {
            continue;
        }
        takes.push(TakeEntry {
            name: dir
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            meta: read_meta(&dir).ok(),
            in_progress: dir.join(MARKER_FILE).exists(),
            files,
            dir,
        });
    }
    Ok(takes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wav::{Bext, WavSpec, WavWriter};
    use chrono::NaiveDate;

    fn t() -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 11, 10)
            .unwrap()
            .and_hms_opt(22, 30, 15)
            .unwrap()
    }

    fn meta() -> TakeMeta {
        TakeMeta {
            app_version: "0.1.0".into(),
            host: "ASIO".into(),
            device: "Audio Interface".into(),
            sample_rate: 48_000,
            format: SampleFormat::Pcm24,
            started_at: "2026-11-10T22:30:15".into(),
            tracks: vec![],
            frames: 0,
            preroll_frames: 0,
            dropped_frames: 0,
            recovered: false,
        }
    }

    #[test]
    fn sanitize_replaces_forbidden_characters() {
        assert_eq!(
            sanitize_component("a/b\\c:d*e?f\"g<h>i|j"),
            "a_b_c_d_e_f_g_h_i_j"
        );
        assert_eq!(sanitize_component("  name. . "), "name");
        assert_eq!(sanitize_component("tab\there"), "tab_here");
        assert_eq!(sanitize_component("ボーカル"), "ボーカル");
    }

    #[test]
    fn sanitize_escapes_reserved_names() {
        assert_eq!(sanitize_component("CON"), "_CON");
        assert_eq!(sanitize_component("com1.wav"), "_com1.wav");
        assert_eq!(sanitize_component("Console"), "Console");
    }

    #[test]
    fn names_follow_the_documented_pattern() {
        assert_eq!(take_dir_name(t(), ""), "2026-11-10_223015");
        assert_eq!(take_dir_name(t(), "サビ案"), "2026-11-10_223015_サビ案");
        assert_eq!(track_file_name(0, "Vocal"), "01_Vocal.wav");
        assert_eq!(track_file_name(11, ""), "12_Track.wav");
    }

    #[test]
    fn take_dirs_get_numbered_suffixes_when_taken() {
        let root = tempfile::tempdir().unwrap();
        let a = create_take_dir(root.path(), t(), "").unwrap();
        let b = create_take_dir(root.path(), t(), "").unwrap();
        assert!(a.ends_with("2026-11-10_223015"));
        assert!(b.ends_with("2026-11-10_223015_2"));
    }

    #[test]
    fn meta_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let m = meta();
        write_meta(dir.path(), &m).unwrap();
        assert_eq!(read_meta(dir.path()).unwrap(), m);
    }

    #[test]
    fn recovery_repairs_marked_takes_and_clears_marker() {
        let root = tempfile::tempdir().unwrap();
        let dir = create_take_dir(root.path(), t(), "").unwrap();
        write_meta(&dir, &meta()).unwrap();
        create_marker(&dir).unwrap();
        let spec = WavSpec {
            channels: 1,
            sample_rate: 48_000,
            format: SampleFormat::Pcm24,
        };
        let path = dir.join("01_In1.wav");
        let mut w =
            WavWriter::create(&path, spec, &Bext::at(t(), 48_000, String::new()), 0).unwrap();
        w.write_samples(&[0.1; 10]).unwrap();
        w.sync().unwrap();
        // Simulate a crash: leak the writer so its Drop never finalizes the header.
        std::mem::forget(w);

        let report = recover_takes(root.path()).unwrap();
        assert_eq!(report.len(), 1);
        assert_eq!(report[0].repaired_files, 1);
        assert!(report[0].errors.is_empty(), "{:?}", report[0].errors);
        assert!(!dir.join(MARKER_FILE).exists());
        let m = read_meta(&dir).unwrap();
        assert!(m.recovered);
        assert_eq!(m.frames, 10);
    }

    #[test]
    fn list_takes_returns_newest_first_and_skips_empty_dirs() {
        let root = tempfile::tempdir().unwrap();
        for name in ["2026-11-09_100000", "2026-11-10_100000", "empty"] {
            fs::create_dir(root.path().join(name)).unwrap();
        }
        fs::write(root.path().join("2026-11-09_100000/01_a.wav"), b"x").unwrap();
        fs::write(root.path().join("2026-11-10_100000/01_a.wav"), b"x").unwrap();
        fs::write(root.path().join("2026-11-10_100000/02_b.WAV"), b"x").unwrap();
        let takes = list_takes(root.path(), 10).unwrap();
        let names: Vec<&str> = takes.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["2026-11-10_100000", "2026-11-09_100000"]);
        assert_eq!(takes[0].files.len(), 2);
        assert_eq!(list_takes(root.path(), 1).unwrap().len(), 1);
    }

    #[test]
    fn missing_root_is_empty_not_an_error() {
        let root = tempfile::tempdir().unwrap();
        let missing = root.path().join("nope");
        assert!(list_takes(&missing, 10).unwrap().is_empty());
        assert!(recover_takes(&missing).unwrap().is_empty());
    }
}
