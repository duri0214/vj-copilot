use std::{
    error::Error,
    fmt,
    sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc::{sync_channel, Receiver, SyncSender},
        Arc, Mutex,
    },
    time::Instant,
};

use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    Device, SampleFormat, SizedSample, Stream, StreamConfig,
};
const MAX_BUFFERED_MILLISECONDS: usize = 250;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AudioBackend {
    #[default]
    System,
    #[cfg(all(windows, feature = "asio"))]
    Asio,
}

impl AudioBackend {
    fn is_asio(self) -> bool {
        match self {
            Self::System => false,
            #[cfg(all(windows, feature = "asio"))]
            Self::Asio => true,
        }
    }
}

#[derive(Default)]
struct CaptureTimingState {
    frames: AtomicUsize,
    interval_us: AtomicUsize,
}

pub struct CaptureTiming {
    pub frames: usize,
    pub buffer_ms: f32,
    pub interval_ms: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AudioSource {
    #[cfg_attr(not(windows), default)]
    LineInput,
    #[cfg(windows)]
    #[default]
    SystemPlayback,
}

impl AudioSource {
    fn devices(
        self,
        host: &cpal::Host,
    ) -> Result<cpal::InputDevices<cpal::Devices>, cpal::DevicesError> {
        match self {
            Self::LineInput => host.input_devices(),
            #[cfg(windows)]
            Self::SystemPlayback => host.output_devices(),
        }
    }

    fn default_device(self, host: &cpal::Host) -> Option<Device> {
        match self {
            Self::LineInput => host.default_input_device(),
            #[cfg(windows)]
            Self::SystemPlayback => host.default_output_device(),
        }
    }
}

#[derive(Clone, Debug)]
pub enum InputStatus {
    Ready,
    Capturing {
        device_name: String,
        sample_rate: u32,
    },
    Stopped,
    NoDevice,
    Unsupported(String),
    Error(String),
}

impl InputStatus {
    pub fn message(&self) -> String {
        match self {
            Self::Ready => "入力デバイスを選択して開始できます".to_owned(),
            Self::Capturing {
                device_name,
                sample_rate,
            } => format!("入力中: {device_name} ({sample_rate} Hz)"),
            Self::Stopped => "入力を停止しています".to_owned(),
            Self::NoDevice => "入力デバイスが見つかりません".to_owned(),
            Self::Unsupported(message) | Self::Error(message) => message.clone(),
        }
    }
}

#[derive(Debug)]
enum AudioInputError {
    Enumerate(cpal::DevicesError),
    SelectedDeviceMissing,
    DeviceName(cpal::DeviceNameError),
    DefaultConfig(cpal::DefaultStreamConfigError),
    BuildStream(cpal::BuildStreamError),
    PlayStream(cpal::PlayStreamError),
    UnsupportedInput(String),
}

impl fmt::Display for AudioInputError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Enumerate(error) => write!(formatter, "入力デバイスを列挙できません: {error}"),
            Self::SelectedDeviceMissing => {
                formatter.write_str("選択した入力デバイスが見つかりません")
            }
            Self::DeviceName(error) => write!(formatter, "入力デバイス名を取得できません: {error}"),
            Self::DefaultConfig(error) => {
                write!(formatter, "既定の入力設定を取得できません: {error}")
            }
            Self::BuildStream(error) => {
                write!(formatter, "入力ストリームを作成できません: {error}")
            }
            Self::PlayStream(error) => write!(formatter, "入力ストリームを開始できません: {error}"),
            Self::UnsupportedInput(message) => formatter.write_str(message),
        }
    }
}

impl Error for AudioInputError {}

pub struct AudioInput {
    host: cpal::Host,
    backend: AudioBackend,
    first_channel: usize,
    source: AudioSource,
    device_names: Vec<String>,
    selected_device: Option<String>,
    stream: Option<Stream>,
    status: InputStatus,
    stream_errors: Arc<Mutex<Option<String>>>,
    dropped_samples: Arc<AtomicUsize>,
    timing: Arc<CaptureTimingState>,
}

pub struct CapturedSamples {
    pub sample_rate: u32,
    pub(crate) samples: Receiver<CapturedSample>,
}

pub(crate) struct CapturedSample {
    pub value: f32,
    pub callback_at: Instant,
    pub sequence: u64,
}

struct CaptureSink {
    sender: SyncSender<CapturedSample>,
    dropped: Arc<AtomicUsize>,
    errors: Arc<Mutex<Option<String>>>,
    timing: Arc<CaptureTimingState>,
}

