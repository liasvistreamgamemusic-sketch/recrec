//! Drawing of the main window. State changes go through the actions in `app.rs`.
//!
//! Layout: settings sidebar (left) · transport + tracks (center) · takes (right).

use std::path::PathBuf;
use std::time::Duration;

use eframe::egui::{
    self, Align, Align2, Button, Color32, ComboBox, CornerRadius, DragValue, Frame, Id, Label,
    Layout, Margin, Order, RichText, ScrollArea, Sense, Slider, Stroke, TextEdit, Ui, Vec2,
};

use super::app::{
    Level, LiveWindow, Message, RecrecApp, UpdateState, format_bytes, format_duration,
};
use super::theme::{self, icon};
use super::widgets::{
    BeatGrid, RECORD_BUTTON_SIZE, WaveOverlay, arm_toggle, badge, beat_lights, clip_badge,
    db_readout, field_label, file_chip, icon_button, overview, record_button, section_label,
    segmented, switch, take_lane, time_ruler, toggle_chip, transport_button, usage_bar, vmeter,
    waveform,
};
use crate::meter::{METER_FLOOR_DB, MeterDisplay};
use crate::platform;
use crate::recorder::MAX_PREROLL_SECS;
use crate::take::TakeEntry;
use crate::track::{self, MAX_VOLUME_DB, MIN_VOLUME_DB, Source, Track};
use crate::transport::{MAX_BPM, MIN_BPM};
use crate::wav::SampleFormat;
use crate::waveform;

const SIDEBAR_WIDTH: f32 = 288.0;
const TAKES_WIDTH: f32 = 340.0;
const TAKES_RAIL_WIDTH: f32 = 52.0;
/// Width of the controls at the left of a track row.
const TRACK_HEADER_WIDTH: f32 = 232.0;
const TRACK_SOURCE_WIDTH: f32 = 112.0;
const TRACK_LANE_HEIGHT: f32 = 54.0;
const DB_READOUT_WIDTH: f32 = 46.0;
const EDIT_DIALOG_WIDTH: f32 = 380.0;
/// Width of the clock and summary beside the record button.
const CLOCK_COLUMN_WIDTH: f32 = 300.0;
const TAKE_NAME_WIDTH: f32 = 240.0;
const SCROLL_BAR_GUTTER: f32 = 10.0;
const MAX_TOASTS: usize = 3;
const TOAST_WIDTH: f32 = 380.0;

impl RecrecApp {
    pub(super) fn draw(&mut self, ui: &mut Ui) {
        let now = ui.input(|i| i.time);
        self.poll(now);

        let ctx = ui.ctx().clone();
        if !ctx.egui_wants_keyboard_input() && !ctx.any_popup_open() && self.take_edit.is_none() {
            if ui.input(|i| i.key_pressed(egui::Key::Space)) {
                self.toggle_recording();
            }
            if ui.input(|i| i.key_pressed(egui::Key::M)) {
                self.add_marker();
            }
            if ui.input(|i| i.key_pressed(egui::Key::P)) {
                self.toggle_pause();
            }
            if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                self.toggle_play();
            }
            if ui.input(|i| i.key_pressed(egui::Key::Home)) {
                self.seek(0);
            }
        }

