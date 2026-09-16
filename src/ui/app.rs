use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

use eframe::egui::{
    self, Color32, Context, Frame, Image, Key, Sense, Stroke, TextureHandle, TextureOptions, Ui,
    Vec2,
};

use crate::{
    domain::{
        service::{rank_clips, AnalysisTick, CandidateRefresh, CandidateState, PREVIEW_SLOT_COUNT},
        valueobject::{AnalysisReading, ClipId, FeatureVector},
    },
    infra::{
        analysis_worker::AnalysisWorker,
        audio_input::AudioInput,
        media::{
            MediaClip, MediaLibrary, MediaLoadReport, VIDEO_FRAMES_PER_SECOND, VIDEO_HEIGHT,
            VIDEO_WIDTH,
        },
    },
};

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
    analysis_ticks: Vec<AnalysisTick>,
    latest_reading: Option<AnalysisReading>,
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
            latest_reading: None,
            latest_search_features: None,
            candidates: CandidateState::default(),
            candidate_refresh: CandidateRefresh::default(),
            needs_candidate_refresh: false,
            preview_slots: std::array::from_fn(|_| PreviewSlot::default()),
            started_at: now,
            last_animation_at: now,
        }
    }

    fn collect_analysis_ticks(&mut self) -> bool {
        self.audio_input.poll_status();
        let mut ticks = std::mem::take(&mut self.analysis_ticks);
        if let Some(worker) = self.analysis_worker.as_mut() {
            worker.drain_ticks(&mut ticks);
        } else {
            ticks.clear();
        }
        let mut received_search_features = false;
        for tick in ticks.drain(..) {
            received_search_features |= self.record_analysis_tick(tick);
        }
        self.analysis_ticks = ticks;

        received_search_features
    }

    fn record_analysis_tick(&mut self, tick: AnalysisTick) -> bool {
        self.latest_reading = Some(tick.reading);
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
        if self.demo {
            ui.colored_label(Color32::YELLOW, "DEMO");
            ui.label("合成 PCM を同じ DSP 経路へ送っています。実機入力は別途確認してください。");
            return;
        }

        let device_names = self.audio_input.device_names().to_vec();
        let mut selected_device = self.audio_input.selected_device().map(str::to_owned);
        ui.horizontal(|ui| {
            egui::ComboBox::from_label("入力デバイス")
                .selected_text(selected_device.as_deref().unwrap_or("未選択"))
                .show_ui(ui, |ui| {
                    for device_name in &device_names {
                        ui.selectable_value(
                            &mut selected_device,
                            Some(device_name.clone()),
                            device_name,
                        );
                    }
                });

            if ui.button("再読込").clicked() {
                self.audio_input.refresh_devices();
                selected_device = self.audio_input.selected_device().map(str::to_owned);
            }
            if ui.button("開始").clicked() {
                self.stop_analysis_worker();
                if let Some(captured) = self.audio_input.start() {
                    self.analysis_worker = Some(AnalysisWorker::start_captured(captured));
                    self.latest_reading = None;
                    self.latest_search_features = None;
                    self.candidate_refresh.reset();
                }
            }
            if ui.button("停止").clicked() {
                self.audio_input.stop();
                self.stop_analysis_worker();
            }
        });

        if selected_device.as_deref() != self.audio_input.selected_device() {
            self.audio_input.select_device(selected_device);
        }

        ui.label(self.audio_input.status().message());
        let dropped = self.audio_input.dropped_samples();
        if dropped > 0 {
            ui.colored_label(
                Color32::YELLOW,
                format!("バッファ満杯のため {dropped} samples を破棄しました"),
            );
        }
    }

    fn show_analysis(&self, ui: &mut Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.label(format!("有効素材: {} 本", self.library.len()));
            ui.separator();
            ui.label(self.analysis_status());
        });

        if let Some(reading) = self.latest_reading {
            ui.monospace(format!(
                "RMS: {:.1} dBFS    energy: {:.3}    centroid: {:.0} Hz    brightness: {:.3}",
                reading.rms_dbfs,
                reading.features.energy(),
                reading.centroid_hz,
                reading.features.brightness(),
            ));
        } else {
            ui.monospace("RMS: -- dBFS    energy: --    centroid: -- Hz    brightness: --");
        }
    }

    fn analysis_status(&self) -> &'static str {
        if self.latest_reading.is_some() && !self.demo && !self.audio_input.is_capturing() {
            return "入力停止中: 候補を保持中";
        }

        match self.latest_reading {
            None => "有音入力を 1 秒分待機中",
            Some(reading) if !reading.audible => "無音を検出: 候補を保持中",
            Some(_) if self.latest_search_features.is_none() => "有音入力を 1 秒分待機中",
            Some(_) if self.candidates.is_held() => "候補更新を保留中",
            Some(_) => "候補を自動更新中",
        }
    }

    fn show_candidate_controls(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            let button_text = if self.candidates.is_held() {
                "保留を解除"
            } else {
                "候補更新を保留"
            };
            if ui.button(button_text).clicked() {
                self.toggle_hold();
            }

            if let Some(slot) = self.candidates.selected_slot() {
                if let Some(id) = self.candidates.slots()[slot].as_ref() {
                    ui.colored_label(Color32::YELLOW, format!("選択中: {}", id.as_str()));
                }
            } else if self.candidates.is_held() {
                ui.label("候補更新を保留中");
            } else {
                ui.label("未選択");
            }
        });
    }

    fn show_preview_grid(&mut self, ui: &mut Ui, context: &Context) {
        let candidate_ids = self.candidates.slots().clone();
        let selected_slot = self.candidates.selected_slot();
        let is_held = self.candidates.is_held();
        let mut clicked_slot = None;

        egui::Grid::new("preview_grid")
            .num_columns(2)
            .spacing(Vec2::new(12.0, 12.0))
            .show(ui, |ui| {
                for (slot_index, candidate_id) in candidate_ids.iter().enumerate() {
                    let clip = candidate_id.as_ref().and_then(|id| self.library.find(id));
                    let clicked = self.preview_slots[slot_index].show(
                        ui,
                        context,
                        slot_index,
                        clip,
                        selected_slot == Some(slot_index),
                        is_held,
                    );
                    if clicked {
                        clicked_slot = Some(slot_index);
                    }

                    if slot_index % 2 == 1 {
                        ui.end_row();
                    }
                }
            });

        if let Some(slot) = clicked_slot {
            self.select_slot(slot);
        }
    }

    fn show_notices(&self, ui: &mut Ui) {
        if self.notices.is_empty() {
            return;
        }

        ui.separator();
        ui.label("状態");
        for notice in &self.notices {
            ui.colored_label(Color32::YELLOW, notice);
        }
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
        let received_search_features = self.collect_analysis_ticks();
        self.update_candidates(now, received_search_features);

        egui::CentralPanel::default().show(context, |ui| {
            ui.heading("VJ Copilot — LINE 入力から動画候補を提示");
            if let Some(media_dir) = self.media_dir.as_ref() {
                ui.label(format!("素材フォルダ: {}", media_dir.display()));
            }
            self.show_input_controls(ui);
            ui.separator();
            self.show_analysis(ui);
            self.show_candidate_controls(ui);
            ui.label("枠をクリック、または 1〜4 キーで選択します。Space で保留を切り替えます。");
            ui.separator();
            self.show_preview_grid(ui, context);
            self.show_notices(ui);
        });

        context.request_repaint_after(Duration::from_millis(16));
    }
}

