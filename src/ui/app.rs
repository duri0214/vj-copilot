use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

use eframe::egui::{
    self, Context, Frame, Image, Key, RichText, Sense, Stroke, TextureHandle, TextureOptions, Ui,
    Vec2,
};

use crate::{
    domain::{
        service::{rank_clips, AnalysisTick, CandidateRefresh, CandidateState, PREVIEW_SLOT_COUNT},
        valueobject::{AnalysisReading, AudioLevels, ClipId, FeatureVector, TempoReading},
    },
    infra::{
        analysis_worker::{AnalysisUpdate, AnalysisWorker, INPUT_IDLE_TIMEOUT},
        audio_input::{AudioInput, AudioSource, InputStatus},
        media::{
            MediaClip, MediaLibrary, MediaLoadReport, VIDEO_FRAMES_PER_SECOND, VIDEO_HEIGHT,
            VIDEO_WIDTH,
        },
    },
};

use super::{level_meter::LevelMeter, theme};

#[cfg(all(windows, feature = "asio"))]
use crate::infra::audio_input::AudioBackend;

const THUMBNAIL_SIZE: Vec2 = Vec2::new(112.0, 63.0);
const SCROLL_CONTENT_RIGHT_MARGIN: f32 = 20.0;

pub struct AppConfig {
    pub media_dir: Option<PathBuf>,
    pub demo: bool,
    pub notices: Vec<String>,
    pub media_report: MediaLoadReport,
}

pub struct VjApp {
    media_dir: Option<PathBuf>,
    library: MediaLibrary,
    notices: Vec<String>,
    demo: bool,
    audio_input: AudioInput,
    analysis_worker: Option<AnalysisWorker>,
    analysis_ticks: Vec<AnalysisUpdate>,
    processing_ms: Option<f32>,
    display_ms: Option<f32>,
    latest_reading: Option<AnalysisReading>,
    latest_levels: Option<AudioLevels>,
    tempo: TempoReading,
    last_beat_at: Option<Instant>,
    last_reading_at: Option<Instant>,
    level_meter: LevelMeter,
    latest_search_features: Option<FeatureVector>,
    candidates: CandidateState,
    candidate_refresh: CandidateRefresh,
    needs_candidate_refresh: bool,
    preview_slots: [PreviewSlot; PREVIEW_SLOT_COUNT],
    started_at: Instant,
    last_animation_at: Instant,
}

impl VjApp {
    pub fn new(config: AppConfig) -> Self {
        let now = Instant::now();
        let mut notices = config.notices;
        notices.extend(config.media_report.notices);
        Self {
            media_dir: config.media_dir,
            library: config.media_report.library,
            notices,
            demo: config.demo,
            audio_input: AudioInput::new(),
            analysis_worker: config.demo.then(AnalysisWorker::start_demo),
            analysis_ticks: Vec::with_capacity(32),
            processing_ms: None,
            display_ms: None,
            latest_reading: None,
            latest_levels: None,
            tempo: TempoReading::default(),
            last_beat_at: None,
            last_reading_at: None,
            level_meter: LevelMeter::new(now),
            latest_search_features: None,
            candidates: CandidateState::default(),
            candidate_refresh: CandidateRefresh::default(),
            needs_candidate_refresh: false,
            preview_slots: std::array::from_fn(|_| PreviewSlot::default()),
            started_at: now,
            last_animation_at: now,
        }
    }

    fn collect_analysis_ticks(&mut self, now: Instant) -> bool {
        self.audio_input.poll_status();
        if !self.demo && !self.audio_input.is_capturing() {
            self.stop_analysis_worker();
            self.clear_analysis();
            return false;
        }
        let mut ticks = std::mem::take(&mut self.analysis_ticks);
        if let Some(worker) = self.analysis_worker.as_mut() {
            worker.drain_ticks(&mut ticks);
        } else {
            ticks.clear();
        }
        let mut received_search_features = false;
        for update in ticks.drain(..) {
            if now.saturating_duration_since(update.callback_at) >= INPUT_IDLE_TIMEOUT {
                continue;
            }
            self.last_reading_at = Some(update.callback_at);
            self.processing_ms = Some(
                update
                    .analyzed_at
                    .saturating_duration_since(update.callback_at)
                    .as_secs_f32()
                    * 1_000.0,
            );
            self.display_ms = Some(
                now.saturating_duration_since(update.callback_at)
                    .as_secs_f32()
                    * 1_000.0,
            );
            received_search_features |= self.record_analysis_tick(update.tick, update.callback_at);
            self.level_meter.update(Some(update.tick.levels), now);
        }
        self.analysis_ticks = ticks;
        if self
            .last_reading_at
            .is_some_and(|last| now.saturating_duration_since(last) >= INPUT_IDLE_TIMEOUT)
        {
            self.clear_analysis();
        }

        received_search_features
    }

