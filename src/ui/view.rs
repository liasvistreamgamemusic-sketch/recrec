//! Drawing of the main window. State changes go through the actions in `app.rs`.
//!
//! Layout: settings sidebar (left) · transport + tracks (center) · takes (right).

use std::path::PathBuf;
use std::time::Duration;

use eframe::egui::{
    self, Align, Align2, Button, Color32, ComboBox, CornerRadius, Frame, Id, Label, Layout, Margin,
    Order, RichText, ScrollArea, Sense, Slider, Stroke, TextEdit, Ui, Vec2,
};

use super::app::{Level, Message, RecrecApp, format_bytes, format_duration};
use super::theme::{self, icon};
use super::widgets::{
    RECORD_BUTTON_SIZE, arm_toggle, badge, clip_badge, db_readout, field_label, file_chip,
    icon_button, meter, meter_scale, record_button, section_label, segmented, usage_bar,
};
use crate::meter::{METER_FLOOR_DB, MeterDisplay};
use crate::platform;
use crate::recorder::MAX_PREROLL_SECS;
use crate::take::TakeEntry;
use crate::track::Source;
use crate::wav::SampleFormat;

const SIDEBAR_WIDTH: f32 = 288.0;
const TAKES_WIDTH: f32 = 340.0;
const TRACK_NAME_WIDTH: f32 = 116.0;
const TRACK_SOURCE_WIDTH: f32 = 140.0;
const TRACK_METER_HEIGHT: f32 = 12.0;
const DB_READOUT_WIDTH: f32 = 46.0;
const MAX_TOASTS: usize = 3;
const TOAST_WIDTH: f32 = 380.0;

impl RecrecApp {
    pub(super) fn draw(&mut self, ui: &mut Ui) {
        let now = ui.input(|i| i.time);
        self.poll(now);

        let ctx = ui.ctx().clone();
        if !ctx.egui_wants_keyboard_input()
            && !ctx.any_popup_open()
            && ui.input(|i| i.key_pressed(egui::Key::Space))
        {
            self.toggle_recording();
        }

        let panel = |margin: Margin| Frame::new().fill(theme::CANVAS).inner_margin(margin);
        egui::Panel::bottom("status_bar")
            .frame(panel(Margin::symmetric(20, 6)))
            .show(ui, |ui| self.status_bar(ui));
        egui::Panel::left("sidebar")
            .exact_size(SIDEBAR_WIDTH)
            .resizable(false)
            .frame(panel(Margin::same(20)))
            .show(ui, |ui| self.sidebar(ui));
        egui::Panel::right("takes")
            .exact_size(TAKES_WIDTH)
            .resizable(false)
            .frame(panel(Margin::same(20)))
            .show(ui, |ui| self.take_panel(ui));
        egui::CentralPanel::no_frame()
            .frame(panel(Margin::same(20)))
            .show(ui, |ui| {
                self.transport(ui);
                ui.add_space(theme::SPACE_LG);
                self.track_list(ui);
            });
        self.toasts(&ctx);
    }

    // ---- sidebar -------------------------------------------------------------------------

    fn sidebar(&mut self, ui: &mut Ui) {
        let busy = self.is_busy();
        ui.horizontal(|ui| {
            let (dot, _) = ui.allocate_exact_size(Vec2::splat(18.0), Sense::hover());
            ui.painter().circle_filled(dot.center(), 6.0, theme::REC);
            ui.label(
                RichText::new("recrec")
                    .font(theme::semibold(theme::TEXT_BRAND))
                    .color(theme::INK),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(
                    RichText::new(concat!("v", env!("CARGO_PKG_VERSION")))
                        .font(theme::mono(theme::TEXT_SMALL))
                        .color(theme::INK_TERTIARY),
                );
            });
        });
        ui.add_space(theme::SPACE_LG);

        section_label(ui, "オーディオ");
        ui.add_space(theme::SPACE_XXS);
        ui.add_enabled_ui(!busy, |ui| self.device_fields(ui));
        ui.add_space(theme::SPACE_XS);
        self.connection_status(ui);

