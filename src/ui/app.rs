use std::{
    collections::HashMap,
    path::PathBuf,
    time::{Duration, Instant},
};

use eframe::egui::{
    self, pos2, Context, Frame, Image, Key, Mesh, Rect, RichText, Sense, Shape, Stroke,
    TextureHandle, TextureOptions, Ui, Vec2, ViewportBuilder, ViewportId,
};

use crate::{
    app_icon,
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
const STAGE_PREVIEW_WIDTH: f32 = 960.0;
const SCROLL_CONTENT_RIGHT_MARGIN: f32 = 20.0;

#[derive(Clone, Copy, PartialEq, Eq)]
enum MediaTab {
    Background,
    Foreground,
}

#[derive(Clone, Debug, PartialEq)]
struct PlacedForeground {
    name: String,
    /// Position as a fraction of the 16:9 stage, independent of window size.
    offset: Vec2,
}

#[derive(Default)]
struct ForegroundState {
    selected: Option<String>,
    cued: Option<String>,
    cue_armed: bool,
    live: Option<PlacedForeground>,
    offsets: HashMap<String, Vec2>,
}

impl ForegroundState {
    fn cue(&mut self) {
        self.cued = self.selected.clone();
        self.cue_armed = self.cued.is_some();
    }

    fn take(&mut self) {
        if !self.cue_armed {
            return;
        }
        if let Some(name) = &self.cued {
            self.live = Some(PlacedForeground {
                name: name.clone(),
                offset: self.offsets.get(name).copied().unwrap_or(Vec2::ZERO),
            });
            self.cue_armed = false;
            self.selected = None;
        }
    }

    fn cue_offset(&self) -> Vec2 {
        self.cued
            .as_ref()
            .and_then(|name| self.offsets.get(name))
            .copied()
            .unwrap_or(Vec2::ZERO)
    }

    fn move_cue(&mut self, delta: Vec2) {
        if !self.cue_armed {
            return;
        }
        if let Some(name) = &self.cued {
            *self.offsets.entry(name.clone()).or_default() += delta;
        }
    }

    fn reset_cue(&mut self) {
        if !self.cue_armed {
            return;
        }
        if let Some(name) = &self.cued {
            self.offsets.remove(name);
        }
    }
}

#[cfg(test)]
mod foreground_tests {
    use super::*;

    #[test]
    fn cue_and_movement_leave_live_unchanged_until_play() {
        let mut state = ForegroundState {
            selected: Some("first.png".into()),
            ..Default::default()
        };
        state.cue();
        state.move_cue(Vec2::new(0.2, 0.3));
        assert!(state.live.is_none());

        state.take();
        let first_live = state.live.clone();
        assert!(!state.cue_armed);
        assert!(state.selected.is_none());
        state.move_cue(Vec2::new(0.1, 0.1));
        state.reset_cue();
        assert_eq!(state.cue_offset(), Vec2::new(0.2, 0.3));
        state.selected = Some("second.png".into());
        state.cue();
        state.move_cue(Vec2::new(-0.1, 0.4));
        assert_eq!(state.live, first_live);

        state.take();
        assert_eq!(state.live.unwrap().name, "second.png");
    }

    #[test]
    fn positions_survive_switching_and_clearing_live() {
        let mut state = ForegroundState {
            selected: Some("first.png".into()),
            ..Default::default()
        };
        state.cue();
        state.move_cue(Vec2::new(0.25, -0.2));
        state.take();
        state.live = None;
        assert_eq!(state.cue_offset(), Vec2::new(0.25, -0.2));

        state.selected = Some("second.png".into());
        state.cue();
        assert_eq!(state.cue_offset(), Vec2::ZERO);
        state.selected = Some("first.png".into());
        state.cue();
        assert_eq!(state.cue_offset(), Vec2::new(0.25, -0.2));
        state.reset_cue();
        assert_eq!(state.cue_offset(), Vec2::ZERO);
    }
}

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
    foreground: ForegroundState,
    media_tab: MediaTab,
    preview_window_open: bool,
    stage_texture: Option<TextureHandle>,
    foreground_preview_textures: HashMap<String, TextureHandle>,
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
            foreground: ForegroundState::default(),
            media_tab: MediaTab::Background,
            preview_window_open: true,
            stage_texture: None,
            foreground_preview_textures: HashMap::new(),
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
        let tab_direction = context.input(|input| {
            if input.key_pressed(Key::ArrowLeft) {
                Some(-1_i8)
            } else if input.key_pressed(Key::ArrowRight) {
                Some(1_i8)
            } else {
                None
            }
        });
        if let Some(direction) = tab_direction {
            self.media_tab = match (self.media_tab, direction) {
                (MediaTab::Background, _) => MediaTab::Foreground,
                (MediaTab::Foreground, _) => MediaTab::Background,
            };
        }
        let space_pressed = context.input(|input| input.key_pressed(Key::Space));
        if space_pressed {
            match self.media_tab {
                MediaTab::Background if self.candidates.is_held() => self.resume_auto_mode(),
                MediaTab::Foreground => self.foreground.selected = None,
                _ => {}
            }
        }

        let selected_slot = context.input(|input| {
            [Key::Num1, Key::Num2, Key::Num3, Key::Num4]
                .iter()
                .position(|key| input.key_pressed(*key))
        });
        if let Some(slot) = selected_slot {
            match self.media_tab {
                MediaTab::Background => self.select_slot(slot),
                MediaTab::Foreground => self.select_foreground_slot(slot),
            }
        }
    }

    fn select_foreground_slot(&mut self, slot: usize) {
        let selected = self
            .library
            .foregrounds()
            .get(slot)
            .map(|foreground| foreground.name.clone());
        self.foreground.selected = if selected == self.foreground.selected {
            None
        } else {
            selected
        };
    }

    fn select_slot(&mut self, slot: usize) {
        if self.candidates.select(slot) {
            self.needs_candidate_refresh = false;
        }
    }

    fn resume_auto_mode(&mut self) {
        self.candidates.release();
        self.candidate_refresh.reset();
        self.needs_candidate_refresh = true;
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

    fn show_stage(&mut self, ui: &mut Ui, now: Instant, staging: bool) {
        let width = ui
            .available_width()
            .min(if staging { 560.0 } else { STAGE_PREVIEW_WIDTH });
        let size = Vec2::new(width, width * 9.0 / 16.0);
        let (response, painter) = ui.allocate_painter(
            size,
            if staging && self.foreground.cue_armed {
                Sense::drag()
            } else {
                Sense::hover()
            },
        );
        painter.rect_filled(response.rect, 4.0, theme::BACKGROUND);

        let selected_frame = self.candidates.selected_slot().and_then(|slot| {
            self.candidates
                .slots()
                .get(slot)
                .and_then(Option::as_ref)
                .and_then(|id| self.library.find(id))
                .and_then(|clip| {
                    clip.frames
                        .get(self.preview_slots[slot].frame_index % clip.frames.len())
                })
        });
        if let Some(frame) = selected_frame {
            let image =
                egui::ColorImage::from_rgba_unmultiplied([VIDEO_WIDTH, VIDEO_HEIGHT], &frame.rgba);
            if let Some(texture) = self.stage_texture.as_mut() {
                texture.set(image, TextureOptions::LINEAR);
            } else {
                self.stage_texture = Some(ui.ctx().load_texture(
                    "stage-background",
                    image,
                    TextureOptions::LINEAR,
                ));
            }
            if let Some(texture) = &self.stage_texture {
                painter.image(
                    texture.id(),
                    response.rect,
                    Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
            }
        }

        let placed = if staging {
            self.foreground.cued.as_ref().map(|name| PlacedForeground {
                name: name.clone(),
                offset: self.foreground.cue_offset(),
            })
        } else {
            self.foreground.live.clone()
        };
        if let Some(mut placed) = placed {
            if staging && self.foreground.cue_armed && response.dragged() {
                let delta = response.drag_delta() / response.rect.size();
                self.foreground.move_cue(delta);
                placed.offset = self.foreground.cue_offset();
            }
            self.ensure_foreground_texture(ui.ctx(), &placed.name);
            self.paint_foreground(&painter, response.rect, now, &placed);
        }
        if staging && self.foreground.cue_armed {
            response.on_hover_text("STAGING / ドラッグで Foreground を移動");
        }
    }

    fn ensure_foreground_texture(&mut self, context: &Context, name: &str) {
        if self.foreground_preview_textures.contains_key(name) {
            return;
        }
        let Some(foreground) = self.library.foreground(name) else {
            return;
        };
        let image = egui::ColorImage::from_rgba_unmultiplied(
            [foreground.width, foreground.height],
            &foreground.rgba,
        );
        self.foreground_preview_textures.insert(
            name.to_owned(),
            context.load_texture(format!("foreground-{name}"), image, TextureOptions::LINEAR),
        );
    }

    fn paint_foreground(
        &self,
        painter: &egui::Painter,
        stage: Rect,
        now: Instant,
        placed: &PlacedForeground,
    ) {
        let Some(texture) = self.foreground_preview_textures.get(&placed.name) else {
            return;
        };
        let Some(foreground) = self.library.foreground(&placed.name) else {
            return;
        };
        let scale = (STAGE_PREVIEW_WIDTH / foreground.width as f32)
            .min((STAGE_PREVIEW_WIDTH * 9.0 / 16.0) / foreground.height as f32)
            .min(1.0)
            * stage.width()
            / STAGE_PREVIEW_WIDTH;
        let image_size = Vec2::new(
            foreground.width as f32 * scale,
            foreground.height as f32 * scale,
        );
        let image_rect =
            Rect::from_center_size(stage.center() + placed.offset * stage.size(), image_size);
        let pivot = image_rect.min
            + Vec2::new(
                foreground.opaque_center.0 * scale,
                foreground.opaque_center.1 * scale,
            );
        let phase = now.saturating_duration_since(self.started_at).as_secs_f32()
            * std::f32::consts::TAU
            / 3.0;
        let horizontal_scale = phase.cos();
        let transform =
            |point: egui::Pos2| pos2(pivot.x + (point.x - pivot.x) * horizontal_scale, point.y);
        let positions = [
            transform(image_rect.left_top()),
            transform(image_rect.right_top()),
            transform(image_rect.right_bottom()),
            transform(image_rect.left_bottom()),
        ];
        let uvs = [
            pos2(0.0, 0.0),
            pos2(1.0, 0.0),
            pos2(1.0, 1.0),
            pos2(0.0, 1.0),
        ];
        let mut mesh = Mesh::with_texture(texture.id());
        for (position, uv) in positions.into_iter().zip(uvs) {
            mesh.vertices.push(egui::epaint::Vertex {
                pos: position,
                uv,
                color: egui::Color32::WHITE,
            });
        }
        mesh.indices.extend_from_slice(&[0, 1, 2, 0, 2, 3]);
        painter.add(Shape::mesh(mesh));
    }

    fn show_foreground_controls(&mut self, ui: &mut Ui) {
        ui.label(
            RichText::new("候補を選択 → CUE → 配置 → PLAY")
                .size(11.0)
                .color(theme::MUTED),
        );
        if self.library.foregrounds().is_empty() {
            ui.label(
                RichText::new("foreground/ に透過 PNG を追加してください")
                    .size(11.0)
                    .color(theme::MUTED),
            );
            return;
        }
        let names = self
            .library
            .foregrounds()
            .iter()
            .map(|foreground| foreground.name.clone())
            .collect::<Vec<_>>();
        self.show_foreground_grid(ui, &names);
        ui.add_space(8.0);
        theme::panel().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                theme::caption(ui, "STAGING");
                if self.foreground.cue_armed {
                    theme::badge(ui, "CUE READY", theme::AMBER);
                } else if self.foreground.cued.is_some() {
                    theme::badge(ui, "PLAYED", theme::MUTED);
                }
            });
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(
                        self.foreground.selected.is_some(),
                        egui::Button::new("● CUE").min_size(Vec2::new(76.0, 30.0)),
                    )
                    .on_hover_text("選択した素材を STAGING に読み込む")
                    .clicked()
                {
                    self.foreground.cue();
                }
                if ui
                    .add_enabled(
                        self.foreground.cue_armed,
                        egui::Button::new("↺ RESET POSITION").small(),
                    )
                    .on_hover_text("STAGING の位置だけを中央へ戻す")
                    .clicked()
                {
                    self.foreground.reset_cue();
                }
            });
            if let Some(name) = &self.foreground.cued {
                ui.label(
                    RichText::new(if self.foreground.cue_armed {
                        format!("CUE  {name}  •  ドラッグで位置を調整")
                    } else {
                        format!("{name}  •  次の CUE は素材を選択してから")
                    })
                    .size(11.0)
                    .color(theme::MUTED),
                );
            } else {
                ui.label(
                    RichText::new("素材を選んで CUE を押すと、ここで出力前に確認できます")
                        .size(11.0)
                        .color(theme::MUTED),
                );
            }
            self.show_stage(ui, Instant::now(), true);
            ui.add_space(8.0);
            ui.separator();
            ui.horizontal(|ui| {
                theme::caption(ui, "LIVE STAGE");
                if let Some(live) = &self.foreground.live {
                    ui.label(RichText::new(&live.name).size(10.0).color(theme::ACCENT));
                }
            });
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(
                        self.foreground.cue_armed,
                        egui::Button::new("▶ PLAY").min_size(Vec2::new(82.0, 30.0)),
                    )
                    .on_hover_text("STAGING の素材と位置を LIVE STAGE に出す")
                    .clicked()
                {
                    self.foreground.take();
                }
                if ui
                    .add_enabled(
                        self.foreground.live.is_some(),
                        egui::Button::new("× CLEAR LIVE").small(),
                    )
                    .on_hover_text("LIVE STAGE の Foreground だけを消す")
                    .clicked()
                {
                    self.foreground.live = None;
                }
            });
        });
    }

    fn show_foreground_grid(&mut self, ui: &mut Ui, names: &[String]) {
        let card_width = ((ui.available_width() - 12.0) / 2.0).floor();
        for (row_index, row) in names.chunks(2).enumerate() {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 12.0;
                for (column_index, name) in row.iter().enumerate() {
                    let index = row_index * 2 + column_index;
                    let selected = self.foreground.selected.as_deref() == Some(name.as_str());
                    let texture = self
                        .foreground_preview_textures
                        .entry(name.clone())
                        .or_insert_with(|| {
                            let foreground = self
                                .library
                                .foreground(name)
                                .expect("foreground name from library");
                            let image = egui::ColorImage::from_rgba_unmultiplied(
                                [foreground.width, foreground.height],
                                &foreground.rgba,
                            );
                            ui.ctx().load_texture(
                                format!("foreground-preview-{name}"),
                                image,
                                TextureOptions::LINEAR,
                            )
                        })
                        .clone();
                    let response = Frame::new()
                        .fill(if selected {
                            egui::Color32::from_rgb(24, 48, 49)
                        } else {
                            theme::PANEL
                        })
                        .stroke(Stroke::new(
                            if selected { 2.0_f32 } else { 1.0_f32 },
                            if selected {
                                theme::ACCENT
                            } else {
                                theme::BORDER
                            },
                        ))
                        .corner_radius(8)
                        .inner_margin(12)
                        .show(ui, |ui| {
                            ui.set_width(card_width - 28.0);
                            ui.horizontal(|ui| {
                                ui.add(Image::new((texture.id(), THUMBNAIL_SIZE)).corner_radius(4));
                                ui.vertical(|ui| {
                                    theme::badge(
                                        ui,
                                        &(index + 1).to_string(),
                                        if selected {
                                            theme::ACCENT
                                        } else {
                                            theme::MUTED
                                        },
                                    );
                                    ui.add(
                                        egui::Label::new(RichText::new(name).size(12.0).strong())
                                            .truncate(),
                                    );
                                    ui.label(
                                        RichText::new("クリックで選択")
                                            .size(10.0)
                                            .color(theme::MUTED),
                                    );
                                });
                            });
                        })
                        .response;
                    let response = ui.interact(
                        response.rect,
                        ui.id().with(("foreground", index)),
                        Sense::click(),
                    );
                    if response.clicked() {
                        self.foreground.selected = Some(name.clone());
                    }
                }
            });
        }
    }

    fn show_preview_window_button(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            theme::caption(ui, "STAGE OUTPUT");
            if self.preview_window_open {
                theme::badge(ui, "OPEN", theme::ACCENT);
            } else if ui.button("OPEN STAGE WINDOW").clicked() {
                self.preview_window_open = true;
            }
            ui.label(
                RichText::new("プロジェクタへ移動できる独立プレビュー")
                    .size(11.0)
                    .color(theme::MUTED),
            );
        });
    }

    fn show_preview_window(&mut self, context: &Context) {
        if !self.preview_window_open {
            return;
        }

        let viewport_id = ViewportId::from_hash_of("vj-copilot-stage-preview");
        let viewport_builder = ViewportBuilder::default()
            .with_title("VJ Copilot - STAGE")
            .with_inner_size([960.0, 540.0])
            .with_min_inner_size([320.0, 180.0])
            .with_icon(std::sync::Arc::new(app_icon::app_icon()));
        let mut open = true;
        context.show_viewport_immediate(viewport_id, viewport_builder, |viewport_context, _| {
            if viewport_context.input(|input| input.viewport().close_requested()) {
                open = false;
                return;
            }

            egui::CentralPanel::default()
                .frame(Frame::new().fill(theme::BACKGROUND))
                .show(viewport_context, |ui| {
                    self.show_stage(ui, Instant::now(), false);
                });
            viewport_context.request_repaint_after(Duration::from_millis(16));
        });
        self.preview_window_open = open;
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

    fn analysis_status(&self) -> String {
        if let Some(slot) = self.candidates.selected_slot() {
            return format!("候補 {} を選択中", slot + 1);
        }
        if !self.demo && !self.audio_input.is_capturing() {
            return "入力を開始すると候補を提案します".to_owned();
        }

        match self.latest_reading {
            None => "音声待機中 — 音源の再生とデバイスを確認してください",
            Some(reading) if !reading.audible => "無音を検出: 候補を保持中",
            Some(_) if self.latest_search_features.is_none() => "有音入力を 1 秒分待機中",
            Some(_) => "候補を自動更新中",
        }
        .to_owned()
    }

    fn show_candidate_controls(&self, ui: &mut Ui) {
        ui.label(
            RichText::new(self.analysis_status())
                .size(11.0)
                .color(theme::MUTED),
        );
    }

    fn show_preview_grid(&mut self, ui: &mut Ui) {
        let candidate_ids = self.candidates.slots().clone();
        let selected_slot = self.candidates.selected_slot();
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

    fn show_media_tabs(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            let background_label = format!("BACKGROUND ({})", self.library.len());
            let background =
                ui.selectable_label(self.media_tab == MediaTab::Background, background_label);
            if background.clicked() {
                self.media_tab = MediaTab::Background;
            }
            let foreground_label = format!("FOREGROUND ({})", self.library.foregrounds().len());
            let foreground =
                ui.selectable_label(self.media_tab == MediaTab::Foreground, foreground_label);
            if foreground.clicked() {
                self.media_tab = MediaTab::Foreground;
            }
        });
        ui.add_space(4.0);
        match self.media_tab {
            MediaTab::Background => {
                self.show_candidate_controls(ui);
                self.show_preview_grid(ui);
            }
            MediaTab::Foreground => self.show_foreground_controls(ui),
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
            .frame(
                Frame::new()
                    .fill(theme::BACKGROUND)
                    .inner_margin(egui::Margin {
                        left: 20,
                        right: 0,
                        top: 20,
                        bottom: 20,
                    }),
            )
            .show(context, |ui| {
                let scroll_area = egui::ScrollArea::vertical().auto_shrink([false, true]);
                scroll_area.show(ui, |ui| {
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
                    self.show_preview_window_button(ui);
                    ui.add_space(8.0);
                    self.show_media_tabs(ui);
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new("← →  タブ切替     1—4  選択     SPACE  選択解除")
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

        self.show_preview_window(context);
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
        width: f32,
    ) -> bool {
        let stroke = if selected {
            Stroke::new(1.0_f32, theme::ACCENT)
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