    fn record_analysis_tick(&mut self, tick: AnalysisTick, now: Instant) -> bool {
        self.latest_levels = Some(tick.levels);
        self.tempo = tick.tempo;
        if tick.tempo.beat {
            self.last_beat_at = Some(now);
        }
        if let Some(reading) = tick.reading {
            self.latest_reading = Some(reading);
            self.latest_search_features = tick.search_features;
        }
        if let Some(features) = tick.search_features {
            self.latest_search_features = Some(features);
            true
        } else {
            false
        }
    }

    fn update_candidates(&mut self, now: Instant, received_search_features: bool) {
        if self.candidates.is_held() || (!received_search_features && !self.needs_candidate_refresh)
        {
            return;
        }

        let Some(features) = self.latest_search_features else {
            return;
        };
        let elapsed = now.saturating_duration_since(self.started_at);
        if !self.candidate_refresh.due_at(elapsed) {
            return;
        }

        let ranked = rank_clips(features, self.library.metadata());
        if self.candidates.update_ranked(&ranked) {
            self.sync_preview_slots();
        }
        self.needs_candidate_refresh = false;
    }

    fn sync_preview_slots(&mut self) {
        let candidate_ids = self.candidates.slots().clone();
        for (slot, clip_id) in self.preview_slots.iter_mut().zip(candidate_ids) {
            slot.set_clip(clip_id);
        }
    }

    fn handle_shortcuts(&mut self, context: &Context) {
        if context.wants_keyboard_input() {
            return;
        }
        if context.input(|input| input.key_pressed(Key::Space)) {
            self.toggle_hold();
        }

        let selected_slot = context.input(|input| {
            [Key::Num1, Key::Num2, Key::Num3, Key::Num4]
                .iter()
                .position(|key| input.key_pressed(*key))
        });
        if let Some(slot) = selected_slot {
            self.select_slot(slot);
        }
    }

    fn select_slot(&mut self, slot: usize) {
        if self.candidates.select(slot) {
            self.needs_candidate_refresh = false;
        }
    }

    fn toggle_hold(&mut self) {
        let was_held = self.candidates.is_held();
        self.candidates.toggle_hold();

        if was_held {
            self.candidate_refresh.reset();
            self.needs_candidate_refresh = true;
        }
    }

    fn advance_animation(&mut self, now: Instant) {
        let elapsed = now.saturating_duration_since(self.last_animation_at);
        let frames_to_advance =
            (elapsed.as_secs_f64() * VIDEO_FRAMES_PER_SECOND as f64).floor() as usize;
        if frames_to_advance == 0 {
            return;
        }

        self.last_animation_at +=
            Duration::from_secs_f64(frames_to_advance as f64 / VIDEO_FRAMES_PER_SECOND as f64);
        let library = &self.library;
        for preview_slot in &mut self.preview_slots {
            let frame_count = preview_slot
                .clip_id
                .as_ref()
                .and_then(|id| library.find(id))
                .map(|clip| clip.frames.len())
                .unwrap_or(0);
            preview_slot.advance(frame_count, frames_to_advance);
        }
    }

