//! OS-specific helpers: sleep prevention, opening folders, dragging files out to other apps.

use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Keeps the system awake while alive (Windows). Must be dropped on the thread that created it.
pub struct SleepGuard {
    _private: (),
}

impl SleepGuard {
    pub fn new() -> Self {
        #[cfg(windows)]
        {
            use windows_sys::Win32::System::Power::{
                ES_CONTINUOUS, ES_SYSTEM_REQUIRED, SetThreadExecutionState,
            };
            // SAFETY: plain Win32 call with flag arguments only.
            if unsafe { SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED) } == 0 {
                log::warn!("SetThreadExecutionState failed; the PC may sleep while recording");
            }
        }
        Self { _private: () }
    }
}

impl Default for SleepGuard {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for SleepGuard {
    fn drop(&mut self) {
        #[cfg(windows)]
        {
            use windows_sys::Win32::System::Power::{ES_CONTINUOUS, SetThreadExecutionState};
            // SAFETY: plain Win32 call with flag arguments only.
            unsafe { SetThreadExecutionState(ES_CONTINUOUS) };
        }
    }
}

/// Opens `path` in Explorer (or the platform file manager).
pub fn open_in_file_manager(path: &Path) -> io::Result<()> {
    let program = if cfg!(windows) {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    // Explorer's exit code is meaningless, so only spawning is checked.
    Command::new(program).arg(path).spawn().map(|_| ())
}

pub fn file_drag_supported() -> bool {
    cfg!(windows)
}

/// Starts an OS drag of `files` from the app window, so they can be dropped into a DAW.
/// Blocks until the drop completes (OLE runs its own modal loop).
#[cfg(windows)]
pub fn start_file_drag(frame: &eframe::Frame, files: Vec<PathBuf>) -> Result<(), String> {
    drag::start_drag(
        frame,
        drag::DragItem::Files(files),
        drag::Image::Raw(Vec::new()),
        |_result, _cursor| {},
        drag::Options::default(),
    )
    .map_err(|e| e.to_string())
}

#[cfg(not(windows))]
pub fn start_file_drag(_frame: &eframe::Frame, _files: Vec<PathBuf>) -> Result<(), String> {
    Err("ファイルのドラッグは Windows 版のみ対応しています".into())
}