#[derive(Default)]
struct PreviewSlot {
    clip_id: Option<ClipId>,
    frame_index: usize,
    texture: Option<TextureHandle>,
}

impl PreviewSlot {
    fn set_clip(&mut self, clip_id: Option<ClipId>) {
        if self.clip_id != clip_id {
            self.clip_id = clip_id;
            self.frame_index = 0;
            self.texture = None;
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
        context: &Context,
        slot_index: usize,
        clip: Option<&MediaClip>,
        selected: bool,
        held: bool,
    ) -> bool {
        let stroke = if selected {
            Stroke::new(3.0_f32, Color32::YELLOW)
        } else if held {
            Stroke::new(1.0_f32, Color32::LIGHT_YELLOW)
        } else {
            Stroke::new(1.0_f32, Color32::DARK_GRAY)
        };
        let mut clicked = false;

        Frame::default().stroke(stroke).show(ui, |ui| {
            let label = clip
                .map(|clip| clip.metadata.id.as_str())
                .unwrap_or("空き枠");
            ui.label(format!("{}: {label}", slot_index + 1));

            let frame = clip.and_then(|clip| {
                if clip.frames.is_empty() {
                    None
                } else {
                    clip.frames.get(self.frame_index % clip.frames.len())
                }
            });
            if let Some(frame) = frame {
                let image = egui::ColorImage::from_rgba_unmultiplied(
                    [VIDEO_WIDTH, VIDEO_HEIGHT],
                    &frame.rgba,
                );
                let texture = self.texture.get_or_insert_with(|| {
                    context.load_texture(
                        format!("preview-slot-{slot_index}"),
                        image.clone(),
                        TextureOptions::LINEAR,
                    )
                });
                texture.set(image, TextureOptions::LINEAR);
                let response = ui.add(
                    Image::new((
                        texture.id(),
                        Vec2::new(VIDEO_WIDTH as f32, VIDEO_HEIGHT as f32),
                    ))
                    .sense(Sense::click()),
                );
                clicked = response.clicked();
            } else {
                let (response, painter) = ui.allocate_painter(
                    Vec2::new(VIDEO_WIDTH as f32, VIDEO_HEIGHT as f32),
                    Sense::click(),
                );
                painter.rect_filled(response.rect, 0.0, Color32::from_gray(24));
                painter.text(
                    response.rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "空き枠",
                    egui::TextStyle::Body.resolve(ui.style()),
                    Color32::GRAY,
                );
            }
        });

        clicked
    }
}