        ui.add_space(theme::SPACE_LG);
        section_label(ui, "録音");
        ui.add_space(theme::SPACE_XXS);
        ui.add_enabled_ui(!busy, |ui| self.recording_fields(ui));

        ui.with_layout(Layout::bottom_up(Align::Min), |ui| self.disk_usage(ui));
    }

    fn device_fields(&mut self, ui: &mut Ui) {
        let width = ui.available_width();

        field_label(ui, "ホスト");
        let mut host = self.host;
        ComboBox::from_id_salt("host")
            .selected_text(host.name())
            .width(width)
            .show_ui(ui, |ui| {
                for h in self.hosts.clone() {
                    ui.selectable_value(&mut host, h, h.name());
                }
            });
        if host != self.host {
            self.select_host(host);
        }

        field_label(ui, "デバイス");
        let mut device = self.device.clone();
        ComboBox::from_id_salt("device")
            .selected_text(device.as_deref().unwrap_or("(なし)"))
            .width(width)
            .show_ui(ui, |ui| {
                for d in &self.devices {
                    ui.selectable_value(
                        &mut device,
                        Some(d.name.clone()),
                        format!("{} ({} 入力)", d.name, d.channels),
                    );
                }
            });
        if let Some(name) = device
            && self.device.as_ref() != Some(&name)
        {
            self.select_device(name);
        }

        let Some(info) = self.current_device_info().cloned() else {
            return;
        };
        let half = (width - ui.spacing().item_spacing.x) / 2.0;
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.set_width(half);
                field_label(ui, "サンプルレート");
                let current = self.session.as_ref().map_or_else(
                    || info.pick_rate(self.config.sample_rate),
                    |s| s.info.sample_rate,
                );
                let mut rate = current;
                ComboBox::from_id_salt("rate")
                    .selected_text(format_rate(rate))
                    .width(half)
                    .show_ui(ui, |ui| {
                        for r in &info.rates {
                            ui.selectable_value(&mut rate, *r, format_rate(*r));
                        }
                    });
                if rate != current {
                    self.select_rate(rate);
                }
            });
            ui.vertical(|ui| {
                ui.set_width(half);
                field_label(ui, "バッファ");
                let choices = info.buffer_choices();
                let current = self.session.as_ref().and_then(|s| s.info.buffer_size);
                let mut buffer = current;
                ui.add_enabled_ui(!choices.is_empty(), |ui| {
                    ComboBox::from_id_salt("buffer")
                        .selected_text(buffer.map_or("既定".to_owned(), |b| b.to_string()))
                        .width(half)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut buffer, None, "ドライバ既定");
                            for b in choices {
                                ui.selectable_value(&mut buffer, Some(b), format!("{b} サンプル"));
                            }
                        });
                });
                if buffer != current {
                    self.select_buffer(buffer);
                }
            });
        });
    }

    fn connection_status(&mut self, ui: &mut Ui) {
        let (color, text) = match (&self.session, &self.session_error) {
            (_, Some(err)) => (theme::REC, err.clone()),
            (Some(s), None) => (
                theme::OK,
                format!(
                    "接続中 · {} 入力 · {}",
                    s.info.channels, s.info.device_format
                ),
            ),
            (None, None) => (theme::INK_TERTIARY, "デバイスが開かれていません".to_owned()),
        };
        let busy = self.is_busy();
        ui.horizontal(|ui| {
            let (dot, _) = ui.allocate_exact_size(Vec2::splat(12.0), Sense::hover());
            ui.painter().circle_filled(dot.center(), 4.0, color);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add_enabled_ui(!busy, |ui| {
                    if icon_button(
                        ui,
                        icon::ARROWS_CLOCKWISE,
                        "デバイス一覧を読み直して接続し直す",
                        false,
                    )
                    .clicked()
                    {
                        self.reconnect();
                    }
                });
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    let text_color = if self.session_error.is_some() {
                        theme::REC
                    } else {
                        theme::INK_SUBTLE
                    };
                    ui.add(
                        Label::new(
                            RichText::new(&text)
                                .font(theme::regular(theme::TEXT_SMALL))
                                .color(text_color),
                        )
                        .truncate(),
                    )
                    .on_hover_text(&text);
                });
            });
        });
    }

    fn recording_fields(&mut self, ui: &mut Ui) {
        field_label(ui, "形式");
        let labels: Vec<&str> = SampleFormat::ALL.iter().map(|f| f.short_label()).collect();
        let selected = SampleFormat::ALL
            .iter()
            .position(|f| *f == self.config.format)
            .unwrap_or(0);
        if let Some(i) = segmented(ui, Id::unique("format"), &labels, selected) {
            self.config.format = SampleFormat::ALL[i];
            self.mark_dirty();
        }

        ui.add_space(theme::SPACE_XXS);
        ui.horizontal(|ui| {
            field_label(ui, "プリロール");
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(
                    RichText::new(format!("{} 秒", self.config.preroll_secs))
                        .font(theme::mono(theme::TEXT_SMALL))
                        .color(theme::INK_MUTED),
                );
            });
        });
        let mut secs = self.config.preroll_secs;
        ui.spacing_mut().slider_width = ui.available_width();
        let slider = ui
            .add(Slider::new(&mut secs, 0..=MAX_PREROLL_SECS).show_value(false))
            .on_hover_text("録音ボタンを押す前の N 秒もファイルに含めます");
        if slider.changed() {
            self.set_preroll(secs);
        }

        ui.add_space(theme::SPACE_XXS);
        field_label(ui, "出力先");
        let dir = self.config.output_dir();
        let shown = dir.display().to_string();
        Frame::new()
            .fill(theme::SURFACE_2)
            .stroke(Stroke::new(1.0, theme::HAIRLINE))
            .corner_radius(CornerRadius::same(theme::RADIUS_MD))
            .inner_margin(Margin::symmetric(10, 4))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if icon_button(ui, icon::FOLDER_OPEN, "エクスプローラーで開く", false)
                            .clicked()
                        {
                            match std::fs::create_dir_all(&dir) {
                                Ok(()) => self.open_folder(&dir),
                                Err(e) => {
                                    self.push(Level::Error, format!("出力先を作れません: {e}"));
                                }
                            }
                        }
                        if icon_button(ui, icon::FOLDER, "出力先を変更…", false).clicked()
                            && let Some(picked) =
                                rfd::FileDialog::new().set_directory(&dir).pick_folder()
                        {
                            self.set_output_dir(picked);
                        }
                        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                            ui.add(
                                Label::new(
                                    RichText::new(&shown)
                                        .font(theme::mono(theme::TEXT_SMALL))
                                        .color(theme::INK_MUTED),
                                )
                                .truncate(),
                            )
                            .on_hover_text(&shown);
                        });
                    });
                });
            });
    }

    fn disk_usage(&mut self, ui: &mut Ui) {
        let Some(free) = self.free_space else {
            return;
        };
        // bottom_up layout: add from the bottom line upward.
        if let Some(total) = self.total_space.filter(|t| *t > 0) {
            usage_bar(ui, 1.0 - free as f32 / total as f32);
        }
        let bps = self.armed_bytes_per_second();
        let remaining = if bps > 0 {
            let hours = free as f64 / bps as f64 / 3600.0;
            if hours >= 1.0 {
                format!("あと約 {hours:.0} 時間")
            } else {
                format!("あと約 {:.0} 分", hours * 60.0)
            }
        } else {
            String::new()
        };
        ui.horizontal(|ui| {
            ui.label(theme::with_icon(
                icon::HARD_DRIVES,
                &format!("空き {}", format_bytes(free)),
                theme::regular(theme::TEXT_SMALL),
                theme::INK_SUBTLE,
            ));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(
                    RichText::new(remaining)
                        .font(theme::regular(theme::TEXT_SMALL))
                        .color(theme::INK_TERTIARY),
                );
            });
        });
    }

    // ---- transport -----------------------------------------------------------------------

    fn transport(&mut self, ui: &mut Ui) {
        let busy = self.is_busy();
        let recording = self.recording_dir.is_some();
        let ctx = ui.ctx().clone();
        let rec_t = theme::animate(
            &ctx,
            Id::unique("rec_state"),
            recording,
            theme::DURATION_SLOW,
        );

        theme::card(theme::RADIUS_LG)
            .stroke(Stroke::new(
                1.0,
                theme::mix(theme::HAIRLINE, theme::REC, 0.55 * rec_t),
            ))
            .inner_margin(Margin::same(24))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.set_min_height(RECORD_BUTTON_SIZE + 16.0);
                    ui.add_space(theme::SPACE_XS);
                    if record_button(ui, busy, self.can_record() || busy).clicked() {
                        self.toggle_recording();
                    }
                    ui.add_space(theme::SPACE_LG);
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 2.0;
                        self.transport_state(ui, recording, rec_t);
                        ui.label(
                            RichText::new(format_duration(self.elapsed_secs()))
                                .font(theme::mono(theme::TEXT_TIMER))
                                .color(theme::mix(theme::INK_MUTED, theme::INK, rec_t)),
                        );
                        ui.label(
                            RichText::new(self.take_summary())
                                .font(theme::regular(theme::TEXT_SMALL))
                                .color(theme::INK_SUBTLE),
                        );
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.vertical(|ui| {
                            ui.set_width(240.0);
                            field_label(ui, "テイク名 (任意)");
                            ui.add_enabled(
                                !busy,
                                TextEdit::singleline(&mut self.take_label)
                                    .hint_text("例: サビ案")
                                    .margin(Margin::symmetric(10, 7))
                                    .desired_width(f32::INFINITY),
                            )
                            .on_hover_text("フォルダ名の末尾に付きます");
                        });
                    });
                });
            });
    }

    fn transport_state(&self, ui: &mut Ui, recording: bool, rec_t: f32) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let (dot, _) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
            let blink = if recording && !theme::reduced_motion() {
                let time = ui.input(|i| i.time);
                0.55 + 0.45 * ((time * std::f64::consts::TAU / theme::PULSE_PERIOD).cos() as f32)
            } else {
                1.0
            };
            ui.painter().circle_filled(
                dot.center(),
                4.0,
                theme::mix(theme::INK_TERTIARY, theme::REC, rec_t).gamma_multiply(blink),
            );
            let text = if recording {
                "録音中"
            } else if self.session_error.is_some() || self.session.is_none() {
                "デバイス未接続"
            } else if !self.tracks.iter().any(|t| t.armed) {
                "録音するトラックがありません"
            } else {
                "待機中"
            };
            ui.label(
                RichText::new(text)
                    .font(theme::medium(theme::TEXT_SMALL + 1.0))
                    .color(theme::mix(theme::INK_SUBTLE, theme::REC, rec_t)),
            );
        });
    }

    fn take_summary(&self) -> String {
        let armed = self.tracks.iter().filter(|t| t.armed).count();
        let mut parts = vec![
            format!("{armed} トラック"),
            self.config.format.label().to_owned(),
        ];
        if let Some(s) = &self.session {
            parts.push(format_rate(s.info.sample_rate));
        }
        if self.config.preroll_secs > 0 {
            parts.push(format!("プリロール {} 秒", self.config.preroll_secs));
        }
        parts.join("  ·  ")
    }

    // ---- tracks --------------------------------------------------------------------------

    fn track_list(&mut self, ui: &mut Ui) {
        let busy = self.is_busy();
        let channels = self.session.as_ref().map_or(0, |s| s.info.channels);
        let bank = self.session.as_ref().map(|s| s.meters.clone());
        let mut changed = false;
        let mut remove = None;

        ui.horizontal(|ui| {
            ui.label(
                RichText::new("トラック")
                    .font(theme::semibold(theme::TEXT_TITLE))
                    .color(theme::INK),
            );
            let armed = self.tracks.iter().filter(|t| t.armed).count();
            ui.label(
                RichText::new(format!("{armed} / {} を録音", self.tracks.len()))
                    .font(theme::regular(theme::TEXT_SMALL))
                    .color(theme::INK_SUBTLE),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add_enabled_ui(!busy && self.session.is_some(), |ui| {
                    if ui
                        .button(theme::with_icon(
                            icon::PLUS,
                            "追加",
                            theme::medium(theme::TEXT_BODY),
                            theme::INK_MUTED,
                        ))
                        .on_hover_text("トラックを 1 つ追加します")
                        .clicked()
                    {
                        self.add_track();
                    }
                    if ui
                        .add(
                            Button::new(theme::with_icon(
                                icon::ARROW_COUNTER_CLOCKWISE,
                                "リセット",
                                theme::medium(theme::TEXT_BODY),
                                theme::INK_SUBTLE,
                            ))
                            .frame(false),
                        )
                        .on_hover_text("入力 1 ch ごとにモノラル 1 トラックの構成に戻します")
                        .clicked()
                    {
                        self.reset_tracks();
                    }
                });
            });
        });

        if let Some((left, right)) = self.meter_span {
            meter_scale(ui, left, right);
        }
        let mut span = None;

        ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 6.0;
                if self.tracks.is_empty() {
                    empty_state(
                        ui,
                        icon::WAVEFORM,
                        "トラックがありません",
                        "「追加」または「リセット」でトラックを作ります。",
                    );
                }
                for (i, t) in self.tracks.iter_mut().enumerate() {
                    let row = Id::unique(("track", i));
                    let appear =
                        theme::animate(ui.ctx(), row.with("in"), true, theme::DURATION_BASE);
                    let armed_t =
                        theme::animate(ui.ctx(), row.with("armed"), t.armed, theme::DURATION_BASE);
                    ui.scope(|ui| {
                        ui.set_opacity(appear);
                        theme::card(theme::RADIUS_MD)
                            .fill(theme::mix(theme::CANVAS, theme::SURFACE_1, armed_t))
                            .inner_margin(Margin::symmetric(12, 8))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    if arm_toggle(ui, t.armed, !busy).clicked() {
                                        t.armed = !t.armed;
                                        changed = true;
                                    }
                                    ui.label(
                                        RichText::new(format!("{:02}", i + 1))
                                            .font(theme::mono(theme::TEXT_SMALL))
                                            .color(theme::INK_TERTIARY),
                                    );
                                    let name = ui.add_enabled(
                                        !busy,
                                        TextEdit::singleline(&mut t.name)
                                            .font(theme::medium(theme::TEXT_BODY))
                                            .margin(Margin::symmetric(8, 5))
                                            .desired_width(TRACK_NAME_WIDTH),
                                    );
                                    changed |= name.on_hover_text("ファイル名になります").changed();
                                    ui.add_enabled_ui(!busy, |ui| {
                                        let before = t.source;
                                        ComboBox::from_id_salt(("source", i))
                                            .selected_text(t.source.label())
                                            .width(TRACK_SOURCE_WIDTH)
                                            .show_ui(ui, |ui| {
                                                for s in Source::options(channels) {
                                                    ui.selectable_value(
                                                        &mut t.source,
                                                        s,
                                                        s.label(),
                                                    );
                                                }
                                            });
                                        changed |= before != t.source;
                                    });

                                    let sources = t.source.channels();
                                    let displays: Vec<&MeterDisplay> = sources
                                        .iter()
                                        .filter_map(|c| self.meters.get(*c))
                                        .collect();
                                    let gap = ui.spacing().item_spacing.x;
                                    let trailing = DB_READOUT_WIDTH + 40.0 + 28.0 + gap * 3.0;
                                    let width = (ui.available_width() - trailing).max(80.0);
                                    ui.scope(|ui| {
                                        ui.set_opacity(theme::lerp(0.35, 1.0, armed_t));
                                        let rect = meter(
                                            ui,
                                            &displays,
                                            Vec2::new(width, TRACK_METER_HEIGHT),
                                        );
                                        span.get_or_insert((rect.left(), rect.right()));
                                        let hold = displays
                                            .iter()
                                            .map(|d| d.hold_db)
                                            .fold(METER_FLOOR_DB, f32::max);
                                        db_readout(ui, hold, DB_READOUT_WIDTH);
                                    });
                                    let clipped = bank.as_ref().is_some_and(|b| {
                                        sources.iter().any(|c| *c < b.channels() && b.clipped(*c))
                                    });
                                    if clip_badge(ui, row.with("clip"), clipped).clicked()
                                        && let Some(b) = &bank
                                    {
                                        for c in sources.iter().filter(|c| **c < b.channels()) {
                                            b.reset_clip(*c);
                                        }
                                    }
                                    ui.add_enabled_ui(!busy, |ui| {
                                        if icon_button(ui, icon::TRASH, "トラックを削除", true)
                                            .clicked()
                                        {
                                            remove = Some(i);
                                        }
                                    });
                                });
                            });
                    });
                }
            });

        self.meter_span = span;
        if let Some(i) = remove {
            self.tracks.remove(i);
            changed = true;
        }
        if changed {
            self.tracks_changed();
        }
    }

    // ---- takes ---------------------------------------------------------------------------

    fn take_panel(&mut self, ui: &mut Ui) {
        let mut drag: Option<Vec<PathBuf>> = None;
        let mut open: Option<PathBuf> = None;
        let mut refresh = false;

        ui.horizontal(|ui| {
            ui.label(
                RichText::new("テイク")
                    .font(theme::semibold(theme::TEXT_TITLE))
                    .color(theme::INK),
            );
            ui.label(
                RichText::new(format!("{} 件", self.takes.len()))
                    .font(theme::regular(theme::TEXT_SMALL))
                    .color(theme::INK_SUBTLE),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if icon_button(ui, icon::ARROWS_CLOCKWISE, "一覧を更新", false).clicked() {
                    refresh = true;
                }
            });
        });
        ui.label(
            RichText::new(if platform::file_drag_supported() {
                "ファイルをつかんで DAW へドロップできます"
            } else {
                "ファイルのドラッグは Windows 版のみ対応しています"
            })
            .font(theme::regular(theme::TEXT_SMALL))
            .color(theme::INK_TERTIARY),
        );
        ui.add_space(theme::SPACE_XXS);

        ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = theme::SPACE_XS;
                if self.takes.is_empty() {
                    empty_state(
                        ui,
                        icon::WAVEFORM,
                        "まだテイクがありません",
                        "● または Space キーで録音を始めます。",
                    );
                }
                for take in &self.takes {
                    take_card(ui, take, &mut drag, &mut open);
                }
            });

        if let Some(files) = drag {
            self.take_drag(files);
        }
        if let Some(dir) = open {
            self.open_folder(&dir);
        }
        if refresh {
            self.refresh_takes();
        }
    }

    // ---- status bar / toasts -------------------------------------------------------------

    fn status_bar(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label(theme::with_icon(
                icon::KEYBOARD,
                "Space で録音 / 停止",
                theme::regular(theme::TEXT_SMALL),
                theme::INK_TERTIARY,
            ));
            if let Some(s) = &self.session {
                let dropped = if self.recording_dir.is_some() {
                    s.dropped_frames().saturating_sub(self.dropped_at_take_start)
                } else {
                    s.dropped_frames()
                };
                if dropped > 0 {
                    ui.label(theme::with_icon(
                        icon::WARNING,
                        &format!("取りこぼし {dropped} フレーム"),
                        theme::medium(theme::TEXT_SMALL),
                        theme::REC,
                    ))
                    .on_hover_text(
                        "書き込みが追いつかず捨てたフレーム数です。バッファを大きくするか、他の負荷を減らしてください",
                    );
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.menu_button(
                    theme::with_icon(
                        icon::BELL,
                        "履歴",
                        theme::regular(theme::TEXT_SMALL),
                        theme::INK_SUBTLE,
                    ),
                    |ui| {
                        ui.set_min_width(420.0);
                        ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                            if self.messages.is_empty() {
                                ui.weak("メッセージはありません");
                            }
                            for m in self.messages.iter().rev() {
                                ui.horizontal_wrapped(|ui| {
                                    ui.label(
                                        RichText::new(m.at.format("%H:%M:%S").to_string())
                                            .font(theme::mono(theme::TEXT_SMALL))
                                            .color(theme::INK_TERTIARY),
                                    );
                                    ui.label(
                                        RichText::new(&m.text)
                                            .font(theme::regular(theme::TEXT_SMALL + 1.0))
                                            .color(level_color(m.level)),
                                    );
                                });
                            }
                        });
                    },
                );
            });
        });
    }

    /// Recent messages slide up from the bottom-right corner and fade out on their own.
    fn toasts(&self, ctx: &egui::Context) {
        let live: Vec<(&Message, f32)> = self
            .messages
            .iter()
            .rev()
            .filter_map(|m| toast_opacity(m).map(|o| (m, o)))
            .take(MAX_TOASTS)
            .collect();
        if live.is_empty() {
            return;
        }
        egui::Area::new(Id::unique("toasts"))
            .anchor(Align2::RIGHT_BOTTOM, Vec2::new(-20.0, -44.0))
            .order(Order::Foreground)
            .interactable(false)
            .show(ctx, |ui| {
                ui.vertical(|ui| {
                    // Oldest first, so the newest toast sits nearest the corner.
                    for (m, opacity) in live.into_iter().rev() {
                        ui.scope(|ui| {
                            ui.set_opacity(opacity);
                            ui.add_space((1.0 - opacity) * 10.0);
                            Frame::new()
                                .fill(theme::SURFACE_3)
                                .stroke(Stroke::new(1.0, theme::HAIRLINE_STRONG))
                                .corner_radius(CornerRadius::same(theme::RADIUS_MD))
                                .inner_margin(Margin::symmetric(14, 10))
                                .show(ui, |ui| {
                                    ui.set_width(TOAST_WIDTH);
                                    ui.horizontal_top(|ui| {
                                        ui.label(
                                            RichText::new(level_icon(m.level))
                                                .font(theme::icons(theme::ICON_SIZE))
                                                .color(level_color(m.level)),
                                        );
                                        ui.add(
                                            Label::new(
                                                RichText::new(&m.text)
                                                    .font(theme::regular(theme::TEXT_SMALL + 1.0))
                                                    .color(theme::INK),
                                            )
                                            .wrap(),
                                        );
                                    });
                                });
                        });
                    }
                });
            });
    }
}

