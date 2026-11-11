//! OS-specific helpers: sleep prevention, opening folders, dragging files out to other apps.

use std::ffi::OsStr;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

const UNINSTALLER_FILE: &str = "unins000.exe";
#[cfg(windows)]
const INSTALLER_SILENT_FLAG: &str = "/SILENT";

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

/// Hands `target` (a folder or a URL) to the desktop shell.
fn shell_open(target: impl AsRef<OsStr>) -> io::Result<()> {
    let program = if cfg!(windows) {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    // Explorer's exit code is meaningless, so only spawning is checked.
    Command::new(program).arg(target).spawn().map(|_| ())
}

/// Opens `path` in Explorer (or the platform file manager).
pub fn open_in_file_manager(path: &Path) -> io::Result<()> {
    shell_open(path)
}

/// Opens `url` in the default browser. Pass only URLs the app built or validated itself.
pub fn open_url(url: &str) -> io::Result<()> {
    shell_open(url)
}

/// Whether this copy was set up by the installer (which can then update it in place), as
/// opposed to the portable zip. Inno Setup leaves its uninstaller next to the executable.
pub fn is_installed_build() -> bool {
    if !cfg!(windows) {
        return false;
    }
    match std::env::current_exe() {
        Ok(exe) => exe
            .parent()
            .is_some_and(|dir| dir.join(UNINSTALLER_FILE).is_file()),
        Err(e) => {
            log::warn!("cannot locate the running executable: {e}");
            false
        }
    }
}

/// Starts a downloaded recrec installer without its wizard pages. It closes the running
/// app itself, so the caller must not be recording.
#[cfg(windows)]
pub fn launch_installer(path: &Path) -> io::Result<()> {
    Command::new(path)
        .arg(INSTALLER_SILENT_FLAG)
        .spawn()
        .map(|_| ())
}

#[cfg(not(windows))]
pub fn launch_installer(_path: &Path) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "インストーラーは Windows 版のみ対応しています",
    ))
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

/// Whether the OS asks apps to minimize animation (Windows: "Animation effects" off).
pub fn prefers_reduced_motion() -> bool {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            SPI_GETCLIENTAREAANIMATION, SystemParametersInfoW,
        };
        let mut enabled: i32 = 1;
        // SAFETY: SPI_GETCLIENTAREAANIMATION writes one BOOL through the pointer we pass.
        let ok = unsafe {
            SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION, 0, (&raw mut enabled).cast(), 0)
        };
        ok != 0 && enabled == 0
    }
    #[cfg(not(windows))]
    {
        false
    }
}

/// Moves a file or folder to the Recycle Bin (recoverable, unlike deleting).
pub fn move_to_trash(path: &Path) -> Result<(), String> {
    trash::delete(path).map_err(|e| e.to_string())
}
