// Release builds are GUI-only: no console window on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::fs::{self, File};
use std::path::Path;

use eframe::egui;
use simplelog::{CombinedLogger, Config, LevelFilter, SharedLogger, SimpleLogger, WriteLogger};

use recrec::config;
use recrec::ui::RecrecApp;

const LOG_FILE: &str = "recrec.log";
const ICON_SIZE: u32 = 64;

fn init_logging() {
    let mut loggers: Vec<Box<dyn SharedLogger>> = Vec::new();
    if cfg!(debug_assertions) {
        loggers.push(SimpleLogger::new(LevelFilter::Debug, Config::default()));
    }
    let mut file_error = None;
    if let Some(dir) = config::log_dir() {
        match open_log_file(&dir) {
            Ok(file) => loggers.push(WriteLogger::new(LevelFilter::Info, Config::default(), file)),
            Err(e) => file_error = Some(format!("{}: {e}", dir.display())),
        }
    }
    if let Err(e) = CombinedLogger::init(loggers) {
        eprintln!("logger init failed: {e}");
    }
    if let Some(e) = file_error {
        log::warn!("log file unavailable: {e}");
    }
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        log::error!("panic: {info}");
        default_hook(info);
    }));
}

/// Keeps the previous run's log as `recrec.log.old`.
fn open_log_file(dir: &Path) -> std::io::Result<File> {
    fs::create_dir_all(dir)?;
    let path = dir.join(LOG_FILE);
    if path.exists() {
        fs::rename(&path, path.with_extension("log.old"))?;
    }
    File::create(path)
}

/// A red record dot, drawn at startup so no image asset is needed.
fn app_icon() -> egui::IconData {
    let size = ICON_SIZE as f32;
    let center = size / 2.0 - 0.5;
    let radius = size * 0.42;
    let mut rgba = Vec::with_capacity((ICON_SIZE * ICON_SIZE * 4) as usize);
    for y in 0..ICON_SIZE {
        for x in 0..ICON_SIZE {
            let d = ((x as f32 - center).powi(2) + (y as f32 - center).powi(2)).sqrt();
            let alpha = (radius - d + 0.5).clamp(0.0, 1.0);
            rgba.extend_from_slice(&[220, 50, 47, (alpha * 255.0) as u8]);
        }
    }
    egui::IconData {
        rgba,
        width: ICON_SIZE,
        height: ICON_SIZE,
    }
}

fn main() -> eframe::Result {
    init_logging();
    log::info!("recrec {} starting", env!("CARGO_PKG_VERSION"));
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(format!("recrec {}", env!("CARGO_PKG_VERSION")))
            .with_inner_size([960.0, 720.0])
            .with_min_inner_size([640.0, 480.0])
            .with_icon(app_icon()),
        ..Default::default()
    };
    let result = eframe::run_native(
        "recrec",
        options,
        Box::new(|_cc| Ok(Box::new(RecrecApp::launch()))),
    );
    if let Err(e) = &result {
        log::error!("UI terminated with error: {e}");
    }
    log::info!("recrec exiting");
    result
}
