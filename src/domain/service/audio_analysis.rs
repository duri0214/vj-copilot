use std::{collections::VecDeque, sync::Arc};

use rustfft::{num_complex::Complex, Fft, FftPlanner};

use crate::domain::valueobject::{
    AnalysisReading, AudioLevels, FeatureVector, TempoReading, SILENCE_DBFS,
};

use super::tempo::TempoTracker;

pub const ANALYSIS_INTERVAL_MS: u32 = 200;
pub const METER_INTERVAL_MS: u32 = 25;
pub const FFT_SIZE: usize = 2048;
const SEARCH_WINDOW_SEGMENTS: usize = 5;
const BRIGHTNESS_REFERENCE_HZ: f32 = 8_000.0;

#[derive(Clone, Copy, Debug)]
pub struct AnalysisTick {
    pub levels: AudioLevels,
    pub tempo: TempoReading,
    pub reading: Option<AnalysisReading>,
    pub search_features: Option<FeatureVector>,
}

pub struct AudioFeatureTracker {
    sample_rate: u32,
    interval_samples: usize,
    interval: Vec<f32>,
    spectral_history: VecDeque<f32>,
    recent_readings: VecDeque<AnalysisReading>,
    fft: Arc<dyn Fft<f32>>,
    fft_buffer: Vec<Complex<f32>>,
    meter_interval_samples: usize,
    meter_interval: Vec<f32>,
    pending_reading: Option<AnalysisReading>,
    tempo: TempoTracker,
}

impl AudioFeatureTracker {
    pub fn new(sample_rate: u32) -> Self {
        let sample_rate = sample_rate.max(1);
        let interval_samples =
            ((f64::from(sample_rate) * f64::from(ANALYSIS_INTERVAL_MS) / 1_000.0).round() as usize)
                .max(1);
        let mut planner = FftPlanner::<f32>::new();
        let meter_interval_samples =
            ((sample_rate as f64 * METER_INTERVAL_MS as f64 / 1_000.0).round() as usize).max(1);

        Self {
            sample_rate,
            interval_samples,
            interval: Vec::with_capacity(interval_samples),
            spectral_history: VecDeque::with_capacity(FFT_SIZE),
            recent_readings: VecDeque::with_capacity(SEARCH_WINDOW_SEGMENTS),
            fft: planner.plan_fft_forward(FFT_SIZE),
            fft_buffer: vec![Complex::new(0.0, 0.0); FFT_SIZE],
            meter_interval_samples,
            meter_interval: Vec::with_capacity(meter_interval_samples),
            pending_reading: None,
            tempo: TempoTracker::new(sample_rate),
        }
    }

    pub fn push_sample(&mut self, sample: f32) -> Option<AnalysisTick> {
        let sample = sanitize_sample(sample);
        self.interval.push(sample);
        self.meter_interval.push(sample);
        self.tempo.push_sample(sample);

        if self.spectral_history.len() == FFT_SIZE {
            let _ = self.spectral_history.pop_front();
        }
        self.spectral_history.push_back(sample);

        if self.interval.len() >= self.interval_samples {
            let reading = self.measure_interval();
            self.interval.clear();
            if self.recent_readings.len() == SEARCH_WINDOW_SEGMENTS {
                let _ = self.recent_readings.pop_front();
            }
            self.recent_readings.push_back(reading);
            self.pending_reading = Some(reading);
        }

        if self.meter_interval.len() < self.meter_interval_samples {
            return None;
        }
        let levels = AudioLevels {
            rms_dbfs: rms_dbfs(&self.meter_interval),
            peak_dbfs: peak_dbfs(&self.meter_interval),
        };
        self.meter_interval.clear();
        let reading = self.pending_reading.take();
        let search_features = reading.and_then(|_| self.one_second_features());
        Some(AnalysisTick {
            levels,
            tempo: self.tempo.take_reading(),
            reading,
            search_features,
        })
    }

