//! Design tokens (colors, type, spacing, radii, motion) and the egui style built from them.
//!
//! Everything visual goes through this file: to retune the look, change values here only.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use eframe::egui::{
    self, Align, Color32, Context, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Id,
    Margin, Stroke, TextFormat, TextStyle, Vec2, style::WidgetVisuals, text::LayoutJob,
};

// ---- color -------------------------------------------------------------------------------
pub const CANVAS: Color32 = Color32::from_rgb(0x01, 0x01, 0x02);
pub const SURFACE_1: Color32 = Color32::from_rgb(0x0f, 0x10, 0x11);
pub const SURFACE_2: Color32 = Color32::from_rgb(0x14, 0x15, 0x16);
pub const SURFACE_3: Color32 = Color32::from_rgb(0x18, 0x19, 0x1a);
pub const SURFACE_4: Color32 = Color32::from_rgb(0x1f, 0x20, 0x22);
pub const HAIRLINE: Color32 = Color32::from_rgb(0x23, 0x25, 0x2a);
pub const HAIRLINE_STRONG: Color32 = Color32::from_rgb(0x34, 0x34, 0x3a);
pub const INK: Color32 = Color32::from_rgb(0xf7, 0xf8, 0xf8);
pub const INK_MUTED: Color32 = Color32::from_rgb(0xd0, 0xd6, 0xe0);
pub const INK_SUBTLE: Color32 = Color32::from_rgb(0x8a, 0x8f, 0x98);
pub const INK_TERTIARY: Color32 = Color32::from_rgb(0x62, 0x66, 0x6d);
/// The single brand accent: focus, selection and emphasis only.
pub const ACCENT: Color32 = Color32::from_rgb(0x5e, 0x6a, 0xd2);
pub const ACCENT_HOVER: Color32 = Color32::from_rgb(0x82, 0x8f, 0xff);
// Functional colors (state, not decoration).
pub const REC: Color32 = Color32::from_rgb(0xeb, 0x57, 0x57);
pub const OK: Color32 = Color32::from_rgb(0x27, 0xa6, 0x44);
pub const WARN: Color32 = Color32::from_rgb(0xe2, 0xb9, 0x3b);
pub const METER_LOW: Color32 = Color32::from_rgb(0x2f, 0xb3, 0x52);
pub const METER_MID: Color32 = WARN;
pub const METER_HIGH: Color32 = REC;

// ---- spacing / radius --------------------------------------------------------------------
pub const SPACE_XXS: f32 = 4.0;
pub const SPACE_XS: f32 = 8.0;
pub const SPACE_SM: f32 = 12.0;
pub const SPACE_LG: f32 = 24.0;
pub const RADIUS_XS: u8 = 4;
pub const RADIUS_SM: u8 = 6;
pub const RADIUS_MD: u8 = 8;
pub const RADIUS_LG: u8 = 12;

// ---- type --------------------------------------------------------------------------------
const FAMILY_MEDIUM: &str = "inter-medium";
const FAMILY_SEMIBOLD: &str = "inter-semibold";
const FAMILY_MONO: &str = "jetbrains-mono";
const FAMILY_ICONS: &str = "phosphor-icons";
pub const TEXT_SMALL: f32 = 11.5;
pub const TEXT_BODY: f32 = 13.5;
pub const TEXT_TITLE: f32 = 15.0;
pub const TEXT_BRAND: f32 = 18.0;
pub const TEXT_TIMER: f32 = 60.0;
pub const ICON_SIZE: f32 = 16.0;

pub fn regular(size: f32) -> FontId {
    FontId::new(size, FontFamily::Proportional)
}

pub fn medium(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(FAMILY_MEDIUM.into()))
}

pub fn semibold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(FAMILY_SEMIBOLD.into()))
}

pub fn mono(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(FAMILY_MONO.into()))
}

/// Font for the glyphs in [`icon`].
pub fn icons(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(FAMILY_ICONS.into()))
}

/// An icon followed by text, as one piece of laid-out text (for buttons and labels).
pub fn with_icon(glyph: &str, text: &str, font: FontId, color: Color32) -> LayoutJob {
    let mut job = LayoutJob::default();
    let format = |font_id: FontId| TextFormat {
        font_id,
        color,
        valign: Align::Center,
        ..Default::default()
    };
    job.append(glyph, 0.0, format(icons(font.size + 2.0)));
    job.append(text, 6.0, format(font));
    job
}