impl AudioInput {
    pub fn new() -> Self {
        let mut input = Self {
            host: cpal::default_host(),
            backend: AudioBackend::default(),
            first_channel: 0,
            source: AudioSource::default(),
            device_names: Vec::new(),
            selected_device: None,
            stream: None,
            status: InputStatus::Stopped,
            stream_errors: Arc::new(Mutex::new(None)),
            dropped_samples: Arc::new(AtomicUsize::new(0)),
            timing: Arc::new(CaptureTimingState::default()),
        };
        input.refresh_devices();
        input
    }

    pub fn device_names(&self) -> &[String] {
        &self.device_names
    }

    pub fn source(&self) -> AudioSource {
        self.source
    }

    pub fn supports_loopback(&self) -> bool {
        !self.backend.is_asio()
    }

    #[cfg(all(windows, feature = "asio"))]
    pub fn backend(&self) -> AudioBackend {
        self.backend
    }

    #[cfg(all(windows, feature = "asio"))]
    pub fn select_backend(&mut self, backend: AudioBackend) {
        if self.backend == backend {
            return;
        }
        self.stop();
        let host = match backend {
            AudioBackend::System => Ok(cpal::default_host()),
            AudioBackend::Asio => cpal::host_from_id(cpal::HostId::Asio),
        };
        match host {
            Ok(host) => {
                self.host = host;
                self.backend = backend;
                self.source = if backend.is_asio() {
                    AudioSource::LineInput
                } else {
                    AudioSource::default()
                };
                self.selected_device = None;
                self.first_channel = 0;
                self.refresh_devices();
            }
            Err(error) => {
                self.status = InputStatus::Error(format!("ASIO を初期化できません: {error}"))
            }
        }
    }

    #[cfg(all(windows, feature = "asio"))]
    pub fn first_channel(&self) -> usize {
        self.first_channel + 1
    }

    #[cfg(all(windows, feature = "asio"))]
    pub fn select_first_channel(&mut self, channel: usize) {
        self.first_channel = channel.saturating_sub(1);
    }

    pub fn timing(&self) -> Option<CaptureTiming> {
        let InputStatus::Capturing { sample_rate, .. } = self.status else {
            return None;
        };
        let frames = self.timing.frames.load(Ordering::Relaxed);
        (frames > 0).then(|| CaptureTiming {
            frames,
            buffer_ms: frames as f32 / sample_rate as f32 * 1_000.0,
            interval_ms: self.timing.interval_us.load(Ordering::Relaxed) as f32 / 1_000.0,
        })
    }

    pub fn select_source(&mut self, source: AudioSource) {
        #[cfg(windows)]
        if source == AudioSource::SystemPlayback && !self.supports_loopback() {
            return;
        }
        if self.source != source {
            self.stop();
            self.source = source;
            self.selected_device = None;
            self.refresh_devices();
        }
    }

    pub fn selected_device(&self) -> Option<&str> {
        self.selected_device.as_deref()
    }

    pub fn select_device(&mut self, device_name: Option<String>) {
        self.selected_device = device_name;
    }

    pub fn status(&self) -> &InputStatus {
        &self.status
    }

    pub fn dropped_samples(&self) -> usize {
        self.dropped_samples.load(Ordering::Relaxed)
    }

    pub fn is_capturing(&self) -> bool {
        self.stream.is_some()
    }

    pub fn refresh_devices(&mut self) {
        let devices = match self.source.devices(&self.host) {
            Ok(devices) => devices,
            Err(error) => {
                self.device_names.clear();
                self.selected_device = None;
                self.status = InputStatus::Error(format!("入力デバイスを列挙できません: {error}"));
                return;
            }
        };

        let mut device_names: Vec<String> =
            devices.filter_map(|device| device.name().ok()).collect();
        device_names.sort();
        device_names.dedup();

        if device_names.is_empty() {
            self.device_names = device_names;
            self.selected_device = None;
            if !self.is_capturing() {
                self.status = InputStatus::NoDevice;
            }
            return;
        }

        let selected_is_available = self
            .selected_device
            .as_ref()
            .is_some_and(|selected| device_names.contains(selected));
        if !selected_is_available {
            self.selected_device = self
                .source
                .default_device(&self.host)
                .and_then(|device| device.name().ok())
                .filter(|name| device_names.contains(name))
                .or_else(|| device_names.first().cloned());
        }

        self.device_names = device_names;
        if !self.is_capturing() {
            self.status = InputStatus::Ready;
        }
    }