    fn show_input_controls(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            theme::caption(ui, "01 / AUDIO SOURCE");
            if self.demo {
                theme::badge(ui, "DEMO", theme::AMBER);
            } else if self.audio_input.is_capturing() {
                theme::badge(ui, "LISTENING", theme::ACCENT);
            } else {
                theme::badge(ui, "STANDBY", theme::MUTED);
            }
        });
        if self.demo {
            ui.label("120 BPM のデモ音源でプレビューを確認中");
            return;
        }

        let capturing = self.audio_input.is_capturing();
        ui.add_enabled_ui(!capturing, |ui| {
            ui.horizontal(|ui| {
                #[cfg(all(windows, feature = "asio"))]
                {
                    let mut backend = self.audio_input.backend();
                    ui.selectable_value(&mut backend, AudioBackend::System, "WASAPI");
                    ui.selectable_value(&mut backend, AudioBackend::Asio, "ASIO");
                    self.audio_input.select_backend(backend);
                    ui.separator();
                }
                let mut source = self.audio_input.source();
                #[cfg(windows)]
                if self.audio_input.supports_loopback() {
                    ui.selectable_value(&mut source, AudioSource::SystemPlayback, "PC 再生音");
                }
                ui.selectable_value(&mut source, AudioSource::LineInput, "LINE / MIC");
                self.audio_input.select_source(source);
            });
            #[cfg(all(windows, feature = "asio"))]
            if self.audio_input.backend() == AudioBackend::Asio {
                let mut channel = self.audio_input.first_channel();
                ui.horizontal(|ui| {
                    ui.label("入力 ch");
                    ui.add(egui::DragValue::new(&mut channel).range(1..=128));
                    ui.label(
                        RichText::new("隣の ch と mono に合成（最終 ch は単独）")
                            .size(11.0)
                            .color(theme::MUTED),
                    );
                });
                self.audio_input.select_first_channel(channel);
            }
        });
        ui.horizontal(|ui| {
            ui.add_enabled_ui(!capturing, |ui| {
                let mut selected_device = self.audio_input.selected_device().map(str::to_owned);
                egui::ComboBox::from_id_salt("audio-device")
                    .width((ui.available_width() - 198.0).max(160.0))
                    .selected_text(selected_device.as_deref().unwrap_or("デバイスなし"))
                    .show_ui(ui, |ui| {
                        for device_name in self.audio_input.device_names() {
                            ui.selectable_value(
                                &mut selected_device,
                                Some(device_name.clone()),
                                device_name,
                            );
                        }
                    });
                self.audio_input.select_device(selected_device);
                if ui.button("再読込").clicked() {
                    self.audio_input.refresh_devices();
                }
            });
            if capturing {
                if ui
                    .add(
                        egui::Button::new(RichText::new("停止").color(theme::RED))
                            .min_size(Vec2::new(76.0, 32.0)),
                    )
                    .clicked()
                {
                    self.audio_input.stop();
                    self.stop_analysis_worker();
                    self.clear_analysis();
                }
            } else if ui
                .add_enabled(
                    self.audio_input.selected_device().is_some(),
                    egui::Button::new(RichText::new("開始").strong().color(theme::BACKGROUND))
                        .fill(theme::ACCENT)
                        .min_size(Vec2::new(76.0, 32.0)),
                )
                .clicked()
            {
                self.stop_analysis_worker();
                self.clear_analysis();
                if let Some(captured) = self.audio_input.start() {
                    self.analysis_worker = Some(AnalysisWorker::start_captured(captured));
                    self.candidate_refresh.reset();
                }
            }
        });
        if self.audio_input.source() == AudioSource::LineInput {
            ui.label(
                RichText::new("ミキサーの LINE 出力、またはマイクを取り込みます。")
                    .size(11.0)
                    .color(theme::MUTED),
            );
        }
        let status_color = match self.audio_input.status() {
            InputStatus::Error(_) | InputStatus::Unsupported(_) | InputStatus::NoDevice => {
                theme::RED
            }
            _ => theme::MUTED,
        };
        ui.label(
            RichText::new(self.audio_input.status().message())
                .size(11.0)
                .color(status_color),
        );
        let dropped = self.audio_input.dropped_samples();
        if dropped > 0 {
            ui.colored_label(
                theme::AMBER,
                format!("バッファ満杯のため {dropped} samples を破棄しました"),
            );
        }
    }

    fn show_analysis(&self, ui: &mut Ui) {
        if let Some(reading) = self.latest_reading {
            ui.label(
                RichText::new(format!(
                    "ENERGY  {:.2}     BRIGHTNESS  {:.2}     {:.0} Hz",
                    reading.features.energy(),
                    reading.features.brightness(),
                    reading.centroid_hz,
                ))
                .monospace()
                .size(11.0)
                .color(theme::MUTED),
            );
        } else {
            ui.label(
                RichText::new("ENERGY  --     BRIGHTNESS  --")
                    .monospace()
                    .size(11.0)
                    .color(theme::MUTED),
            );
        }
    }

    fn show_tempo(&self, ui: &mut Ui, now: Instant) {
        ui.horizontal(|ui| {
            let lit = self.tempo.bpm.is_some()
                && self.last_beat_at.is_some_and(|at| {
                    now.saturating_duration_since(at) < Duration::from_millis(100)
                });
            let (rect, response) = ui.allocate_exact_size(Vec2::splat(28.0), Sense::hover());
            ui.painter().circle_filled(
                rect.center(),
                12.0,
                theme::ACCENT.gamma_multiply(if lit { 0.25 } else { 0.04 }),
            );
            ui.painter().circle_filled(
                rect.center(),
                6.0,
                if lit { theme::ACCENT } else { theme::BORDER },
            );
            response.on_hover_text("検出した拍で点灯します。推定中はタイミングが揺れることがあります。");
            let bpm = self
                .tempo
                .bpm
                .map_or_else(|| "--".to_owned(), |bpm| format!("{bpm:.1}"));
            ui.add_sized(
                Vec2::new(98.0, 38.0),
                egui::Label::new(RichText::new(bpm).monospace().size(30.0).strong())
                    .halign(egui::Align::RIGHT),
            );
            theme::caption(ui, "BPM");
            let status = if self.latest_levels.is_none() {
                "入力待ち"
            } else if self.tempo.stable {
                "安定"
            } else {
                "推定中"
            };
            theme::badge(
                ui,
                status,
                if self.tempo.stable { theme::ACCENT } else { theme::AMBER },
            );
            ui.label(
                RichText::new(format!("信頼度 {:.0}%", self.tempo.confidence * 100.0))
                    .size(11.0)
                    .color(theme::MUTED),
            )
            .on_hover_text("周期性の強さの目安です。BPM が正しい確率ではありません。半分・倍のテンポを拾う場合があります。");
        });
    }

    fn show_timing(&self, ui: &mut Ui) {
        if self.demo {
            return;
        }
        egui::CollapsingHeader::new("入力タイミング / 検証").show(ui, |ui| {
            if let Some(timing) = self.audio_input.timing() {
                ui.label(
                    RichText::new(format!(
                        "コールバック: {:>8} frames / {:>8.1} ms 分の音声",
                        timing.frames, timing.buffer_ms,
                    ))
                    .monospace(),
                );
                ui.label(
                    RichText::new(format!("到着間隔: {:>8.1} ms", timing.interval_ms))
                        .monospace(),
                );
            }
            if let (Some(processing), Some(display)) = (self.processing_ms, self.display_ms) {
                ui.label(
                    RichText::new(format!(
                        "受信 → 解析: {processing:>8.1} ms / 受信 → 描画要求: {display:>8.1} ms"
                    ))
                    .monospace(),
                );
            } else {
                ui.label("入力を開始すると計測します。");
            }
            ui.label(
                RichText::new("直近の値。機器・ドライバーの遅延、画面の表示遅延は含みません。音量の集計窓は 25 ms です。")
                    .size(11.0)
                    .color(theme::MUTED),
            );
        });
    }

    fn analysis_status(&self) -> &'static str {
        if !self.demo && !self.audio_input.is_capturing() {
            return "入力を開始すると候補を提案します";
        }

        match self.latest_reading {
            None => "音声待機中 — 音源の再生とデバイスを確認してください",
            Some(reading) if !reading.audible => "無音を検出: 候補を保持中",
            Some(_) if self.latest_search_features.is_none() => "有音入力を 1 秒分待機中",
            Some(_) if self.candidates.is_held() => "候補を固定中",
            Some(_) => "候補を自動更新中",
        }
    }

    fn show_candidate_controls(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            theme::caption(ui, "02 / CLIP CANDIDATES");
            theme::badge(ui, &format!("{} CLIPS", self.library.len()), theme::MUTED);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let (button_text, tooltip) = if self.candidates.is_held() {
                    (
                        "自動更新を再開",
                        "候補の固定を解除して、自動更新を再開します。選択表示も解除します。",
                    )
                } else {
                    ("候補を固定", "現在の候補を固定し、自動更新を停止します。")
                };
                let color = if self.candidates.is_held() {
                    theme::AMBER
                } else {
                    theme::ACCENT
                };
                if ui
                    .button(RichText::new(button_text).strong().color(color))
                    .on_hover_text(tooltip)
                    .clicked()
                {
                    self.toggle_hold();
                }
            });
        });
        ui.label(
            RichText::new(self.analysis_status())
                .size(11.0)
                .color(theme::MUTED),
        );
    }

    fn show_preview_grid(&mut self, ui: &mut Ui) {
        let candidate_ids = self.candidates.slots().clone();
        let selected_slot = self.candidates.selected_slot();
        let is_held = self.candidates.is_held();
        let mut clicked_slot = None;
        let card_width = ((ui.available_width() - 12.0) / 2.0).floor();

        for (row_index, row) in candidate_ids.chunks(2).enumerate() {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 12.0;
                for (column_index, candidate_id) in row.iter().enumerate() {
                    let slot_index = row_index * 2 + column_index;
                    let clip = candidate_id.as_ref().and_then(|id| self.library.find(id));
                    let clicked = self.preview_slots[slot_index].show(
                        ui,
                        slot_index,
                        clip,
                        selected_slot == Some(slot_index),
                        is_held,
                        card_width,
                    );
                    if clicked {
                        clicked_slot = Some(slot_index);
                    }
                }
            });
        }

        if let Some(slot) = clicked_slot {
            self.select_slot(slot);
        }
    }

    fn show_notices(&self, ui: &mut Ui) {
        if self.notices.is_empty() {
            return;
        }

        egui::CollapsingHeader::new(format!("素材の状態 / {} 件", self.notices.len()))
            .default_open(self.library.len() == 0)
            .show(ui, |ui| {
                for notice in &self.notices {
                    ui.colored_label(theme::AMBER, notice);
                }
            });
    }

    fn clear_analysis(&mut self) {
        self.latest_reading = None;
        self.latest_levels = None;
        self.tempo = TempoReading::default();
        self.last_beat_at = None;
        self.processing_ms = None;
        self.display_ms = None;
        self.last_reading_at = None;
        self.latest_search_features = None;
    }

    fn stop_analysis_worker(&mut self) {
        if let Some(mut worker) = self.analysis_worker.take() {
            worker.stop();
        }
    }
}