// ---- motion ------------------------------------------------------------------------------
pub const DURATION_FAST: f32 = 0.15;
pub const DURATION_BASE: f32 = 0.25;
pub const DURATION_SLOW: f32 = 0.45;
/// Period of the recording pulse rings.
pub const PULSE_PERIOD: f64 = 1.6;

static REDUCED_MOTION: AtomicBool = AtomicBool::new(false);

pub fn reduced_motion() -> bool {
    REDUCED_MOTION.load(Ordering::Relaxed)
}

/// Eased 0..1 progress toward `on`. Jumps instantly when the OS asks for reduced motion.
pub fn animate(ctx: &Context, id: Id, on: bool, duration: f32) -> f32 {
    let duration = if reduced_motion() { 0.0 } else { duration };
    ctx.animate_bool_with_time_and_easing(id, on, duration, egui::emath::easing::cubic_out)
}

/// Animates a value toward `target` (used for sliding selections).
pub fn animate_value(ctx: &Context, id: Id, target: f32, duration: f32) -> f32 {
    let duration = if reduced_motion() { 0.0 } else { duration };
    ctx.animate_value_with_time(id, target, duration)
}

pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    a.lerp_to_gamma(b, t.clamp(0.0, 1.0))
}

// ---- frames ------------------------------------------------------------------------------
/// A charcoal card with a hairline border (no shadow: depth comes from the surface ladder).
pub fn card(radius: u8) -> egui::Frame {
    egui::Frame::new()
        .fill(SURFACE_1)
        .stroke(Stroke::new(1.0, HAIRLINE))
        .corner_radius(CornerRadius::same(radius))
}

// ---- install -----------------------------------------------------------------------------
/// Loads fonts and applies the dark style. Call once at startup.
pub fn install(ctx: &Context, reduced_motion: bool) {
    REDUCED_MOTION.store(reduced_motion, Ordering::Relaxed);
    ctx.set_fonts(fonts());
    ctx.set_theme(egui::Theme::Dark);
    ctx.all_styles_mut(apply_style);
}

fn fonts() -> FontDefinitions {
    const ICONS: &str = "phosphor";
    let mut defs = FontDefinitions::default();
    let mut add = |name: &str, bytes: &'static [u8]| {
        defs.font_data
            .insert(name.to_owned(), Arc::new(FontData::from_static(bytes)));
    };
    add(
        "inter-regular",
        include_bytes!("../../assets/fonts/Inter-Regular.ttf"),
    );
    add(
        FAMILY_MEDIUM,
        include_bytes!("../../assets/fonts/Inter-Medium.ttf"),
    );
    add(
        FAMILY_SEMIBOLD,
        include_bytes!("../../assets/fonts/Inter-SemiBold.ttf"),
    );
    add(
        FAMILY_MONO,
        include_bytes!("../../assets/fonts/JetBrainsMono-Medium.ttf"),
    );
    add(ICONS, include_bytes!("../../assets/fonts/Phosphor.ttf"));

    // Built-in fonts stay as fallbacks; Japanese glyphs come from the system font provider.
    let builtin = defs
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    let family = |primary: &str| {
        let mut list = vec![primary.to_owned()];
        list.extend(builtin.iter().cloned());
        list
    };
    defs.families
        .insert(FontFamily::Proportional, family("inter-regular"));
    defs.families
        .insert(FontFamily::Monospace, family(FAMILY_MONO));
    // Icons get their own family: Inter and Phosphor both claim private-use codepoints, so
    // neither may be a fallback of the other.
    defs.families.insert(
        FontFamily::Name(FAMILY_ICONS.into()),
        vec![ICONS.to_owned()],
    );
    for name in [FAMILY_MEDIUM, FAMILY_SEMIBOLD, FAMILY_MONO] {
        defs.families
            .insert(FontFamily::Name(name.into()), family(name));
    }
    defs
}

