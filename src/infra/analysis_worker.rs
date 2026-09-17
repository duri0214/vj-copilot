use std::{
    sync::mpsc::{sync_channel, Receiver, SyncSender, TryRecvError},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use crate::{
    domain::service::{AnalysisTick, AudioFeatureTracker},
    infra::{
        audio_input::CapturedSamples,
        demo_input::{DemoInput, DEMO_SAMPLE_RATE},
    },
};

const RESULT_BUFFER_CAPACITY: usize = 32;
const DEMO_BATCH_SIZE: usize = 2_400;
pub const INPUT_IDLE_TIMEOUT: Duration = Duration::from_millis(700);

pub struct AnalysisWorker {
    stop_sender: SyncSender<()>,
    ticks: Receiver<AnalysisTick>,
    handle: Option<JoinHandle<()>>,
}

impl AnalysisWorker {
    pub fn start_captured(captured: CapturedSamples) -> Self {
        Self::start(move |stop_receiver, tick_sender| {
            run_captured(captured, stop_receiver, tick_sender);
        })
    }

    pub fn start_demo() -> Self {
        Self::start(run_demo)
    }

    pub fn drain_ticks(&mut self, ticks: &mut Vec<AnalysisTick>) {
        ticks.clear();
        while let Ok(tick) = self.ticks.try_recv() {
            ticks.push(tick);
        }
    }

    pub fn stop(&mut self) {
        let _ = self.stop_sender.try_send(());
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }

    fn start(run: impl FnOnce(Receiver<()>, SyncSender<AnalysisTick>) + Send + 'static) -> Self {
        let (stop_sender, stop_receiver) = sync_channel(1);
        let (tick_sender, ticks) = sync_channel(RESULT_BUFFER_CAPACITY);
        let handle = thread::spawn(move || run(stop_receiver, tick_sender));

        Self {
            stop_sender,
            ticks,
            handle: Some(handle),
        }
    }
}

impl Drop for AnalysisWorker {
    fn drop(&mut self) {
        self.stop();
    }
}

fn run_captured(
    captured: CapturedSamples,
    stop_receiver: Receiver<()>,
    tick_sender: SyncSender<AnalysisTick>,
) {
    let CapturedSamples {
        sample_rate,
        samples,
    } = captured;
    let mut tracker = AudioFeatureTracker::new(sample_rate);
    let mut last_analysis_at = None;

    loop {
        if stop_requested(&stop_receiver) {
            return;
        }

        match samples.recv_timeout(Duration::from_millis(20)) {
            Ok(sample) => {
                if process_sample(sample, &mut tracker, &tick_sender) || last_analysis_at.is_none()
                {
                    last_analysis_at = Some(Instant::now());
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                if last_analysis_at.is_some_and(|last| last.elapsed() >= INPUT_IDLE_TIMEOUT) {
                    // WASAPI loopback can stop sending PCM when playback stops.
                    // Do not combine audio on either side of that gap into one search window.
                    tracker = AudioFeatureTracker::new(sample_rate);
                    last_analysis_at = None;
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
        }
    }
}

fn run_demo(stop_receiver: Receiver<()>, tick_sender: SyncSender<AnalysisTick>) {
    let mut tracker = AudioFeatureTracker::new(DEMO_SAMPLE_RATE);
    let mut generator = DemoInput::new(Instant::now());
    let mut samples = Vec::with_capacity(DEMO_BATCH_SIZE);

    loop {
        if stop_requested(&stop_receiver) {
            return;
        }

        generator.append_samples(Instant::now(), &mut samples, DEMO_BATCH_SIZE);
        if samples.is_empty() {
            thread::sleep(Duration::from_millis(4));
            continue;
        }

        for sample in samples.drain(..) {
            if stop_requested(&stop_receiver) {
                return;
            }
            process_sample(sample, &mut tracker, &tick_sender);
        }
    }
}

fn process_sample(
    sample: f32,
    tracker: &mut AudioFeatureTracker,
    tick_sender: &SyncSender<AnalysisTick>,
) -> bool {
    if let Some(tick) = tracker.push_sample(sample) {
        let _ = tick_sender.try_send(tick);
        true
    } else {
        false
    }
}

fn stop_requested(stop_receiver: &Receiver<()>) -> bool {
    !matches!(stop_receiver.try_recv(), Err(TryRecvError::Empty))
}