impl Drop for VjApp {
    fn drop(&mut self) {
        self.audio_input.stop();
        self.stop_analysis_worker();
    }
}

impl eframe::App for VjApp {
    fn update(&mut self, context: &Context, _frame: &mut eframe::Frame) {
        let now = Instant::now();
        self.advance_animation(now);
        self.handle_shortcuts(context);
        let received_search_features = self.collect_analysis_ticks(now);
        self.update_candidates(now, received_search_features);
        self.level_meter.update(self.latest_levels, now);

        egui::CentralPanel::default()
            .frame(Frame::new().fill(theme::BACKGROUND).inner_margin(20))
            .show(context, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.set_width((ui.available_width() - SCROLL_CONTENT_RIGHT_MARGIN).max(0.0));
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("VJ").size(25.0).strong().color(theme::ACCENT));
                        ui.label(RichText::new("COPILOT").size(25.0).strong());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            theme::badge(ui, "PREVIEW", theme::MUTED);
                        });
                    });
                    ui.label(
                        RichText::new("音を聴く。映像を選ぶ。")
                            .size(12.0)
                            .color(theme::MUTED),
                    );
                    ui.add_space(8.0);
                    theme::panel().show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        self.show_input_controls(ui);
                        ui.add_space(6.0);
                        self.level_meter.show(ui, self.latest_levels, now);
                        self.show_tempo(ui, now);
                        self.show_analysis(ui);
                        self.show_timing(ui);
                    });
                    ui.add_space(8.0);
                    self.show_candidate_controls(ui);
                    self.show_preview_grid(ui);
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new("1—4  選択     SPACE  候補固定 / 再開")
                            .monospace()
                            .size(11.0)
                            .color(theme::MUTED),
                    );
                    if let Some(media_dir) = self.media_dir.as_ref() {
                        ui.add(
                            egui::Label::new(
                                RichText::new(format!("LIBRARY  {}", media_dir.display()))
                                    .size(10.0)
                                    .color(theme::MUTED),
                            )
                            .truncate(),
                        );
                    }
                    self.show_notices(ui);
                });
            });

        context.request_repaint_after(Duration::from_millis(16));
    }
}

