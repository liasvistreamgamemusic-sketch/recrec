//! The egui front end.

mod app;
mod theme;
mod view;
mod widgets;

pub use app::RecrecApp;

/// Applies fonts and the visual style. Call once before the first frame.
pub fn install_theme(ctx: &eframe::egui::Context) {
    theme::install(ctx, crate::platform::prefers_reduced_motion());
}
