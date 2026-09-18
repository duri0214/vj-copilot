use std::{collections::VecDeque, sync::Arc};

use rustfft::{num_complex::Complex, Fft, FftPlanner};

use crate::domain::valueobject::TempoReading;

const HOP_SECONDS: f32 = 0.01;
const HISTORY_FRAMES: usize = 800;
const MINIMUM_FRAMES: usize = 400;
const ESTIMATE_EVERY: usize = 50;
const MIN_BPM: f32 = 60.0;
const MAX_BPM: f32 = 200.0;
const FLUX_FLOOR: f32 = 0.015;

/// Experimental tempo tracking for candidate selection, not a beat-sync clock.
/// Positive log spectral flux supplies an onset envelope. Normalized
/// autocorrelation finds its period; repeated estimates establish stability.
pub(super) struct TempoTracker {
    hop_samples: usize,
    hop_seconds: f32,
    samples: VecDeque<f32>,
    since_hop: usize,
    fft: Arc<dyn Fft<f32>>,
    buffer: Vec<Complex<f32>>,
    scratch: Vec<Complex<f32>>,
    window: Vec<f32>,
    previous_spectrum: Vec<f32>,
    envelope: VecDeque<f32>,
    onsets: VecDeque<usize>,
    frame: usize,
    silent_frames: usize,
    flux_mean: f32,
    previous_flux: f32,
    older_flux: f32,
    last_beat: Option<usize>,
    consistent_estimates: usize,
    reading: TempoReading,
}