    pub fn start(&mut self) -> Option<CapturedSamples> {
        self.stop();

        match self.try_start() {
            Ok(captured) => Some(captured),
            Err(AudioInputError::UnsupportedInput(message)) => {
                self.status = InputStatus::Unsupported(message);
                None
            }
            Err(error) => {
                self.status = InputStatus::Error(error.to_string());
                None
            }
        }
    }

    pub fn stop(&mut self) {
        let _ = self.stream.take();

        if !matches!(self.status, InputStatus::NoDevice | InputStatus::Error(_)) {
            self.status = InputStatus::Stopped;
        }
    }

    pub fn poll_status(&mut self) {
        self.refresh_stream_error();
    }

    fn try_start(&mut self) -> Result<CapturedSamples, AudioInputError> {
        let selected_device = self
            .selected_device
            .clone()
            .ok_or(AudioInputError::SelectedDeviceMissing)?;
        let device = self.find_device(&selected_device)?;
        let supported_config = match self.source {
            AudioSource::LineInput => device.default_input_config(),
            #[cfg(windows)]
            AudioSource::SystemPlayback => device.default_output_config(),
        }
        .map_err(AudioInputError::DefaultConfig)?;
        let sample_format = supported_config.sample_format();
        let config: StreamConfig = supported_config.config();
        let channels = usize::from(config.channels);

        if channels == 0 || (!self.backend.is_asio() && channels > 2) {
            return Err(AudioInputError::UnsupportedInput(format!(
                "{channels} チャンネル入力は未対応です。mono または stereo を選んでください"
            )));
        }

        if self.first_channel >= channels {
            return Err(AudioInputError::UnsupportedInput(format!(
                "入力 ch {} は範囲外です。この機器の入力は {channels} ch です。",
                self.first_channel + 1
            )));
        }

        if !matches!(
            sample_format,
            SampleFormat::F32
                | SampleFormat::F64
                | SampleFormat::I16
                | SampleFormat::U16
                | SampleFormat::I32
        ) {
            return Err(AudioInputError::UnsupportedInput(format!(
                "{sample_format:?} PCM は未対応です。f32、f64、i16、i32、u16 の入力を選んでください"
            )));
        }

        let capacity = (config.sample_rate.0 as usize)
            .saturating_mul(MAX_BUFFERED_MILLISECONDS)
            .saturating_div(1_000)
            .clamp(1_024, 48_000);
        let (sender, receiver) = sync_channel(capacity);
        self.dropped_samples.store(0, Ordering::Relaxed);
        self.timing.frames.store(0, Ordering::Relaxed);
        self.timing.interval_us.store(0, Ordering::Relaxed);
        if let Ok(mut errors) = self.stream_errors.lock() {
            *errors = None;
        }

        let sink = CaptureSink {
            sender,
            dropped: Arc::clone(&self.dropped_samples),
            errors: Arc::clone(&self.stream_errors),
            timing: Arc::clone(&self.timing),
        };
        let stream = match sample_format {
            SampleFormat::F32 => {
                build_stream::<f32>(&device, &config, self.first_channel, sink, normalize_f32)?
            }
            SampleFormat::I16 => {
                build_stream::<i16>(&device, &config, self.first_channel, sink, normalize_i16)?
            }
            SampleFormat::U16 => {
                build_stream::<u16>(&device, &config, self.first_channel, sink, normalize_u16)?
            }
            SampleFormat::I32 => {
                build_stream::<i32>(&device, &config, self.first_channel, sink, normalize_i32)?
            }
            SampleFormat::F64 => {
                build_stream::<f64>(&device, &config, self.first_channel, sink, normalize_f64)?
            }
            _ => {
                return Err(AudioInputError::UnsupportedInput(format!(
                    "{sample_format:?} PCM は未対応です"
                )));
            }
        };

        stream.play().map_err(AudioInputError::PlayStream)?;
        self.stream = Some(stream);
        self.status = InputStatus::Capturing {
            device_name: selected_device,
            sample_rate: config.sample_rate.0,
        };

        Ok(CapturedSamples {
            sample_rate: config.sample_rate.0,
            samples: receiver,
        })
    }

    fn find_device(&self, selected_name: &str) -> Result<Device, AudioInputError> {
        for device in self
            .source
            .devices(&self.host)
            .map_err(AudioInputError::Enumerate)?
        {
            let name = device.name().map_err(AudioInputError::DeviceName)?;
            if name == selected_name {
                return Ok(device);
            }
        }

        Err(AudioInputError::SelectedDeviceMissing)
    }

