use std::time::{Duration, Instant};

pub const DEMO_SAMPLE_RATE: u32 = 48_000;
const DEMO_SEGMENT: Duration = Duration::from_secs(5);

pub struct DemoInput {
    last_generated_at: Instant,
    sample_index: u64,
    phase: f32,
}

impl DemoInput {
    pub fn new(now: Instant) -> Self {
        Self {
            last_generated_at: now,
            sample_index: 0,
            phase: 0.0,
        }
    }

    pub fn append_samples(&mut self, now: Instant, samples: &mut Vec<f32>, maximum: usize) {
        samples.clear();
        let elapsed = now.saturating_duration_since(self.last_generated_at);
        let requested = (elapsed.as_secs_f64() * f64::from(DEMO_SAMPLE_RATE)).floor() as usize;
        let count = requested.min(maximum);

        for _ in 0..count {
            let (frequency_hz, amplitude) = self.current_signal();
            samples.push(amplitude * self.phase.sin());
            self.phase = (self.phase
                + 2.0 * std::f32::consts::PI * frequency_hz / DEMO_SAMPLE_RATE as f32)
                .rem_euclid(2.0 * std::f32::consts::PI);
            self.sample_index += 1;
        }

        if count > 0 {
            self.last_generated_at +=
                Duration::from_secs_f64(count as f64 / f64::from(DEMO_SAMPLE_RATE));
        }
    }

    fn current_signal(&self) -> (f32, f32) {
        match self.segment_index() {
            0 => (220.0, 0.08),
            1 => (3_500.0, 0.08),
            2 => (220.0, 0.70),
            _ => (3_500.0, 0.70),
        }
    }

    fn segment_index(&self) -> u64 {
        let samples_per_segment = u64::from(DEMO_SAMPLE_RATE) * DEMO_SEGMENT.as_secs();
        (self.sample_index / samples_per_segment) % 4
    }
}
