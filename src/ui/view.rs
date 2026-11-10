//! Drawing of the main window. State changes go through the actions in `app.rs`.

use std::path::PathBuf;

use eframe::egui::{
    self, Button, Color32, ComboBox, Label, RichText, ScrollArea, Sense, Slider, TextEdit, Ui, Vec2,
};

use super::app::{Level, RecrecApp, format_bytes, format_duration};
use super::widgets::{REC_RED, clip_led, db_label, meter, record_button};
use crate::meter::METER_FLOOR_DB;
use crate::platform;
use crate::recorder::MAX_PREROLL_SECS;
use crate::track::Source;
use crate::wav::SampleFormat;

const INPUT_METER_SIZE: Vec2 = Vec2::new(90.0, 10.0);
const TRACK_METER_WIDTH: f32 = 220.0;
const TRACK_METER_HEIGHT: f32 = 14.0;
const WARN_COLOR: Color32 = Color32::from_rgb(230, 170, 40);

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

        egui::Panel::top("device_bar").show(ui, |ui| self.device_bar(ui));
        egui::Panel::bottom("status_bar").show(ui, |ui| self.status_bar(ui));
        egui::CentralPanel::default_margins().show(ui, |ui| {
            self.transport(ui);
            ui.separator();
            self.input_strip(ui);
            ui.separator();
            self.track_list(ui);
            ui.separator();
            self.take_list(ui);
        });
    }

    fn device_bar(&mut self, ui: &mut Ui) {
        let busy = self.is_busy();
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            ui.add_enabled_ui(!busy, |ui| {
                ui.label("ホスト");
                let mut host = self.host;
                ComboBox::from_id_salt("host")
                    .selected_text(host.name())
                    .show_ui(ui, |ui| {
                        for h in self.hosts.clone() {
                            ui.selectable_value(&mut host, h, h.name());
                        }
                    });
                if host != self.host {
                    self.select_host(host);
                }

                ui.label("デバイス");
                let mut device = self.device.clone();
                ComboBox::from_id_salt("device")
                    .selected_text(device.as_deref().unwrap_or("(なし)"))
                    .width(220.0)
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

                if let Some(info) = self.current_device_info().cloned() {
                    let current_rate = self.session.as_ref().map_or_else(
                        || info.pick_rate(self.config.sample_rate),
                        |s| s.info.sample_rate,
                    );
                    let mut rate = current_rate;
                    ui.label("レート");
                    ComboBox::from_id_salt("rate")
                        .selected_text(format!("{rate} Hz"))
                        .show_ui(ui, |ui| {
                            for r in &info.rates {
                                ui.selectable_value(&mut rate, *r, format!("{r} Hz"));
                            }
                        });
                    if rate != current_rate {
                        self.select_rate(rate);
                    }

                    let choices = info.buffer_choices();
                    if !choices.is_empty() {
                        let current = self.session.as_ref().and_then(|s| s.info.buffer_size);
                        let mut buffer = current;
                        ui.label("バッファ");
                        ComboBox::from_id_salt("buffer")
                            .selected_text(
                                buffer.map_or("ドライバ既定".to_owned(), |b| b.to_string()),
                            )
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut buffer, None, "ドライバ既定");
                                for b in choices {
                                    ui.selectable_value(&mut buffer, Some(b), b.to_string());
                                }
                            });
                        if buffer != current {
                            self.select_buffer(buffer);
                        }
                    }
                }

                if ui
                    .button("⟳ 再接続")
                    .on_hover_text("デバイス一覧を読み直して開き直します")
                    .clicked()
                {
                    self.reconnect();
                }
            });
        });
        match (&self.session, &self.session_error) {
            (_, Some(err)) => {
                ui.colored_label(REC_RED, err);
            }
            (Some(s), None) => {
                let i = &s.info;
                let buffer = i
                    .buffer_size
                    .map_or("ドライバ既定".to_owned(), |b| format!("{b} サンプル"));
                ui.weak(format!(
                    "{} · {} · {} 入力 · {} · {} Hz · バッファ {buffer}",
                    i.host, i.device, i.channels, i.device_format, i.sample_rate
                ));
            }
            (None, None) => {
                ui.weak("入力デバイスが開かれていません");
            }
        }
        ui.add_space(2.0);
    }

    fn transport(&mut self, ui: &mut Ui) {
        let busy = self.is_busy();
        let recording = self.recording_dir.is_some();
        ui.horizontal(|ui| {
            if record_button(ui, busy, self.can_record() || busy).clicked() {
                self.toggle_recording();
            }
            ui.add_space(8.0);
            ui.vertical(|ui| {
                let time = RichText::new(format_duration(self.elapsed_secs()))
                    .monospace()
                    .size(32.0);
                ui.label(if recording { time.color(REC_RED) } else { time });
                ui.horizontal(|ui| {
                    ui.label("テイク名");
                    ui.add_enabled(
                        !busy,
                        TextEdit::singleline(&mut self.take_label)
                            .hint_text("任意 (フォルダ名に付きます)")
                            .desired_width(220.0),
                    );
                });
            });
        });

        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            ui.add_enabled_ui(!busy, |ui| {
                ui.label("形式");
                let mut format = self.config.format;
                ComboBox::from_id_salt("format")
                    .selected_text(format.label())
                    .show_ui(ui, |ui| {
                        for f in SampleFormat::ALL {
                            ui.selectable_value(&mut format, f, f.label());
                        }
                    });
                if format != self.config.format {
                    self.config.format = format;
                    self.mark_dirty();
                }

                ui.label("プリロール");
                let mut secs = self.config.preroll_secs;
                let slider = ui
                    .add(Slider::new(&mut secs, 0..=MAX_PREROLL_SECS).suffix(" 秒"))
                    .on_hover_text("録音ボタンを押す前の N 秒もファイルに含めます");
                if slider.changed() {
                    self.set_preroll(secs);
                }

                ui.label("出力先");
                let dir = self.config.output_dir();
                ui.add(Label::new(RichText::new(dir.display().to_string()).monospace()).truncate())
                    .on_hover_text(dir.display().to_string());
                if ui.button("変更…").clicked()
                    && let Some(picked) = rfd::FileDialog::new().set_directory(&dir).pick_folder()
                {
                    self.set_output_dir(picked);
                }
            });
            if ui.button("開く").clicked() {
                let dir = self.config.output_dir();
                match std::fs::create_dir_all(&dir) {
                    Ok(()) => self.open_folder(&dir),
                    Err(e) => self.push(Level::Error, format!("出力先を作れません: {e}")),
                }
            }
        });
    }

    fn input_strip(&mut self, ui: &mut Ui) {
        let Some(session) = &self.session else {
            return;
        };
        let meters = session.meters.clone();
        ui.horizontal_wrapped(|ui| {
            ui.strong("入力");
            for (ch, display) in self.meters.iter().enumerate() {
                ui.label(format!("In {}", ch + 1));
                meter(ui, display, INPUT_METER_SIZE);
                if clip_led(ui, meters.clipped(ch)).clicked() {
                    meters.reset_clip(ch);
                }
                ui.add_space(6.0);
            }
        });
    }

    fn track_list(&mut self, ui: &mut Ui) {
        let busy = self.is_busy();
        let channels = self.session.as_ref().map_or(0, |s| s.info.channels);
        let bank = self.session.as_ref().map(|s| s.meters.clone());
        let mut changed = false;
        let mut remove = None;

        ui.strong("トラック");
        egui::Grid::new("tracks")
            .num_columns(8)
            .spacing([8.0, 6.0])
            .striped(true)
            .show(ui, |ui| {
                for (i, t) in self.tracks.iter_mut().enumerate() {
                    let dot = RichText::new("●").color(if t.armed {
                        REC_RED
                    } else {
                        ui.visuals().weak_text_color()
                    });
                    if ui
                        .add_enabled(!busy, Button::new(dot).selected(t.armed))
                        .on_hover_text("録音する / しない")
                        .clicked()
                    {
                        t.armed = !t.armed;
                        changed = true;
                    }
                    ui.label(format!("{:02}", i + 1));
                    if ui
                        .add_enabled(
                            !busy,
                            TextEdit::singleline(&mut t.name).desired_width(140.0),
                        )
                        .changed()
                    {
                        changed = true;
                    }
                    ui.add_enabled_ui(!busy, |ui| {
                        let before = t.source;
                        ComboBox::from_id_salt(("source", i))
                            .selected_text(t.source.label())
                            .width(150.0)
                            .show_ui(ui, |ui| {
                                for s in Source::options(channels) {
                                    ui.selectable_value(&mut t.source, s, s.label());
                                }
                            });
                        changed |= before != t.source;
                    });

                    let sources = t.source.channels();
                    let height = TRACK_METER_HEIGHT / sources.len() as f32;
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 1.0;
                        for c in &sources {
                            if let Some(d) = self.meters.get(*c) {
                                meter(ui, d, Vec2::new(TRACK_METER_WIDTH, height));
                            }
                        }
                    });
                    let hold = sources
                        .iter()
                        .filter_map(|c| self.meters.get(*c))
                        .map(|d| d.hold_db)
                        .fold(METER_FLOOR_DB, f32::max);
                    ui.label(RichText::new(format!("{:>6} dB", db_label(hold))).monospace());
                    if let Some(bank) = &bank {
                        let clipped = sources
                            .iter()
                            .any(|c| *c < bank.channels() && bank.clipped(*c));
                        if clip_led(ui, clipped).clicked() {
                            for c in &sources {
                                if *c < bank.channels() {
                                    bank.reset_clip(*c);
                                }
                            }
                        }
                    } else {
                        ui.label("");
                    }
                    if ui
                        .add_enabled(!busy, Button::new("✖"))
                        .on_hover_text("トラックを削除")
                        .clicked()
                    {
                        remove = Some(i);
                    }
                    ui.end_row();
                }
            });

        ui.horizontal(|ui| {
            ui.add_enabled_ui(!busy && self.session.is_some(), |ui| {
                if ui.button("+ トラック追加").clicked() {
                    self.add_track();
                }
                if ui
                    .button("全入力をモノラルで作り直す")
                    .on_hover_text("入力 1 ch ごとにモノラル 1 トラックにします")
                    .clicked()
                {
                    self.reset_tracks();
                }
            });
        });

        if let Some(i) = remove {
            self.tracks.remove(i);
            changed = true;
        }
        if changed {
            self.tracks_changed();
        }
    }

    fn take_list(&mut self, ui: &mut Ui) {
        let mut drag: Option<Vec<PathBuf>> = None;
        let mut open: Option<PathBuf> = None;
        let mut refresh = false;
        let can_drag = platform::file_drag_supported();
        let drag_hint = if can_drag {
            "ドラッグして DAW などへドロップ"
        } else {
            "ファイルのドラッグは Windows 版のみ対応しています"
        };

        ui.horizontal(|ui| {
            ui.strong("テイク");
            if ui.small_button("⟳").on_hover_text("一覧を更新").clicked() {
                refresh = true;
            }
        });
        ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if self.takes.is_empty() {
                    ui.weak("まだテイクがありません。● で録音を始めます。");
                }
                for take in &self.takes {
                    let draggable = can_drag && !take.in_progress;
                    ui.horizontal(|ui| {
                        let handle = ui
                            .add(Label::new(RichText::new("☰").size(16.0)).sense(Sense::drag()))
                            .on_hover_text(format!("{drag_hint} (全ファイル)"));
                        if draggable && handle.drag_started() {
                            drag = Some(take.files.clone());
                        }
                        ui.label(RichText::new(&take.name).strong());
                        if let Some(meta) = &take.meta {
                            ui.weak(format!(
                                "{} · {} ファイル · {} · {} Hz",
                                format_duration(meta.duration_secs()),
                                take.files.len(),
                                meta.format.label(),
                                meta.sample_rate
                            ));
                            if meta.recovered {
                                ui.colored_label(WARN_COLOR, "修復済み");
                            }
                            if meta.dropped_frames > 0 {
                                ui.colored_label(
                                    REC_RED,
                                    format!("取りこぼし {} フレーム", meta.dropped_frames),
                                );
                            }
                        }
                        if take.in_progress {
                            ui.colored_label(REC_RED, "録音中");
                        }
                        if ui.small_button("フォルダを開く").clicked() {
                            open = Some(take.dir.clone());
                        }
                    });
                    ui.indent(take.dir.display().to_string(), |ui| {
                        ui.horizontal_wrapped(|ui| {
                            for f in &take.files {
                                let name = f
                                    .file_name()
                                    .map(|n| n.to_string_lossy().into_owned())
                                    .unwrap_or_default();
                                let r = ui
                                    .add(Label::new(format!("☰ {name}")).sense(Sense::drag()))
                                    .on_hover_text(drag_hint);
                                if draggable && r.drag_started() {
                                    drag = Some(vec![f.clone()]);
                                }
                                ui.add_space(8.0);
                            }
                        });
                    });
                    ui.add_space(2.0);
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

    fn status_bar(&mut self, ui: &mut Ui) {
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            if let Some(free) = self.free_space {
                let bps = self.armed_bytes_per_second();
                let text = if bps > 0 {
                    let hours = free as f64 / bps as f64 / 3600.0;
                    let remaining = if hours >= 1.0 {
                        format!("{hours:.0} 時間")
                    } else {
                        format!("{:.0} 分", hours * 60.0)
                    };
                    format!("空き {} (約 {remaining} 録音可能)", format_bytes(free))
                } else {
                    format!("空き {}", format_bytes(free))
                };
                ui.label(text);
                ui.separator();
            }
            if let Some(s) = &self.session {
                let dropped = if self.recording_dir.is_some() {
                    s.dropped_frames().saturating_sub(self.dropped_at_take_start)
                } else {
                    s.dropped_frames()
                };
                let text = format!("取りこぼし {dropped}");
                if dropped > 0 {
                    ui.colored_label(REC_RED, text)
                        .on_hover_text("書き込みが追いつかず捨てたフレーム数です。バッファを大きくするか、他の負荷を減らしてください");
                } else {
                    ui.weak(text);
                }
                ui.separator();
            }
            ui.menu_button("履歴", |ui| {
                ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                    for m in self.messages.iter().rev() {
                        ui.label(
                            RichText::new(format!("{} {}", m.at.format("%H:%M:%S"), m.text))
                                .color(level_color(ui, m.level)),
                        );
                    }
                    if self.messages.is_empty() {
                        ui.weak("メッセージはありません");
                    }
                });
            });
            if let Some(m) = self.messages.last() {
                ui.add(
                    Label::new(RichText::new(&m.text).color(level_color(ui, m.level))).truncate(),
                );
            }
        });
        ui.add_space(2.0);
    }
}

fn level_color(ui: &Ui, level: Level) -> Color32 {
    match level {
        Level::Info => ui.visuals().text_color(),
        Level::Warn => WARN_COLOR,
        Level::Error => REC_RED,
    }
}