    fn refresh_stream_error(&mut self) {
        let error = self
            .stream_errors
            .lock()
            .ok()
            .and_then(|mut errors| errors.take());

        if let Some(error) = error {
            let _ = self.stream.take();
            self.status = InputStatus::Error(format!("入力ストリームエラー: {error}"));
        }
    }
}

fn build_stream<T>(
    device: &Device,
    config: &StreamConfig,
    first_channel: usize,
    sink: CaptureSink,
    normalize: fn(T) -> f32,
) -> Result<Stream, AudioInputError>
where
    T: SizedSample + Copy + Send + 'static,
{
    let channels = usize::from(config.channels);
    let mut sequence = 0;
    let mut previous_callback: Option<Instant> = None;
    device
        .build_input_stream(
            config,
            move |data: &[T], _| {
                let callback_at = Instant::now();
                sink.timing
                    .frames
                    .store(data.len() / channels, Ordering::Relaxed);
                if let Some(previous) = previous_callback {
                    sink.timing.interval_us.store(
                        callback_at.saturating_duration_since(previous).as_micros() as usize,
                        Ordering::Relaxed,
                    );
                }
                previous_callback = Some(callback_at);
                for frame in data.chunks_exact(channels) {
                    let value = mono_sample(frame, first_channel, normalize);
                    let sample = CapturedSample {
                        value,
                        callback_at,
                        sequence,
                    };
                    sequence = sequence.wrapping_add(1);
                    if sink.sender.try_send(sample).is_err() {
                        sink.dropped.fetch_add(1, Ordering::Relaxed);
                    }
                }
            },
            move |error| {
                if let Ok(mut errors) = sink.errors.lock() {
                    *errors = Some(error.to_string());
                }
            },
            None,
        )
        .map_err(AudioInputError::BuildStream)
}

fn mono_sample<T: Copy>(frame: &[T], first_channel: usize, normalize: fn(T) -> f32) -> f32 {
    let first = normalize(frame[first_channel]);
    if let Some(second) = frame.get(first_channel + 1) {
        (first + normalize(*second)) * 0.5
    } else {
        first
    }
}

fn normalize_f32(sample: f32) -> f32 {
    if sample.is_finite() {
        sample.clamp(-1.0, 1.0)
    } else {
        0.0
    }
}

fn normalize_i16(sample: i16) -> f32 {
    (sample as f32 / 32_768.0).clamp(-1.0, 1.0)
}

fn normalize_i32(sample: i32) -> f32 {
    (sample as f64 / 2_147_483_648.0) as f32
}

fn normalize_f64(sample: f64) -> f32 {
    if sample.is_finite() {
        sample.clamp(-1.0, 1.0) as f32
    } else {
        0.0
    }
}

fn normalize_u16(sample: u16) -> f32 {
    (sample as f32 / 32_767.5 - 1.0).clamp(-1.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stereo_capture_averages_channels_before_analysis() {
        assert!((mono_sample(&[0.8, 0.2], 0, normalize_f32) - 0.5).abs() < 0.0001);
        assert!((mono_sample(&[-0.6, 0.2], 0, normalize_f32) + 0.2).abs() < 0.0001);
    }

    #[test]
    fn multichannel_capture_reads_the_selected_pair_or_last_mono_channel() {
        let frame = [1.0, 1.0, 0.2, 0.6, -0.5];
        assert!((mono_sample(&frame, 2, normalize_f32) - 0.4).abs() < 0.0001);
        assert_eq!(mono_sample(&frame, 4, normalize_f32), -0.5);
    }

    #[test]
    fn capture_normalizes_integer_pcm_and_rejects_non_finite_samples() {
        assert_eq!(normalize_i16(i16::MIN), -1.0);
        assert_eq!(normalize_u16(0), -1.0);
        assert_eq!(normalize_u16(u16::MAX), 1.0);
        assert_eq!(normalize_f32(f32::NAN), 0.0);
        assert_eq!(normalize_f32(f32::INFINITY), 0.0);
        assert_eq!(normalize_i32(i32::MIN), -1.0);
        assert_eq!(normalize_i32(i32::MAX), 1.0);
        assert_eq!(normalize_f64(f64::NAN), 0.0);
        assert_eq!(normalize_f64(f64::INFINITY), 0.0);
        assert_eq!(normalize_f64(2.0), 1.0);
    }
}