fn apply_style(style: &mut egui::Style) {
    style.text_styles = [
        (TextStyle::Small, regular(TEXT_SMALL)),
        (TextStyle::Body, regular(TEXT_BODY)),
        (TextStyle::Button, medium(TEXT_BODY)),
        (TextStyle::Heading, semibold(TEXT_TITLE)),
        (TextStyle::Monospace, mono(TEXT_BODY)),
    ]
    .into();

    let s = &mut style.spacing;
    s.item_spacing = Vec2::new(SPACE_XS, SPACE_XS);
    s.button_padding = Vec2::new(SPACE_SM, 6.0);
    s.interact_size = Vec2::new(40.0, 30.0);
    s.combo_height = 320.0;
    s.menu_margin = Margin::same(6);
    s.window_margin = Margin::same(12);
    s.scroll.bar_width = 6.0;
    s.scroll.floating = true;

    let v = &mut style.visuals;
    v.dark_mode = true;
    v.panel_fill = CANVAS;
    v.window_fill = SURFACE_2;
    v.window_stroke = Stroke::new(1.0, HAIRLINE_STRONG);
    v.window_corner_radius = CornerRadius::same(RADIUS_MD);
    v.menu_corner_radius = CornerRadius::same(RADIUS_MD);
    v.extreme_bg_color = SURFACE_2;
    v.text_edit_bg_color = Some(SURFACE_2);
    v.faint_bg_color = SURFACE_1;
    v.hyperlink_color = ACCENT_HOVER;
    v.warn_fg_color = WARN;
    v.error_fg_color = REC;
    v.selection.bg_fill = mix(SURFACE_4, ACCENT, 0.55);
    v.selection.stroke = Stroke::new(1.0, ACCENT_HOVER);
    v.slider_trailing_fill = true;
    v.popup_shadow = egui::Shadow {
        offset: [0, 8],
        blur: 24,
        spread: 0,
        color: Color32::from_black_alpha(140),
    };

    let radius = CornerRadius::same(RADIUS_MD);
    let widget = |bg: Color32, border: Color32, fg: Color32| WidgetVisuals {
        bg_fill: bg,
        weak_bg_fill: bg,
        bg_stroke: Stroke::new(1.0, border),
        corner_radius: radius,
        fg_stroke: Stroke::new(1.0, fg),
        expansion: 0.0,
    };
    v.widgets.noninteractive = widget(SURFACE_1, HAIRLINE, INK_MUTED);
    v.widgets.inactive = widget(SURFACE_3, HAIRLINE, INK_MUTED);
    v.widgets.hovered = widget(SURFACE_4, HAIRLINE_STRONG, INK);
    v.widgets.active = widget(SURFACE_4, ACCENT, INK);
    v.widgets.open = widget(SURFACE_4, HAIRLINE_STRONG, INK);
}

/// Phosphor icon glyphs (regular weight) used by the UI.
pub mod icon {
    pub const ARROWS_CLOCKWISE: &str = "\u{E094}";
    pub const ARROW_COUNTER_CLOCKWISE: &str = "\u{E038}";
    pub const BELL: &str = "\u{E0CE}";
    pub const CARET_DOUBLE_LEFT: &str = "\u{E128}";
    pub const CARET_DOUBLE_RIGHT: &str = "\u{E12A}";
    pub const CHECK_CIRCLE: &str = "\u{E184}";
    pub const DOTS_SIX_VERTICAL: &str = "\u{EAE2}";
    pub const FILE_AUDIO: &str = "\u{EA20}";
    pub const FLAG: &str = "\u{E244}";
    pub const FOLDER: &str = "\u{E24A}";
    pub const FOLDER_OPEN: &str = "\u{E256}";
    pub const HARD_DRIVES: &str = "\u{E2A0}";
    pub const HEADPHONES: &str = "\u{E2A6}";
    pub const KEYBOARD: &str = "\u{E2D8}";
    pub const METRONOME: &str = "\u{EC8E}";
    pub const PAUSE: &str = "\u{E39E}";
    pub const PENCIL_SIMPLE: &str = "\u{E3B4}";
    pub const PLAY: &str = "\u{E3D0}";
    pub const PLUS: &str = "\u{E3D4}";
    pub const SKIP_BACK: &str = "\u{E5A4}";
    pub const SLIDERS_HORIZONTAL: &str = "\u{E434}";
    pub const STOP: &str = "\u{E46C}";
    pub const TRASH: &str = "\u{E4A6}";
    pub const WARNING: &str = "\u{E4E0}";
    pub const WARNING_CIRCLE: &str = "\u{E4E2}";
    pub const WAVEFORM: &str = "\u{E802}";
    pub const X: &str = "\u{E4F6}";
}