        let panel = |margin: Margin| Frame::new().fill(theme::CANVAS).inner_margin(margin);
        if self.update_banner_visible() {
            egui::Panel::top("update_banner")
                .frame(
                    Frame::new()
                        .fill(theme::SURFACE_2)
                        .inner_margin(Margin::symmetric(20, 8)),
                )
                .show(ui, |ui| self.update_banner(ui));
        }
        egui::Panel::bottom("status_bar")
            .frame(panel(Margin::symmetric(20, 6)))
            .show(ui, |ui| self.status_bar(ui));
        egui::Panel::left("sidebar")
            .exact_size(SIDEBAR_WIDTH)
            .resizable(false)
            .frame(panel(Margin::same(20)))
            .show(ui, |ui| self.sidebar(ui));
        // The take list folds into a narrow rail so the track lanes can use the width.
        let collapsed = self.config.takes_collapsed;
        let takes_width = theme::animate_value(
            &ctx,
            Id::unique("takes_width"),
            if collapsed {
                TAKES_RAIL_WIDTH
            } else {
                TAKES_WIDTH
            },
            theme::DURATION_BASE,
        );
        egui::Panel::right("takes")
            .exact_size(takes_width)
            .resizable(false)
            .frame(panel(if collapsed {
                Margin::symmetric(12, 20)
            } else {
                Margin::same(20)
            }))
            .show(ui, |ui| {
                if collapsed {
                    self.take_rail(ui);
                } else {
                    self.take_panel(ui);
                }
            });
        egui::CentralPanel::no_frame()
            .frame(panel(Margin::same(20)))
            .show(ui, |ui| {
                self.transport(ui);
                ui.add_space(theme::SPACE_LG);
                self.track_list(ui);
            });
        self.take_edit_dialog(&ctx);
        self.toasts(&ctx);
    }

    // ---- update banner -------------------------------------------------------------------

    /// Stays out of the way while recording, except to say that a finished download waits.
    fn update_banner_visible(&self) -> bool {
        match self.update {
            UpdateState::Idle => false,
            UpdateState::Available(_) | UpdateState::Downloading { .. } => !self.is_busy(),
            UpdateState::Ready { .. } => true,
        }
    }

    fn update_banner(&mut self, ui: &mut Ui) {
        let busy = self.is_busy();
        let text = match &self.update {
            UpdateState::Idle => return,
            UpdateState::Available(release) => format!(
                "recrec {} が利用できます (現在 {})",
                release.version,
                env!("CARGO_PKG_VERSION")
            ),
            UpdateState::Downloading {
                release,
                received,
                total,
            } => {
                let progress = match total {
                    Some(total) => {
                        format!("{} / {}", format_bytes(*received), format_bytes(*total))
                    }
                    None => format_bytes(*received),
                };
                format!(
                    "recrec {} をダウンロードしています… {progress}",
                    release.version
                )
            }
            UpdateState::Ready { release, .. } if busy => format!(
                "recrec {} の準備ができました。録音を止めると更新できます",
                release.version
            ),
            UpdateState::Ready { release, .. } => {
                format!("recrec {} の準備ができました", release.version)
            }
        };
        let available = matches!(self.update, UpdateState::Available(_));
        let ready = matches!(self.update, UpdateState::Ready { .. });
        let self_update = self.can_self_update();
        ui.horizontal(|ui| {
            ui.label(theme::with_icon(
                icon::BELL,
                &text,
                theme::medium(theme::TEXT_BODY),
                theme::INK,
            ));
            // Right-to-left: the first button added sits at the far right.
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if available {
                    if banner_button(ui, "閉じる", false)
                        .on_hover_text("次に起動したときにまたお知らせします")
                        .clicked()
                    {
                        self.dismiss_update();
                    }
                    if banner_button(ui, "このバージョンをスキップ", false).clicked() {
                        self.skip_update();
                    }
                }
                if banner_button(ui, "変更内容", false)
                    .on_hover_text("リリースページをブラウザーで開きます")
                    .clicked()
                {
                    self.open_release_page();
                }
                if available {
                    let (label, hint) = if self_update {
                        ("更新", "ダウンロードして recrec を更新し、開き直します")
                    } else {
                        (
                            "ダウンロードページを開く",
                            "新しいバージョンをブラウザーからダウンロードします",
                        )
                    };
                    if banner_button(ui, label, true).on_hover_text(hint).clicked() {
                        self.start_update();
                    }
                }
                if ready && !busy && banner_button(ui, "更新", true).clicked() {
                    self.install_update();
                }
            });
        });
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

        // The settings scroll on short windows; the disk gauge stays pinned at the bottom.
        let footer = 44.0;
        ScrollArea::vertical()
            .max_height((ui.available_height() - footer).max(0.0))
            .auto_shrink([false, true])
            .show(ui, |ui| self.sidebar_sections(ui, busy));
        ui.with_layout(Layout::bottom_up(Align::Min), |ui| self.disk_usage(ui));
    }

    fn sidebar_sections(&mut self, ui: &mut Ui, busy: bool) {
        // Leave room for the scroll bar so it never covers the right-aligned values.
        ui.set_width(ui.available_width() - SCROLL_BAR_GUTTER);
        section_label(ui, "オーディオ");
        ui.add_space(theme::SPACE_XXS);
        ui.add_enabled_ui(!busy, |ui| self.device_fields(ui));
        ui.add_space(theme::SPACE_XS);
        self.connection_status(ui);

        ui.add_space(theme::SPACE_LG);
        section_label(ui, "録音");
        ui.add_space(theme::SPACE_XXS);
        ui.add_enabled_ui(!busy, |ui| self.recording_fields(ui));

        ui.add_space(theme::SPACE_LG);
        section_label(ui, "出力 (モニターとクリック)");
        ui.add_space(theme::SPACE_XXS);
        self.output_fields(ui);

        ui.add_space(theme::SPACE_LG);
        section_label(ui, "アップデート");
        ui.add_space(theme::SPACE_XXS);
        let mut check_updates = !self.config.skip_update_check;
        if ui
            .checkbox(&mut check_updates, "起動時に確認する")
            .on_hover_text("起動するたびに GitHub へ最新バージョンを問い合わせます")
            .changed()
        {
            self.set_update_check(check_updates);
        }
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
            (Some(s), None) => (theme::OK, {
                let loopback = s
                    .info
                    .channel_names
                    .iter()
                    .filter(|n| track::is_loopback_name(n))
                    .count();
                let physical = usize::from(s.info.channels) - loopback;
                if loopback > 0 {
                    format!("接続中 · 入力 {physical} + ループバック {loopback}")
                } else {
                    format!("接続中 · {physical} 入力 · {}", s.info.device_format)
                }
            }),
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
        self.recording_format_fields(ui);
        ui.add_space(theme::SPACE_XXS);
        let mut skip = self.config.skip_silent_tracks;
        if ui
            .checkbox(&mut skip, "無音のトラックは保存しない")
            .on_hover_text(
                "テイクの最初から最後まで -60 dB を超えなかったトラック (何もつないでいない入力など) の\n                 ファイルを残しません。全トラックが無音のときはそのまま残します",
            )
            .changed()
        {
            self.config.skip_silent_tracks = skip;
            self.mark_dirty();
        }
    }

    fn recording_format_fields(&mut self, ui: &mut Ui) {
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

    fn output_fields(&mut self, ui: &mut Ui) {
        if let Some(err) = self.session.as_ref().and_then(|s| s.output_error.clone()) {
            ui.add(
                Label::new(
                    RichText::new(format!("音を出せません: {err}"))
                        .font(theme::regular(theme::TEXT_SMALL))
                        .color(theme::WARN),
                )
                .wrap(),
            );
            return;
        }
        let mut volume = self.config.output_volume;
        let mut comp = self.config.latency_comp_ms;
        ui.horizontal(|ui| {
            field_label(ui, "音量");
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(
                    RichText::new(format!("{:.0} %", volume * 100.0))
                        .font(theme::mono(theme::TEXT_SMALL))
                        .color(theme::INK_MUTED),
                );
            });
        });
        ui.spacing_mut().slider_width = ui.available_width();
        ui.add(Slider::new(&mut volume, 0.0..=1.0).show_value(false))
            .on_hover_text(
                "アプリから出る音 (モニターとクリック) 全体の音量。録音には影響しません",
            );
        ui.horizontal(|ui| {
            field_label(ui, "録音開始の遅延補正");
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add(
                    DragValue::new(&mut comp)
                        .range(-50.0..=200.0)
                        .speed(0.5)
                        .fixed_decimals(1)
                        .suffix(" ms"),
                )
                .on_hover_text(
                    "メトロノーム使用時、録音の頭がクリックより早い/遅いときに調整します。\n                     DAW に並べて 1 拍目がグリッドより前にあるなら、その分だけ増やします",
                );
            });
        });
        if volume != self.config.output_volume || comp != self.config.latency_comp_ms {
            self.config.output_volume = volume;
            self.config.latency_comp_ms = comp;
            self.mark_dirty();
        }
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
                // Always span the column: the card must not resize with its contents.
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.set_min_height(RECORD_BUTTON_SIZE + 16.0);
                    ui.add_space(theme::SPACE_XS);
                    if record_button(ui, busy, self.can_record() || busy).clicked() {
                        self.toggle_recording();
                    }
                    // Always present (disabled when idle) so the clock never shifts.
                    let paused = self.is_paused();
                    ui.add_enabled_ui(recording, |ui| {
                        let (glyph, tip) = if paused {
                            (icon::PLAY, "録音を再開 (P) — 同じテイクの続きに録ります")
                        } else {
                            (icon::PAUSE, "一時停止 (P) — テイクを閉じずに止めます")
                        };
                        if transport_button(ui, glyph, tip, paused).clicked() {
                            self.toggle_pause();
                        }
                    });
                    ui.add_space(theme::SPACE_SM);
                    ui.vertical(|ui| {
                        // Fixed width, so the summary line (which grows when the metronome
                        // is on) cannot push the take-name field around.
                        ui.set_width(CLOCK_COLUMN_WIDTH);
                        ui.spacing_mut().item_spacing.y = 2.0;
                        self.transport_state(ui, recording, rec_t);
                        // While counting in, the clock shows bars to go and the beat.
                        let clock = match (self.count_in_beat(), self.punch_countdown()) {
                            (Some(c), _) => format!("-{} · {}", c.bars_left, c.beat),
                            (None, Some(Some(secs))) => format!("-{secs:.1}"),
                            (None, Some(None)) => "…".to_owned(),
                            (None, None) => {
                                // On the metronome, the clock can count bars and beats.
                                match self.live_grid().filter(|_| self.config.ruler_bars) {
                                    Some(grid) => grid.label(self.elapsed_secs()),
                                    None => format_duration(self.elapsed_secs()),
                                }
                            }
                        };
                        ui.label(
                            RichText::new(clock)
                                .font(theme::mono(theme::TEXT_TIMER))
                                .color(theme::mix(theme::INK_MUTED, theme::INK, rec_t)),
                        );
                        ui.add(
                            Label::new(
                                RichText::new(self.take_summary())
                                    .font(theme::regular(theme::TEXT_SMALL))
                                    .color(theme::INK_SUBTLE),
                            )
                            .truncate(),
                        );
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        // Shrinks on narrow windows instead of pushing the card wider.
                        let width = ui.available_width().min(TAKE_NAME_WIDTH);
                        ui.vertical(|ui| {
                            ui.set_width(width);
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
                ui.add_space(theme::SPACE_SM);
                self.metronome_strip(ui);
            });
    }

    fn metronome_strip(&mut self, ui: &mut Ui) {
        let busy = self.is_busy();
        let has_output = self.has_output();
        let beat = self.beat_phase();
        let mut m = self.config.metronome.clone();
        let mut tapped = false;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            ui.add_enabled_ui(has_output, |ui| {
                ui.label(
                    RichText::new(icon::METRONOME)
                        .font(theme::icons(theme::ICON_SIZE + 2.0))
                        .color(if m.enabled {
                            theme::INK
                        } else {
                            theme::INK_SUBTLE
                        }),
                );
                ui.add_enabled_ui(!busy, |ui| {
                    switch(ui, &mut m.enabled).on_hover_text(
                        "オンにすると、クリックに合わせて小節の頭から録音が始まります",
                    );
                });
                ui.label(RichText::new("メトロノーム").color(theme::INK_MUTED));
                ui.add_enabled_ui(m.enabled, |ui| {
                    ui.add_enabled_ui(!busy, |ui| {
                        ui.add(
                            DragValue::new(&mut m.bpm)
                                .range(MIN_BPM..=MAX_BPM)
                                .speed(0.2)
                                .fixed_decimals(1)
                                .suffix(" BPM"),
                        )
                        .on_hover_text("ドラッグ、またはダブルクリックして数値を入力");
                        tapped = ui
                            .button("タップ")
                            .on_hover_text("曲のテンポに合わせて数回クリックします")
                            .clicked();
                        ComboBox::from_id_salt("beats")
                            .selected_text(m.beats_per_bar.to_string())
                            .width(44.0)
                            .show_ui(ui, |ui| {
                                for n in 1..=12 {
                                    ui.selectable_value(&mut m.beats_per_bar, n, n.to_string());
                                }
                            });
                        ui.label(RichText::new("/").color(theme::INK_TERTIARY));
                        ComboBox::from_id_salt("beat_unit")
                            .selected_text(m.beat_unit.to_string())
                            .width(44.0)
                            .show_ui(ui, |ui| {
                                for n in [4, 8] {
                                    ui.selectable_value(&mut m.beat_unit, n, n.to_string());
                                }
                            });
                        ComboBox::from_id_salt("count_in")
                            .selected_text(count_in_label(m.count_in_bars))
                            .width(128.0)
                            .show_ui(ui, |ui| {
                                for bars in [0, 1, 2, 4] {
                                    ui.selectable_value(
                                        &mut m.count_in_bars,
                                        bars,
                                        count_in_label(bars),
                                    );
                                }
                            });
                    });
                    ui.checkbox(&mut m.click_while_recording, "録音中も鳴らす")
                        .on_hover_text("オフにすると、カウントインのあとクリックが止まります");
                    ui.checkbox(&mut m.click_while_idle, "待機中も鳴らす")
                        .on_hover_text("録音していないときもクリックを鳴らします (練習用)");
                    ui.spacing_mut().slider_width = 72.0;
                    ui.add(Slider::new(&mut m.volume, 0.0..=1.0).show_value(false))
                        .on_hover_text("クリックの音量");
                    beat_lights(ui, m.beats_per_bar, beat);
                });
            });
            if !has_output && self.session.is_some() {
                ui.label(
                    RichText::new("音を出せないため使えません")
                        .font(theme::regular(theme::TEXT_SMALL))
                        .color(theme::WARN),
                );
            }
        });
        if m != self.config.metronome {
            self.config.metronome = m;
            self.mark_dirty();
        }
        if tapped {
            self.tap_tempo();
        }
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
            let text = if recording && self.is_paused() {
                "一時停止中 (P で再開)"
            } else if recording {
                "録音中"
            } else if let Some(countdown) = self.punch_countdown() {
                if countdown.is_some() {
                    "録り直し: まもなく録音 (Space で中止)"
                } else {
                    "録り直しの準備中…"
                }
            } else if self.count_in_beat().is_some() {
                "カウントイン (Space で中止)"
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
        let m = &self.config.metronome;
        if m.enabled && self.has_output() {
            // A take on the metronome starts on a bar line, so the pre-roll does not apply.
            parts.push(format!(
                "{:.1} BPM  {}/{}",
                m.bpm, m.beats_per_bar, m.beat_unit
            ));
        } else if self.config.preroll_secs > 0 {
            parts.push(format!("プリロール {} 秒", self.config.preroll_secs));
        }
        parts.join("  ·  ")
    }

    // ---- tracks --------------------------------------------------------------------------

    fn track_list(&mut self, ui: &mut Ui) {
        let busy = self.is_busy();
        let has_output = self.has_output();
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
                        .on_hover_text(
                            "入力 1 つにつきモノラル 1 トラックの構成に戻します \
                             (ループバック入力は録音オフの状態で並びます)",
                        )
                        .clicked()
                    {
                        self.reset_tracks();
                    }
                });
            });
        });

        // An opened take replaces the live lanes with its timeline (not while recording).
        let lane_width = self.lane_span.map_or(400.0, |(l, r)| (r - l).max(1.0));
        let position = self.play_position();
        let playing = self.is_playing();
        let take_mode = !busy && self.open_take.is_some();
        if busy {
            self.lane_span = None;
        }
        if take_mode {
            self.take_toolbar(ui, lane_width);
        } else {
            self.live_toolbar(ui);
        }
        let view = self.open_take.as_mut().filter(|_| take_mode).map(|t| {
            let frames_per_px = t.zoom(lane_width);
            if playing {
                t.follow(position, lane_width);
            }
            t.clamp_view(lane_width);
            (t.view_start, frames_per_px)
        });
        let take_grid = self.take_grid();
        let bars = self.config.ruler_bars;
        let mut take_grid_px: Vec<(f32, bool)> = Vec::new();
        if let (Some((view_start, frames_per_px)), Some((left, right)), Some(take)) =
            (view, self.lane_span, &self.open_take)
        {
            let rate = f64::from(take.meta.sample_rate.max(1));
            let secs_per_px = frames_per_px / rate;
            let seek_px = time_ruler(
                ui,
                (left, right),
                view_start / rate,
                secs_per_px,
                take.duration_secs(),
                Some(((position as f64 - view_start) / frames_per_px) as f32),
                take_grid.filter(|_| bars),
            );
            let window = overview(
                ui,
                (left, right),
                &take.overview(lane_width.floor().max(1.0) as usize),
                take.window(lane_width),
                Some(position as f64 / take.frames.max(1) as f64),
            );
            if let Some(grid) = take_grid {
                let from = view_start / rate;
                take_grid_px = grid
                    .lines(
                        from,
                        from + f64::from(lane_width) * secs_per_px,
                        6.0 * secs_per_px,
                    )
                    .into_iter()
                    .map(|(t, bar)| (((t - from) / secs_per_px) as f32, bar))
                    .collect();
            }
            if let Some(px) = seek_px {
                let frame = self.open_take.as_ref().map(|t| t.frame_at(px, lane_width));
                if let Some(frame) = frame {
                    self.seek(frame);
                }
            }
            if let (Some((start, end)), Some(t)) = (window, &mut self.open_take) {
                t.set_window(start, end, lane_width);
            }
        }
        let view = self.open_take.as_ref().zip(view);
        let mut seek: Option<u64> = None;
        let mut zoom: Option<(f64, f32)> = None;
        let mut scroll_px = 0.0f32;
        let mut span: Option<(f32, f32)> = None;

        // Live lanes: the newest `window_blocks` blocks of the inputs, newest at the right.
        let window_blocks = self.live_window_blocks();
        let live_grid = self.live_grid();
        let blocks_per_sec = f64::from(waveform::BLOCKS_PER_SECOND);
        if !take_mode
            && let (Some(start), Some((left, right)), Some(s)) =
                (self.rec_wave_start, self.lane_span, &self.session)
        {
            // While recording, the ruler counts from the start of the take.
            let total = s.waves.total_blocks();
            time_ruler(
                ui,
                (left, right),
                (total as f64 - window_blocks as f64 - start as f64) / blocks_per_sec,
                window_blocks as f64 / blocks_per_sec / f64::from((right - left).max(1.0)),
                total.saturating_sub(start) as f64 / blocks_per_sec,
                None,
                live_grid.filter(|_| bars),
            );
        }

        let fallback_names = track::fallback_names(channels);
        let names = self
            .session
            .as_ref()
            .map_or(&fallback_names, |s| &s.info.channel_names);
        let waves = self.session.as_ref().map(|s| &s.waves);
        let rec_start = self.rec_wave_start;
        let markers = &self.marker_blocks;
        let overlay = |columns: usize| -> WaveOverlay {
            let Some(waves) = waves else {
                return WaveOverlay::default();
            };
            let total = waves.total_blocks();
            // Column of a block inside the visible window (None when it scrolled out).
            let col_of = |block: u64| -> Option<usize> {
                let from_left = (block + window_blocks as u64).checked_sub(total)?;
                Some((from_left as usize * columns / window_blocks).min(columns - 1))
            };
            let mut o = WaveOverlay {
                recording_from: rec_start.map(|s| col_of(s).unwrap_or(0)),
                markers: markers.iter().filter_map(|b| col_of(*b)).collect(),
                ..Default::default()
            };
            if let (Some(start), Some(grid)) = (rec_start, live_grid) {
                // Grid times are measured from the start of the take (bar 1).
                let first_visible = total.saturating_sub(window_blocks as u64);
                let from = (first_visible as f64 - start as f64) / blocks_per_sec;
                let to = (total as f64 - start as f64) / blocks_per_sec;
                let secs_per_column = window_blocks as f64 / blocks_per_sec / columns as f64;
                for (t, bar) in grid.lines(from, to, 6.0 * secs_per_column) {
                    let block = start + (t * blocks_per_sec).round() as u64;
                    if let Some(col) = col_of(block) {
                        if bar {
                            o.bars.push(col);
                        } else {
                            o.beats.push(col);
                        }
                    }
                }
            }
            o
        };

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
                    let sources = t.source.channels();
                    ui.scope(|ui| {
                        ui.set_opacity(appear);
                        theme::card(theme::RADIUS_MD)
                            .fill(theme::mix(theme::CANVAS, theme::SURFACE_1, armed_t))
                            .inner_margin(Margin::symmetric(10, 8))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.vertical(|ui| {
                                        ui.set_width(TRACK_HEADER_WIDTH);
                                        ui.spacing_mut().item_spacing.y = 4.0;
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
                                                    .margin(Margin::symmetric(8, 4))
                                                    .desired_width(f32::INFINITY),
                                            );
                                            changed |= name
                                                .on_hover_text("ファイル名になります")
                                                .changed();
                                        });
                                        ui.horizontal(|ui| {
                                            ui.spacing_mut().item_spacing.x = 4.0;
                                            ui.add_enabled_ui(!busy, |ui| {
                                                let before = t.source;
                                                ComboBox::from_id_salt(("source", i))
                                                    .selected_text(t.source.label_with(names))
                                                    .width(TRACK_SOURCE_WIDTH)
                                                    .show_ui(ui, |ui| {
                                                        for s in Source::options(channels) {
                                                            ui.selectable_value(
                                                                &mut t.source,
                                                                s,
                                                                s.label_with(names),
                                                            );
                                                        }
                                                    });
                                                changed |= before != t.source;
                                            });
                                            ui.add_enabled_ui(has_output, |ui| {
                                                changed |= track_monitor_controls(ui, t);
                                            });
                                        });
                                    });

                                    let displays: Vec<&MeterDisplay> = sources
                                        .iter()
                                        .filter_map(|c| self.meters.get(*c))
                                        .collect();
                                    let gap = ui.spacing().item_spacing.x;
                                    let meter_width = if sources.len() > 1 { 14.0 } else { 8.0 };
                                    let trailing =
                                        meter_width + DB_READOUT_WIDTH + 28.0 + gap * 3.0;
                                    let width = (ui.available_width() - trailing).max(60.0);
                                    ui.scope(|ui| {
                                        ui.set_opacity(theme::lerp(0.35, 1.0, armed_t));
                                        let size = Vec2::new(width, TRACK_LANE_HEIGHT);
                                        let lane = match (view, waves) {
                                            (Some((take, (view_start, frames_per_px))), _) => {
                                                let block = f64::from(take.block);
                                                let px_of = |frame: u64| {
                                                    ((frame as f64 - view_start) / frames_per_px)
                                                        as f32
                                                };
                                                let lanes: Vec<_> = take
                                                    .track_for(&sources)
                                                    .map(|t| take.lanes_of(t))
                                                    .unwrap_or(0..0)
                                                    .map(|l| {
                                                        take.lanes[l].columns(
                                                            view_start / block,
                                                            frames_per_px * f64::from(width)
                                                                / block,
                                                            width.floor().max(1.0) as usize,
                                                        )
                                                    })
                                                    .collect();
                                                let markers: Vec<f32> = take
                                                    .meta
                                                    .markers
                                                    .iter()
                                                    .map(|m| px_of(*m))
                                                    .collect();
                                                let response = take_lane(
                                                    ui,
                                                    size,
                                                    &lanes,
                                                    Some(px_of(position)),
                                                    &markers,
                                                    &take_grid_px,
                                                    lanes.is_empty().then_some(
                                                        "このテイクには録音されていません",
                                                    ),
                                                );
                                                let left = response.rect.left();
                                                if (response.clicked() || response.dragged())
                                                    && let Some(pos) =
                                                        response.interact_pointer_pos()
                                                {
                                                    let px = (pos.x - left).clamp(0.0, width);
                                                    let frame =
                                                        view_start + f64::from(px) * frames_per_px;
                                                    seek = Some(frame.max(0.0) as u64);
                                                }
                                                if let Some(pos) = response.hover_pos() {
                                                    let (factor, dx) = ui.input(|i| {
                                                        (i.zoom_delta(), i.smooth_scroll_delta.x)
                                                    });
                                                    if factor != 1.0 {
                                                        zoom =
                                                            Some((f64::from(factor), pos.x - left));
                                                    }
                                                    scroll_px -= dx;
                                                }
                                                response.rect
                                            }
                                            (None, Some(waves)) => waveform(
                                                ui,
                                                size,
                                                waves,
                                                &sources,
                                                window_blocks,
                                                &overlay,
                                            ),
                                            (None, None) => ui.allocate_space(size).1,
                                        };
                                        span.get_or_insert((lane.left(), lane.right()));
                                        vmeter(
                                            ui,
                                            &displays,
                                            Vec2::new(meter_width, TRACK_LANE_HEIGHT),
                                        );
                                    });
                                    ui.vertical(|ui| {
                                        ui.set_width(DB_READOUT_WIDTH);
                                        ui.spacing_mut().item_spacing.y = 2.0;
                                        let hold = displays
                                            .iter()
                                            .map(|d| d.hold_db)
                                            .fold(METER_FLOOR_DB, f32::max);
                                        db_readout(ui, hold, DB_READOUT_WIDTH);
                                        let clipped = bank.as_ref().is_some_and(|b| {
                                            sources
                                                .iter()
                                                .any(|c| *c < b.channels() && b.clipped(*c))
                                        });
                                        if clip_badge(ui, row.with("clip"), clipped).clicked()
                                            && let Some(b) = &bank
                                        {
                                            for c in sources.iter().filter(|c| **c < b.channels()) {
                                                b.reset_clip(*c);
                                            }
                                        }
                                    });
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

        self.lane_span = span;
        if let Some(t) = &mut self.open_take {
            if let Some((factor, anchor)) = zoom {
                t.zoom_by(factor, anchor, lane_width);
            }
            if scroll_px != 0.0 {
                t.scroll_by(scroll_px, lane_width);
            }
        }
        if let Some(frame) = seek {
            self.seek(frame);
        }
        if let Some(i) = remove {
            self.tracks.remove(i);
            changed = true;
        }
        if changed {
            self.tracks_changed();
        }
    }

    /// Playback controls of the opened take, shown above its timeline.
    fn take_toolbar(&mut self, ui: &mut Ui, lane_width: f32) {
        let Some(take) = &self.open_take else { return };
        let playing = self.is_playing();
        let blocker = self.playback_blocker();
        let rate = f64::from(take.meta.sample_rate.max(1));
        let now = self.play_position() as f64 / rate;
        let clock = match self.take_grid().filter(|_| self.config.ruler_bars) {
            Some(grid) => format!("{} / {}", grid.label(now), grid.label(take.duration_secs())),
            None => format!(
                "{} / {}",
                format_duration(now),
                format_duration(take.duration_secs())
            ),
        };
        let name = take.name.clone();
        let mut toggle = false;
        let mut rewind = false;
        let mut close = false;
        let mut append = false;
        let mut punch = false;
        let at_start = take.playhead == 0;
        let mut zoom: Option<f64> = None;
        let mut show_all = false;

        theme::card(theme::RADIUS_MD)
            .inner_margin(Margin::symmetric(10, 6))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.add_enabled_ui(blocker.is_none(), |ui| {
                        let (glyph, tip) = if playing {
                            (icon::STOP, "停止 (Enter)")
                        } else {
                            (icon::PLAY, "再生 (Enter)")
                        };
                        toggle = transport_button(ui, glyph, tip, playing).clicked();
                    });
                    rewind =
                        transport_button(ui, icon::SKIP_BACK, "先頭へ (Home)", false).clicked();
                    ui.label(
                        RichText::new(clock)
                            .font(theme::mono(theme::TEXT_BODY))
                            .color(theme::INK),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        close =
                            icon_button(ui, icon::X, "テイクを閉じて、入力の波形表示に戻る", false)
                                .clicked();
                        append = ui
                            .add_enabled(self.can_record(), Button::new("続きから録音"))
                            .on_hover_text(
                                "このテイクの最後に続けて録音します (つなぎ目にマーカーが入ります)",
                            )
                            .clicked();
                        punch = ui
                            .add_enabled(
                                self.can_record() && blocker.is_none() && !at_start && !playing,
                                Button::new("ここから録り直す"),
                            )
                            .on_hover_text(
                                "再生位置の 4 秒前から再生し、再生位置から録音し直します。\n                                 元のテイクは残り、新しいテイクが作られます",
                            )
                            .on_disabled_hover_text(
                                "録り直したい位置を波形でクリックして選びます (再生中は使えません)",
                            )
                            .clicked();
                        self.unit_toggle(ui);
                        ui.spacing_mut().item_spacing.x = 8.0;
                        if ui
                            .button("全体")
                            .on_hover_text("テイク全体が収まるように表示")
                            .clicked()
                        {
                            show_all = true;
                        }
                        if ui
                            .button("＋")
                            .on_hover_text("拡大 (Ctrl + ホイール)")
                            .clicked()
                        {
                            zoom = Some(2.0);
                        }
                        if ui
                            .button("−")
                            .on_hover_text("縮小 (Ctrl + ホイール)")
                            .clicked()
                        {
                            zoom = Some(0.5);
                        }
                        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                            let text = blocker.clone().unwrap_or(name);
                            let color = if blocker.is_some() {
                                theme::WARN
                            } else {
                                theme::INK_SUBTLE
                            };
                            ui.add(
                                Label::new(
                                    RichText::new(text)
                                        .font(theme::regular(theme::TEXT_SMALL))
                                        .color(color),
                                )
                                .truncate(),
                            );
                        });
                    });
                });
            });

        if let Some(t) = &mut self.open_take {
            if let Some(factor) = zoom {
                t.zoom_by(factor, lane_width / 2.0, lane_width);
            }
            if show_all {
                t.show_all();
            }
        }
        if toggle {
            self.toggle_play();
        }
        if rewind {
            self.seek(0);
        }
        if append {
            self.append_to_open_take();
        }
        if punch {
            self.start_punch();
        }
        if close {
            self.close_take();
        }
    }

    /// Tempo grid of the take being recorded (it starts on bar 1 when the metronome is on).
    fn live_grid(&self) -> Option<BeatGrid> {
        let s = self.session.as_ref()?;
        let m = &self.config.metronome;
        if self.rec_wave_start.is_none() || !m.enabled || !s.has_output() {
            return None;
        }
        Some(BeatGrid {
            beat_secs: 60.0 / m.bpm * 4.0 / f64::from(m.beat_unit.max(1)),
            beats_per_bar: m.beats_per_bar,
        })
    }

    /// Tempo grid of the opened take, if it was recorded on the metronome.
    fn take_grid(&self) -> Option<BeatGrid> {
        let tempo = self.open_take.as_ref()?.meta.tempo?;
        Some(BeatGrid {
            beat_secs: 60.0 / tempo.bpm * 4.0 / f64::from(tempo.beat_unit.max(1)),
            beats_per_bar: tempo.beats_per_bar,
        })
    }

    /// Switch between minutes:seconds and bars.beats for the ruler and the clocks.
    fn unit_toggle(&mut self, ui: &mut Ui) {
        let mut bars = self.config.ruler_bars;
        ui.spacing_mut().item_spacing.x = 2.0;
        ui.selectable_value(&mut bars, true, "小節")
            .on_hover_text("小節.拍で表示します (メトロノームを使って録ったテイクのみ)");
        ui.selectable_value(&mut bars, false, "秒")
            .on_hover_text("分:秒で表示します");
        if bars != self.config.ruler_bars {
            self.config.ruler_bars = bars;
            self.mark_dirty();
        }
    }

    /// Controls of the live lanes: how much of the recent input (or of the running take)
    /// they show.
    fn live_toolbar(&mut self, ui: &mut Ui) {
        let recording = self.recording_dir.is_some();
        let label = match self.live_window {
            LiveWindow::WholeTake if recording => "テイク全体".to_owned(),
            LiveWindow::WholeTake => format!("直近 {} 秒", LiveWindow::DEFAULT_SECS),
            LiveWindow::Secs(s) if s >= 60 => format!("直近 {} 分", s / 60),
            LiveWindow::Secs(s) => format!("直近 {s} 秒"),
        };
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("表示範囲")
                    .font(theme::regular(theme::TEXT_SMALL))
                    .color(theme::INK_TERTIARY),
            );
            if ui
                .small_button("−")
                .on_hover_text("より長い範囲を表示")
                .clicked()
            {
                self.live_window = self.live_window.wider();
            }
            if ui
                .small_button("＋")
                .on_hover_text("より短い範囲を表示")
                .clicked()
            {
                self.live_window = self.live_window.narrower();
            }
            if ui
                .add_enabled(recording, Button::new("全体").small())
                .on_hover_text("録音中のテイクを最初から表示")
                .on_disabled_hover_text("録音中に使えます")
                .clicked()
            {
                self.live_window = LiveWindow::WholeTake;
            }
            ui.label(
                RichText::new(label)
                    .font(theme::mono(theme::TEXT_SMALL))
                    .color(theme::INK_SUBTLE),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                self.unit_toggle(ui);
            });
        });
    }

    // ---- takes ---------------------------------------------------------------------------

    fn take_panel(&mut self, ui: &mut Ui) {
        let mut drag: Option<Vec<PathBuf>> = None;
        let mut open: Option<PathBuf> = None;
        let mut edit: Option<PathBuf> = None;
        // Take to open, and whether to start playing it right away.
        let mut listen: Option<(PathBuf, bool)> = None;
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
                if icon_button(
                    ui,
                    icon::CARET_DOUBLE_RIGHT,
                    "テイクの一覧を折りたたむ (トラックを広く表示)",
                    false,
                )
                .clicked()
                {
                    self.config.takes_collapsed = true;
                    self.mark_dirty();
                }
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
                    let is_open = self.open_take.as_ref().is_some_and(|t| t.dir == take.dir);
                    take_card(
                        ui,
                        take,
                        is_open,
                        &mut drag,
                        &mut open,
                        &mut edit,
                        &mut listen,
                    );
                }
            });

        if let Some((dir, play)) = listen {
            self.open_take(&dir);
            if play {
                self.play_from_start();
            }
        }

        if let Some(dir) = edit
            && let Some(take) = self.takes.iter().find(|t| t.dir == dir).cloned()
        {
            self.begin_take_edit(&take);
        }

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

    /// The folded take list: just a button to bring it back and the number of takes.
    fn take_rail(&mut self, ui: &mut Ui) {
        ui.vertical_centered(|ui| {
            if icon_button(ui, icon::CARET_DOUBLE_LEFT, "テイクの一覧を表示", false).clicked()
            {
                self.config.takes_collapsed = false;
                self.mark_dirty();
            }
            ui.add_space(theme::SPACE_XS);
            ui.label(
                RichText::new(self.takes.len().to_string())
                    .font(theme::mono(theme::TEXT_SMALL))
                    .color(theme::INK_SUBTLE),
            )
            .on_hover_text("テイクの数");
        });
    }

    /// Dialog for renaming a take, attaching a note, or moving it to the Recycle Bin.
    fn take_edit_dialog(&mut self, ctx: &egui::Context) {
        enum Action {
            Keep,
            Save,
            Cancel,
            Delete,
        }
        let Some(mut edit) = self.take_edit.clone() else {
            return;
        };
        let mut action = Action::Keep;
        let modal = egui::Modal::new(Id::unique("take_edit")).show(ctx, |ui| {
            ui.set_width(EDIT_DIALOG_WIDTH);
            ui.label(
                RichText::new("テイクを編集")
                    .font(theme::semibold(theme::TEXT_TITLE))
                    .color(theme::INK),
            );
            ui.add_space(theme::SPACE_XS);
            field_label(ui, "テイク名 (フォルダ名の末尾に付きます)");
            ui.add(
                TextEdit::singleline(&mut edit.label)
                    .hint_text("例: サビ案")
                    .margin(Margin::symmetric(10, 7))
                    .desired_width(f32::INFINITY),
            );
            field_label(ui, "メモ");
            ui.add(
                TextEdit::multiline(&mut edit.note)
                    .hint_text("例: 2 番のサビが良い")
                    .margin(Margin::symmetric(10, 7))
                    .desired_rows(3)
                    .desired_width(f32::INFINITY),
            );
            ui.add_space(theme::SPACE_SM);
            if edit.confirm_delete {
                ui.label(
                    RichText::new(
                        "このテイクのフォルダをごみ箱に移動します。ごみ箱から元に戻せます。",
                    )
                    .font(theme::regular(theme::TEXT_SMALL))
                    .color(theme::WARN),
                );
                ui.horizontal(|ui| {
                    if ui
                        .button(RichText::new("ごみ箱に移動する").color(theme::REC))
                        .clicked()
                    {
                        action = Action::Delete;
                    }
                    if ui.button("やめる").clicked() {
                        edit.confirm_delete = false;
                    }
                });
            } else {
                ui.horizontal(|ui| {
                    if ui
                        .add(
                            Button::new(theme::with_icon(
                                icon::TRASH,
                                "ごみ箱へ…",
                                theme::medium(theme::TEXT_BODY),
                                theme::INK_SUBTLE,
                            ))
                            .frame(false),
                        )
                        .clicked()
                    {
                        edit.confirm_delete = true;
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .add(
                                Button::new(RichText::new("保存").color(Color32::WHITE))
                                    .fill(theme::ACCENT),
                            )
                            .clicked()
                        {
                            action = Action::Save;
                        }
                        if ui.button("キャンセル").clicked() {
                            action = Action::Cancel;
                        }
                    });
                });
            }
        });
        if modal.should_close() && matches!(action, Action::Keep) {
            action = Action::Cancel;
        }
        self.take_edit = Some(edit);
        match action {
            Action::Keep => {}
            Action::Save => self.save_take_edit(),
            Action::Cancel => self.take_edit = None,
            Action::Delete => self.delete_edited_take(),
        }
    }

    // ---- status bar / toasts -------------------------------------------------------------

    fn status_bar(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label(theme::with_icon(
                icon::KEYBOARD,
                "Space 録音/停止  ·  P 一時停止  ·  M マーカー  ·  Enter 再生/停止",
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
    is_open: bool,
    drag: &mut Option<Vec<PathBuf>>,
    open: &mut Option<PathBuf>,
    edit: &mut Option<PathBuf>,
    listen: &mut Option<(PathBuf, bool)>,
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
                if is_open {
                    theme::INK_SUBTLE
                } else {
                    theme::mix(theme::HAIRLINE, theme::HAIRLINE_STRONG, hover)
                },
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
                    let name = ui.add(
                        Label::new(
                            RichText::new(&take.name)
                                .font(theme::medium(theme::TEXT_BODY))
                                .color(theme::INK),
                        )
                        .truncate()
                        .selectable(false)
                        .sense(Sense::click()),
                    );
                    if !take.in_progress {
                        if name.clicked() {
                            *listen = Some((take.dir.clone(), false));
                        }
                        name.on_hover_text("クリックで開く (波形を表示)")
                            .on_hover_cursor(egui::CursorIcon::PointingHand);
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if icon_button(ui, icon::FOLDER_OPEN, "フォルダを開く", false).clicked()
                        {
                            *open = Some(take.dir.clone());
                        }
                        if !take.in_progress
                            && icon_button(ui, icon::PLAY, "開いて再生", false).clicked()
                        {
                            *listen = Some((take.dir.clone(), true));
                        }
                        if !take.in_progress
                            && icon_button(
                                ui,
                                icon::PENCIL_SIMPLE,
                                "名前・メモを編集 / 削除",
                                false,
                            )
                            .clicked()
                        {
                            *edit = Some(take.dir.clone());
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
                        if let Some(tempo) = &meta.tempo {
                            badge(ui, &format!("{:.1} BPM", tempo.bpm), theme::INK_SUBTLE);
                        }
                        if !meta.markers.is_empty() {
                            badge(
                                ui,
                                &format!("マーカー {}", meta.markers.len()),
                                theme::INK_SUBTLE,
                            );
                        }
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

                if let Some(note) = take
                    .meta
                    .as_ref()
                    .map(|m| &m.note)
                    .filter(|n| !n.is_empty())
                {
                    ui.add(
                        Label::new(
                            RichText::new(note)
                                .font(theme::regular(theme::TEXT_SMALL + 0.5))
                                .color(theme::INK_MUTED),
                        )
                        .wrap(),
                    );
                }

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

/// Monitor / mute / solo toggles and the level menu of a track. Returns true on any change.
fn track_monitor_controls(ui: &mut Ui, t: &mut Track) -> bool {
    let before = t.clone();
    if toggle_chip(ui, icon::HEADPHONES, true, t.monitor, theme::ACCENT_HOVER)
        .on_hover_text(
            "モニター: この入力をアプリ経由で聴きます (少し遅れて聴こえます)。\n             インターフェース側のダイレクトモニターと同時に使うと二重に聴こえます",
        )
        .clicked()
    {
        t.monitor = !t.monitor;
    }
    if toggle_chip(ui, "M", false, t.mute, theme::WARN)
        .on_hover_text("ミュート: 聴こえなくします (録音はされます)")
        .clicked()
    {
        t.mute = !t.mute;
    }
    if toggle_chip(ui, "S", false, t.solo, theme::METER_LOW)
        .on_hover_text("ソロ: このトラックだけを聴きます (録音は全トラックされます)")
        .clicked()
    {
        t.solo = !t.solo;
    }
    ui.menu_button(
        RichText::new(icon::SLIDERS_HORIZONTAL).font(theme::icons(14.0)),
        |ui| {
            ui.set_min_width(240.0);
            field_label(ui, "モニター音量 (録音には影響しません)");
            ui.add(
                Slider::new(&mut t.volume_db, MIN_VOLUME_DB..=MAX_VOLUME_DB)
                    .fixed_decimals(1)
                    .suffix(" dB"),
            );
            field_label(ui, "パン (左 -1 〜 右 1)");
            ui.add(Slider::new(&mut t.pan, -1.0..=1.0).fixed_decimals(2));
            if ui.button("音量とパンを戻す").clicked() {
                t.volume_db = 0.0;
                t.pan = 0.0;
            }
            ui.separator();
            ui.checkbox(&mut t.loopback, "ループバック入力")
                .on_hover_text(
                    "PC の再生音を録る入力に付ける印です。\n                     付けておくと、メトロノームの音が録音に混ざるときに警告します",
                );
        },
    );
    *t != before
}

fn count_in_label(bars: u32) -> String {
    match bars {
        0 => "カウントインなし".to_owned(),
        n => format!("カウントイン {n} 小節"),
    }
}

/// Text button of the update banner; `primary` marks the one recommended action.
fn banner_button(ui: &mut Ui, text: &str, primary: bool) -> egui::Response {
    let label = RichText::new(text).font(theme::medium(theme::TEXT_BODY));
    if primary {
        ui.add(Button::new(label.color(theme::INK)).fill(theme::ACCENT))
    } else {
        ui.add(Button::new(label.color(theme::INK_SUBTLE)).frame(false))
    }
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