    fn measure_interval(&mut self) -> AnalysisReading {
        let rms_dbfs = rms_dbfs(&self.interval);
        let centroid_hz = self.centroid_from_history();
        let features = FeatureVector::from_clamped(
            ((rms_dbfs + 60.0) / 60.0).clamp(0.0, 1.0),
            (centroid_hz / BRIGHTNESS_REFERENCE_HZ).clamp(0.0, 1.0),
        );

        AnalysisReading {
            features,
            centroid_hz,
            audible: rms_dbfs > SILENCE_DBFS,
        }
    }

    fn centroid_from_history(&mut self) -> f32 {
        fill_hann_buffer(
            self.spectral_history.make_contiguous(),
            &mut self.fft_buffer,
        );
        self.fft.process(&mut self.fft_buffer);
        centroid_from_spectrum(&self.fft_buffer, self.sample_rate)
    }

    fn one_second_features(&self) -> Option<FeatureVector> {
        if self.recent_readings.len() != SEARCH_WINDOW_SEGMENTS
            || self.recent_readings.iter().any(|reading| !reading.audible)
        {
            return None;
        }

        let count = self.recent_readings.len() as f32;
        let energy = self
            .recent_readings
            .iter()
            .map(|reading| reading.features.energy())
            .sum::<f32>()
            / count;
        let brightness = self
            .recent_readings
            .iter()
            .map(|reading| reading.features.brightness())
            .sum::<f32>()
            / count;

        Some(FeatureVector::from_clamped(energy, brightness))
    }
}

fn peak_dbfs(samples: &[f32]) -> f32 {
    let peak = samples
        .iter()
        .map(|sample| sanitize_sample(*sample).abs())
        .fold(0.0_f32, f32::max);
    if peak <= f32::EPSILON {
        -120.0
    } else {
        20.0 * peak.log10()
    }
}

pub fn rms_dbfs(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return -120.0;
    }

    let mean_square = samples
        .iter()
        .map(|sample| {
            let sample = sanitize_sample(*sample);
            sample * sample
        })
        .sum::<f32>()
        / samples.len() as f32;
    let rms = mean_square.sqrt();

    if rms <= f32::EPSILON {
        -120.0
    } else {
        20.0 * rms.log10()
    }
}

#[cfg(test)]
pub fn spectral_centroid_hz(samples: &[f32], sample_rate: u32) -> f32 {
    if sample_rate == 0 {
        return 0.0;
    }

    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(FFT_SIZE);
    let mut buffer = vec![Complex::new(0.0, 0.0); FFT_SIZE];
    fill_hann_buffer(samples, &mut buffer);
    fft.process(&mut buffer);

    centroid_from_spectrum(&buffer, sample_rate)
}

fn fill_hann_buffer(samples: &[f32], buffer: &mut [Complex<f32>]) {
    buffer.fill(Complex::new(0.0, 0.0));

    let sample_count = samples.len().min(buffer.len());
    let source_start = samples.len().saturating_sub(sample_count);
    let destination_start = buffer.len().saturating_sub(sample_count);
    let denominator = buffer.len().saturating_sub(1).max(1) as f32;

    for (offset, sample) in samples[source_start..].iter().enumerate() {
        let index = destination_start + offset;
        let window = 0.5 - 0.5 * (2.0 * std::f32::consts::PI * index as f32 / denominator).cos();
        buffer[index] = Complex::new(sanitize_sample(*sample) * window, 0.0);
    }
}

fn centroid_from_spectrum(spectrum: &[Complex<f32>], sample_rate: u32) -> f32 {
    let mut weighted_frequency = 0.0;
    let mut amplitude_sum = 0.0;
    let highest_bin = spectrum.len() / 2;

    for (index, bin) in spectrum.iter().take(highest_bin + 1).enumerate() {
        let amplitude = bin.norm();
        let frequency = index as f32 * sample_rate as f32 / spectrum.len() as f32;
        weighted_frequency += frequency * amplitude;
        amplitude_sum += amplitude;
    }

    if amplitude_sum <= f32::EPSILON || !amplitude_sum.is_finite() {
        0.0
    } else {
        weighted_frequency / amplitude_sum
    }
}