impl TempoTracker {
    pub(super) fn new(sample_rate: u32) -> Self {
        let sample_rate = sample_rate.max(1);
        let hop_samples = (sample_rate as f32 * HOP_SECONDS).round().max(1.0) as usize;
        let size = (sample_rate as usize / 24)
            .next_power_of_two()
            .clamp(256, 4096);
        let fft = FftPlanner::<f32>::new().plan_fft_forward(size);
        let scratch = vec![Complex::default(); fft.get_inplace_scratch_len()];
        Self {
            hop_samples,
            hop_seconds: hop_samples as f32 / sample_rate as f32,
            samples: VecDeque::with_capacity(size),
            since_hop: 0,
            fft,
            buffer: vec![Complex::default(); size],
            scratch,
            window: (0..size)
                .map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / (size - 1) as f32).cos())
                .collect(),
            previous_spectrum: vec![0.0; size / 2 + 1],
            envelope: VecDeque::with_capacity(HISTORY_FRAMES),
            onsets: VecDeque::new(),
            frame: 0,
            silent_frames: 0,
            flux_mean: 0.0,
            previous_flux: 0.0,
            older_flux: 0.0,
            last_beat: None,
            consistent_estimates: 0,
            reading: TempoReading::default(),
        }
    }

    pub(super) fn push_sample(&mut self, sample: f32) {
        if self.samples.len() == self.buffer.len() {
            self.samples.pop_front();
        }
        self.samples.push_back(sample);
        self.since_hop += 1;
        if self.since_hop < self.hop_samples {
            return;
        }
        self.since_hop = 0;
        self.analyze_frame();
    }

    pub(super) fn take_reading(&mut self) -> TempoReading {
        let reading = self.reading;
        self.reading.beat = false;
        reading
    }

    fn analyze_frame(&mut self) {
        self.frame += 1;
        let mut energy = 0.0;
        self.buffer.fill(Complex::default());
        let start = self.buffer.len() - self.samples.len();
        for (i, sample) in self.samples.iter().enumerate() {
            energy += sample * sample;
            self.buffer[start + i].re = sample * self.window[start + i];
        }
        let audible = energy / self.samples.len().max(1) as f32 > 0.000_001;
        if audible {
            self.silent_frames = 0;
        } else {
            self.silent_frames += 1;
        }
        self.fft
            .process_with_scratch(&mut self.buffer, &mut self.scratch);
        let scale = 10.0 / self.buffer.len() as f32;
        let mut flux = 0.0;
        for (bin, previous) in self.buffer.iter().zip(&mut self.previous_spectrum) {
            let magnitude = (bin.norm() * scale).ln_1p();
            flux += (magnitude - *previous).max(0.0);
            *previous = magnitude;
        }
        if !audible {
            flux = 0.0;
        }
        self.flux_mean += 0.03 * (flux - self.flux_mean);
        let novelty = (flux - self.flux_mean * 1.25).max(0.0);
        if self.envelope.len() == HISTORY_FRAMES {
            self.envelope.pop_front();
        }
        self.envelope.push_back(novelty);

        let onset_frame = self.frame.saturating_sub(1);
        if self.previous_flux > FLUX_FLOOR.max(self.flux_mean * 1.5)
            && self.previous_flux > flux
            && self.previous_flux >= self.older_flux
            && self
                .onsets
                .back()
                .is_none_or(|last| onset_frame - last >= 20)
        {
            self.onsets.push_back(onset_frame);
            if let Some(bpm) = self.reading.bpm {
                let period = 60.0 / bpm / self.hop_seconds;
                if self
                    .last_beat
                    .is_none_or(|last| (onset_frame - last) as f32 >= period * 0.75)
                {
                    self.reading.beat = true;
                    self.last_beat = Some(onset_frame);
                }
            }
        }
        self.older_flux = self.previous_flux;
        self.previous_flux = flux;
        while self
            .onsets
            .front()
            .is_some_and(|first| self.frame - first > HISTORY_FRAMES)
        {
            self.onsets.pop_front();
        }
        // A short quiet space between kicks must not reset a slow tempo.
        if self.silent_frames as f32 * self.hop_seconds >= 1.5 {
            self.clear_estimate();
            self.envelope.clear();
            self.onsets.clear();
        } else if self.frame.is_multiple_of(ESTIMATE_EVERY) {
            self.estimate();
        }
    }

    fn clear_estimate(&mut self) {
        self.reading = TempoReading::default();
        self.consistent_estimates = 0;
        self.last_beat = None;
    }

    fn estimate(&mut self) {
        if self.envelope.len() < MINIMUM_FRAMES || self.onsets.len() < 4 {
            self.clear_estimate();
            return;
        }
        let envelope = self.envelope.make_contiguous();
        let minimum_lag = (60.0 / MAX_BPM / self.hop_seconds).round() as usize;
        let maximum_lag = (60.0 / MIN_BPM / self.hop_seconds).round() as usize;
        let mut correlations = vec![0.0; maximum_lag + 2];
        for (lag, correlation) in correlations.iter_mut().enumerate().skip(minimum_lag) {
            *correlation = autocorrelation(envelope, lag);
        }
        let mut best_lag = minimum_lag;
        let mut best_score = 0.0;
        for lag in minimum_lag..=maximum_lag {
            // Prefer the shorter strong period over its half-time multiple.
            let half_lag = (lag as f32 / 2.0).round() as usize;
            let score = correlations[lag] - 0.3 * correlations[half_lag].max(0.0);
            if score > best_score {
                best_lag = lag;
                best_score = score;
            }
        }
        let confidence = correlations[best_lag].clamp(0.0, 1.0);
        if confidence < 0.35 {
            self.clear_estimate();
            return;
        }
        let left = correlations[best_lag.saturating_sub(1)];
        let center = correlations[best_lag];
        let right = correlations[best_lag + 1];
        let curvature = left - 2.0 * center + right;
        let offset = if curvature.abs() > f32::EPSILON {
            (0.5 * (left - right) / curvature).clamp(-0.5, 0.5)
        } else {
            0.0
        };
        let bpm = (60.0 / ((best_lag as f32 + offset) * self.hop_seconds)).clamp(MIN_BPM, MAX_BPM);
        let consistent = self
            .reading
            .bpm
            .is_some_and(|previous| (bpm - previous).abs() < previous * 0.025);
        self.consistent_estimates = if consistent {
            self.consistent_estimates + 1
        } else {
            1
        };
        self.reading.bpm = Some(bpm);
        self.reading.confidence = confidence;
        self.reading.stable = confidence >= 0.6 && self.consistent_estimates >= 3;

        // Old periodic audio must not keep a new irregular section marked stable.
        if self.reading.stable {
            let recent = &envelope[envelope.len().saturating_sub(250)..];
            if autocorrelation(recent, best_lag) < 0.45 {
                self.reading.stable = false;
                self.consistent_estimates = 0;
            }
        }
    }
}