fn take_card(
    ui: &mut Ui,
    take: &TakeEntry,
    drag: &mut Option<Vec<PathBuf>>,
    open: &mut Option<PathBuf>,
) {
    let id = Id::unique(("take", &take.dir));
    let can_drag = platform::file_drag_supported() && !take.in_progress;
    let appear = theme::animate(ui.ctx(), id.with("in"), true, theme::DURATION_SLOW);
    let was_hovered: bool = ui.data(|d| d.get_temp(id).unwrap_or(false));
    let hover = theme::animate(
        ui.ctx(),
        id.with("hover"),
        was_hovered,
        theme::DURATION_FAST,
    );

    let card = ui.scope(|ui| {
        ui.set_opacity(appear);
        theme::card(theme::RADIUS_MD)
            .stroke(Stroke::new(
                1.0,
                theme::mix(theme::HAIRLINE, theme::HAIRLINE_STRONG, hover),
            ))
            .inner_margin(Margin::same(12))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    let handle = ui
                        .add(
                            Label::new(
                                RichText::new(icon::DOTS_SIX_VERTICAL)
                                    .font(theme::icons(theme::ICON_SIZE))
                                    .color(theme::mix(
                                        theme::INK_TERTIARY,
                                        theme::INK_MUTED,
                                        hover,
                                    )),
                            )
                            .selectable(false)
                            .sense(Sense::click_and_drag()),
                        )
                        .on_hover_text("つかんで全ファイルを DAW へドロップ");
                    if can_drag {
                        if handle.drag_started() {
                            *drag = Some(take.files.clone());
                        }
                        handle.on_hover_cursor(egui::CursorIcon::Grab);
                    }
                    ui.add(
                        Label::new(
                            RichText::new(&take.name)
                                .font(theme::medium(theme::TEXT_BODY))
                                .color(theme::INK),
                        )
                        .truncate(),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if icon_button(ui, icon::FOLDER_OPEN, "フォルダを開く", false).clicked()
                        {
                            *open = Some(take.dir.clone());
                        }
                    });
                });

                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    if let Some(meta) = &take.meta {
                        ui.label(
                            RichText::new(format!(
                                "{}  ·  {} ファイル  ·  {}  ·  {}",
                                format_duration(meta.duration_secs()),
                                take.files.len(),
                                meta.format.short_label(),
                                format_rate(meta.sample_rate),
                            ))
                            .font(theme::regular(theme::TEXT_SMALL))
                            .color(theme::INK_SUBTLE),
                        );
                        if meta.recovered {
                            badge(ui, "修復済み", theme::WARN);
                        }
                        if meta.dropped_frames > 0 {
                            badge(
                                ui,
                                &format!("取りこぼし {}", meta.dropped_frames),
                                theme::REC,
                            );
                        }
                    }
                    if take.in_progress {
                        badge(ui, "録音中", theme::REC);
                    }
                });

                ui.add_space(2.0);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = Vec2::splat(6.0);
                    for f in &take.files {
                        let name = f
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        let tip = if can_drag {
                            "つかんで DAW へドロップ"
                        } else {
                            "録音が終わるとドラッグできます"
                        };
                        if file_chip(ui, &name, can_drag, tip).drag_started() && can_drag {
                            *drag = Some(vec![f.clone()]);
                        }
                    }
                });
            })
    });
    let hovered = card.response.contains_pointer();
    ui.data_mut(|d| d.insert_temp(id, hovered));
}

