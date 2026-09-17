use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, FontId, Rect, Sense, Stroke, Ui, Vec2};

use crate::domain::valueobject::AudioLevels;

use super::theme;

const FLOOR_DBFS: f32 = -60.0;
const PEAK_HOLD: Duration = Duration::from_secs(1);

pub struct LevelMeter {
    rms_dbfs: f32,
    peak_dbfs: f32,
    peak_at: Instant,
    last_update: Instant,
    clip_at: Option<Instant>,
}

impl LevelMeter {
    pub fn new(now: Instant) -> Self {
        Self {
            rms_dbfs: FLOOR_DBFS,
            peak_dbfs: FLOOR_DBFS,
            peak_at: now,
            last_update: now,
            clip_at: None,
        }
    }

    pub fn update(&mut self, reading: Option<AudioLevels>, now: Instant) {
        let elapsed = now
            .saturating_duration_since(self.last_update)
            .as_secs_f32();
        self.last_update = now;
        let Some(reading) = reading else {
            *self = Self::new(now);
            return;
        };
        self.rms_dbfs = reading.rms_dbfs.max(self.rms_dbfs - elapsed * 36.0);
        if reading.peak_dbfs >= self.peak_dbfs {
            self.peak_dbfs = reading.peak_dbfs;
            self.peak_at = now;
        } else if now.saturating_duration_since(self.peak_at) >= PEAK_HOLD {
            self.peak_dbfs = reading.peak_dbfs.max(self.peak_dbfs - elapsed * 36.0);
        }
        if reading.peak_dbfs >= -0.1 {
            self.clip_at = Some(now);
        }
    }

    pub fn show(&self, ui: &mut Ui, reading: Option<AudioLevels>, now: Instant) {
        ui.horizontal(|ui| {
            theme::caption(ui, "INPUT LEVEL / 25 ms");
            let text = reading
                .map(|reading| format!("{:.1} dBFS", reading.rms_dbfs))
                .unwrap_or_else(|| "-- dBFS".to_owned());
            ui.add_sized(
                Vec2::new(96.0, 18.0),
                egui::Label::new(egui::RichText::new(text).monospace()).halign(egui::Align::RIGHT),
            );
            if self
                .clip_at
                .is_some_and(|at| now.saturating_duration_since(at) < PEAK_HOLD)
            {
                theme::badge(ui, "CLIP", theme::RED);
            }
        });
        let (rect, response) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), 34.0), Sense::hover());
        response.on_hover_text(
            "バーは平均音量（RMS）、白線はピークを1秒保持。stereo は mono に平均して表示します。",
        );
        let painter = ui.painter();
        const SEGMENTS: usize = 40;
        let segment_width = rect.width() / SEGMENTS as f32;
        for index in 0..SEGMENTS {
            let threshold = FLOOR_DBFS + (index + 1) as f32 / SEGMENTS as f32 * -FLOOR_DBFS;
            let color = if threshold > -3.0 {
                theme::RED
            } else if threshold > -12.0 {
                theme::AMBER
            } else {
                theme::ACCENT
            };
            let segment = Rect::from_min_size(
                rect.min + Vec2::new(index as f32 * segment_width, 0.0),
                Vec2::new((segment_width - 2.0).max(1.0), 12.0),
            );
            painter.rect_filled(
                segment,
                2.0,
                if self.rms_dbfs >= threshold {
                    color
                } else {
                    color.gamma_multiply(0.12)
                },
            );
        }
        if self.peak_dbfs > FLOOR_DBFS {
            let peak_x = rect.left() + fraction(self.peak_dbfs) * (rect.width() - 2.0);
            painter.line_segment(
                [
                    egui::pos2(peak_x, rect.top() - 1.0),
                    egui::pos2(peak_x, rect.top() + 13.0),
                ],
                Stroke::new(2.0_f32, Color32::WHITE),
            );
        }
        for dbfs in [-60, -36, -12, 0] {
            let align = if dbfs == 0 {
                egui::Align2::RIGHT_TOP
            } else {
                egui::Align2::LEFT_TOP
            };
            painter.text(
                rect.min + Vec2::new(fraction(dbfs as f32) * rect.width(), 18.0),
                align,
                dbfs.to_string(),
                FontId::monospace(10.0),
                theme::MUTED,
            );
        }
    }
}

fn fraction(dbfs: f32) -> f32 {
    ((dbfs - FLOOR_DBFS) / -FLOOR_DBFS).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reading(dbfs: f32) -> AudioLevels {
        AudioLevels {
            rms_dbfs: dbfs,
            peak_dbfs: dbfs,
        }
    }

    #[test]
    fn holds_transient_peak_for_one_second_then_releases_it() {
        let now = Instant::now();
        let mut meter = LevelMeter::new(now);
        meter.update(Some(reading(0.0)), now);
        meter.update(Some(reading(-40.0)), now + Duration::from_millis(500));
        assert_eq!(meter.peak_dbfs, 0.0);
        assert!(meter.rms_dbfs < 0.0);

        meter.update(Some(reading(-40.0)), now + Duration::from_millis(1_100));
        assert!(meter.peak_dbfs < 0.0);
        assert!(meter.peak_dbfs >= -40.0);
    }

    #[test]
    fn stopped_or_stale_input_immediately_clears_the_meter_and_clip_indicator() {
        let now = Instant::now();
        let mut meter = LevelMeter::new(now);
        meter.update(Some(reading(0.0)), now);
        assert!(meter.clip_at.is_some());

        meter.update(None, now + Duration::from_millis(100));
        assert_eq!(meter.rms_dbfs, FLOOR_DBFS);
        assert_eq!(meter.peak_dbfs, FLOOR_DBFS);
        assert!(meter.clip_at.is_none());
    }
}