fn autocorrelation(values: &[f32], lag: usize) -> f32 {
    if lag >= values.len() {
        return 0.0;
    }
    let mean = values.iter().sum::<f32>() / values.len() as f32;
    let mut product = 0.0;
    let mut energy_a = 0.0;
    let mut energy_b = 0.0;
    for (a, b) in values[lag..].iter().zip(values) {
        let a = a - mean;
        let b = b - mean;
        product += a * b;
        energy_a += a * a;
        energy_b += b * b;
    }
    let scale = (energy_a * energy_b).sqrt();
    if scale > 0.000_001 {
        product / scale
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed_clicks(
        tracker: &mut TempoTracker,
        rate: u32,
        bpm: f32,
        seconds: u32,
    ) -> (TempoReading, Vec<f32>) {
        let mut beats = Vec::new();
        let mut reading = TempoReading::default();
        for index in 0..rate * seconds {
            let time = index as f32 / rate as f32;
            let phase = time.rem_euclid(60.0 / bpm);
            let pulse = if phase < 0.12 {
                (-phase * 40.0).exp()
            } else {
                0.0
            };
            tracker.push_sample(pulse * (std::f32::consts::TAU * 180.0 * time).sin() * 0.8);
            if index.is_multiple_of(rate / 40) {
                reading = tracker.take_reading();
                if reading.beat {
                    beats.push(time);
                }
            }
        }
        (reading, beats)
    }

    #[test]
    fn known_pcm_tempos_stabilize_and_flash_once_per_beat_at_common_sample_rates() {
        for (rate, bpm) in [(44_100, 90.0), (48_000, 120.0), (48_000, 150.0)] {
            let mut tracker = TempoTracker::new(rate);
            let (reading, beats) = feed_clicks(&mut tracker, rate, bpm, 12);
            assert!(reading.stable, "{bpm}: {reading:?}");
            assert!(
                (reading.bpm.unwrap() - bpm).abs() < 2.0,
                "{bpm}: {reading:?}"
            );
            assert!(beats.len() >= 6);
            for pair in beats.windows(2).rev().take(5) {
                assert!(
                    ((pair[1] - pair[0]) - 60.0 / bpm).abs() < 0.05,
                    "{bpm}: {beats:?}"
                );
            }
        }
    }

    #[test]
    fn silence_clears_tempo_and_does_not_generate_beats() {
        let mut tracker = TempoTracker::new(8_000);
        assert!(feed_clicks(&mut tracker, 8_000, 120.0, 10).0.stable);
        for _ in 0..16_000 {
            tracker.push_sample(0.0);
        }
        let reading = tracker.take_reading();
        assert!(reading.bpm.is_none());
        assert!(!reading.stable && !reading.beat);
    }

    #[test]
    fn steady_tone_and_irregular_noise_do_not_report_stable_tempo() {
        for noise in [false, true] {
            let mut tracker = TempoTracker::new(8_000);
            let mut random = 7_u32;
            for index in 0..96_000 {
                random = random.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let sample = if noise {
                    (random as f32 / u32::MAX as f32 - 0.5) * 0.4
                } else {
                    (std::f32::consts::TAU * 440.0 * index as f32 / 8_000.0).sin() * 0.5
                };
                tracker.push_sample(sample);
                assert!(!tracker.take_reading().stable);
            }
        }
    }

    #[test]
    fn tempo_change_loses_stability_then_reacquires_the_new_period() {
        let mut tracker = TempoTracker::new(8_000);
        assert!(feed_clicks(&mut tracker, 8_000, 120.0, 10).0.stable);
        let changing = feed_clicks(&mut tracker, 8_000, 150.0, 3).0;
        assert!(!changing.stable, "{changing:?}");
        let settled = feed_clicks(&mut tracker, 8_000, 150.0, 10).0;
        assert!(settled.stable, "{settled:?}");
        assert!((settled.bpm.unwrap() - 150.0).abs() < 2.0);
    }
}