#[derive(Default)]
struct PreviewSlot {
    clip_id: Option<ClipId>,
    frame_index: usize,
    texture: Option<TextureHandle>,
    uploaded_frame: Option<usize>,
}

impl PreviewSlot {
    fn set_clip(&mut self, clip_id: Option<ClipId>) {
        if self.clip_id != clip_id {
            self.clip_id = clip_id;
            self.frame_index = 0;
            self.texture = None;
            self.uploaded_frame = None;
        }
    }

    fn advance(&mut self, frame_count: usize, frames_to_advance: usize) {
        if frame_count > 0 {
            self.frame_index = (self.frame_index + frames_to_advance) % frame_count;
        }
    }

    fn show(
        &mut self,
        ui: &mut Ui,
        slot_index: usize,
        clip: Option<&MediaClip>,
        selected: bool,
        held: bool,
        width: f32,
    ) -> bool {
        let stroke = if selected {
            Stroke::new(1.0_f32, theme::ACCENT)
        } else if held {
            Stroke::new(1.0_f32, theme::AMBER.gamma_multiply(0.6))
        } else {
            Stroke::new(1.0_f32, theme::BORDER)
        };
        let label = clip
            .map(|clip| clip.metadata.id.as_str())
            .unwrap_or("Nothing");
        let response = Frame::new()
            .fill(if selected {
                egui::Color32::from_rgb(24, 48, 49)
            } else {
                theme::PANEL
            })
            .stroke(stroke)
            .corner_radius(8)
            .inner_margin(12)
            .show(ui, |ui| {
                ui.set_width(width - 28.0);
                ui.horizontal(|ui| {
                    let frame = clip.and_then(|clip| {
                        if clip.frames.is_empty() {
                            None
                        } else {
                            clip.frames.get(self.frame_index % clip.frames.len())
                        }
                    });
                    if let Some(frame) = frame {
                        if self.uploaded_frame != Some(self.frame_index) {
                            let image = egui::ColorImage::from_rgba_unmultiplied(
                                [VIDEO_WIDTH, VIDEO_HEIGHT],
                                &frame.rgba,
                            );
                            if let Some(texture) = self.texture.as_mut() {
                                texture.set(image, TextureOptions::LINEAR);
                            } else {
                                self.texture = Some(ui.ctx().load_texture(
                                    format!("preview-slot-{slot_index}"),
                                    image,
                                    TextureOptions::LINEAR,
                                ));
                            }
                            self.uploaded_frame = Some(self.frame_index);
                        }
                        if let Some(texture) = &self.texture {
                            ui.add(Image::new((texture.id(), THUMBNAIL_SIZE)).corner_radius(4));
                        }
                    } else {
                        let (response, painter) =
                            ui.allocate_painter(THUMBNAIL_SIZE, Sense::hover());
                        painter.rect_filled(response.rect, 4.0, theme::BACKGROUND);
                        painter.text(
                            response.rect.center(),
                            egui::Align2::CENTER_CENTER,
                            "Nothing",
                            egui::FontId::proportional(12.0),
                            theme::MUTED,
                        );
                    }
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            theme::badge(
                                ui,
                                &(slot_index + 1).to_string(),
                                if selected {
                                    theme::ACCENT
                                } else {
                                    theme::MUTED
                                },
                            );
                            if selected {
                                ui.label(
                                    RichText::new("SELECTED")
                                        .size(10.0)
                                        .strong()
                                        .color(theme::ACCENT),
                                );
                            }
                        });
                        ui.add(
                            egui::Label::new(RichText::new(label).size(12.0).strong()).truncate(),
                        );
                        let hint = if clip.is_some() {
                            "クリックで選択"
                        } else {
                            "候補なし"
                        };
                        ui.label(RichText::new(hint).size(10.0).color(theme::MUTED));
                    });
                });
            })
            .response;
        let response = ui.interact(
            response.rect,
            ui.id().with(("candidate", slot_index)),
            if clip.is_some() {
                Sense::click()
            } else {
                Sense::hover()
            },
        );
        if selected || (response.hovered() && clip.is_some()) {
            ui.painter().rect_stroke(
                response.rect,
                8.0,
                Stroke::new(if selected { 2.0_f32 } else { 1.0_f32 }, theme::ACCENT),
                egui::StrokeKind::Inside,
            );
        }
        let clicked = response.clicked();
        response.on_hover_text(label);
        clicked
    }
}