fn empty_state(ui: &mut Ui, glyph: &str, title: &str, hint: &str) {
    ui.add_space(theme::SPACE_LG * 2.0);
    ui.vertical_centered(|ui| {
        ui.label(
            RichText::new(glyph)
                .font(theme::icons(36.0))
                .color(theme::INK_TERTIARY),
        );
        ui.label(
            RichText::new(title)
                .font(theme::medium(theme::TEXT_BODY))
                .color(theme::INK_MUTED),
        );
        ui.label(
            RichText::new(hint)
                .font(theme::regular(theme::TEXT_SMALL))
                .color(theme::INK_SUBTLE),
        );
    });
}

/// Opacity of a toast for `m` right now, or `None` once it has expired.
fn toast_opacity(m: &Message) -> Option<f32> {
    let ttl = match m.level {
        Level::Info => Duration::from_secs(4),
        Level::Warn => Duration::from_secs(7),
        Level::Error => Duration::from_secs(10),
    };
    let age = m.born.elapsed();
    if age >= ttl {
        return None;
    }
    if theme::reduced_motion() {
        return Some(1.0);
    }
    let fade_in = (age.as_secs_f32() / theme::DURATION_BASE).min(1.0);
    let fade_out = ((ttl - age).as_secs_f32() / theme::DURATION_SLOW).min(1.0);
    Some(egui::emath::easing::cubic_out(fade_in.min(fade_out)))
}

fn level_color(level: Level) -> Color32 {
    match level {
        Level::Info => theme::INK_MUTED,
        Level::Warn => theme::WARN,
        Level::Error => theme::REC,
    }
}

fn level_icon(level: Level) -> &'static str {
    match level {
        Level::Info => icon::CHECK_CIRCLE,
        Level::Warn => icon::WARNING,
        Level::Error => icon::WARNING_CIRCLE,
    }
}

fn format_rate(rate: u32) -> String {
    let khz = f64::from(rate) / 1000.0;
    if khz.fract() == 0.0 {
        format!("{khz:.0} kHz")
    } else {
        format!("{khz:.1} kHz")
    }
}