fn sanitize_sample(sample: f32) -> f32 {
    if sample.is_finite() {
        sample.clamp(-1.0, 1.0)
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calculates_rms_in_dbfs_for_a_normalized_signal() {
        let dbfs = rms_dbfs(&[0.5; 200]);

        assert!((dbfs + 6.0206).abs() < 0.01);
    }

    #[test]
    fn maps_rms_to_the_specified_energy_range() {
        let mut tracker = AudioFeatureTracker::new(1_000);
        let mut tick = None;

        for _ in 0..200 {
            tick = tracker.push_sample(0.5);
        }

        let energy = tick
            .expect("one analysis interval should complete")
            .reading
            .expect("feature interval")
            .features
            .energy();
        assert!((energy - 0.8997).abs() < 0.01);
    }

    #[test]
    fn meter_preserves_a_short_full_scale_peak_despite_low_average_level() {
        let mut tracker = AudioFeatureTracker::new(1_000);
        tracker.push_sample(-1.0);
        let mut tick = None;
        for _ in 1..25 {
            tick = tracker.push_sample(0.0);
        }
        let reading = tick.expect("one meter interval").levels;
        assert!(reading.peak_dbfs.abs() < 0.001);
        assert!(reading.rms_dbfs < -10.0);
    }

    #[test]
    fn silent_input_has_finite_levels_below_the_meter_floor() {
        let mut tracker = AudioFeatureTracker::new(1_000);
        let mut tick = None;
        for _ in 0..200 {
            tick = tracker.push_sample(0.0);
        }
        let reading = tick.expect("one meter interval").levels;
        assert!(reading.rms_dbfs.is_finite() && reading.rms_dbfs < -60.0);
        assert!(reading.peak_dbfs.is_finite() && reading.peak_dbfs < -60.0);
    }

    #[test]
    fn calculates_centroid_for_a_bin_aligned_sine_wave() {
        let bin = 40.0;
        let samples: Vec<f32> = (0..FFT_SIZE)
            .map(|index| (2.0 * std::f32::consts::PI * bin * index as f32 / FFT_SIZE as f32).sin())
            .collect();

        let centroid = spectral_centroid_hz(&samples, 48_000);

        assert!((900.0..=975.0).contains(&centroid));
    }

    #[test]
    fn keeps_search_features_empty_for_silence() {
        let mut tracker = AudioFeatureTracker::new(1_000);
        let mut last_tick = None;

        for _ in 0..1_000 {
            if let Some(tick) = tracker.push_sample(0.0) {
                last_tick = Some(tick);
            }
        }

        let tick = last_tick.expect("five analysis intervals should complete");
        assert!(!tick.reading.unwrap().audible);
        assert!(tick.search_features.is_none());
    }

    #[test]
    fn emits_one_second_average_after_five_audible_intervals() {
        let mut tracker = AudioFeatureTracker::new(1_000);
        let mut last_tick = None;

        for _ in 0..1_000 {
            if let Some(tick) = tracker.push_sample(0.5) {
                last_tick = Some(tick);
            }
        }

        let tick = last_tick.expect("five analysis intervals should complete");
        assert!(tick.reading.unwrap().audible);
        assert!(tick.search_features.is_some());
    }

    #[test]
    fn meter_updates_every_25ms_without_shortening_the_search_window() {
        for rate in [44_100, 48_000] {
            let mut tracker = AudioFeatureTracker::new(rate);
            let mut previous = 0;
            let mut updates = 0;
            let mut readings = 0;
            for index in 1..=rate {
                if let Some(tick) = tracker.push_sample(0.25) {
                    let milliseconds = (index - previous) as f32 / rate as f32 * 1_000.0;
                    assert!((24.9..25.1).contains(&milliseconds));
                    assert!((tick.levels.rms_dbfs + 12.0412).abs() < 0.01);
                    assert!(tick.search_features.is_none() || index >= rate);
                    readings += usize::from(tick.reading.is_some());
                    previous = index;
                    updates += 1;
                }
            }
            assert!((39..=40).contains(&updates));
            assert!((4..=5).contains(&readings));
        }
    }
}
