use std::{
    error::Error,
    fmt,
    sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc::{sync_channel, Receiver, SyncSender},
        Arc, Mutex,
    },
};

use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    Device, SampleFormat, SizedSample, Stream, StreamConfig,
};
const MAX_BUFFERED_SECONDS: usize = 2;

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
    source: AudioSource,
    device_names: Vec<String>,
    selected_device: Option<String>,
    stream: Option<Stream>,
    status: InputStatus,
    stream_errors: Arc<Mutex<Option<String>>>,
    dropped_samples: Arc<AtomicUsize>,
}

pub struct CapturedSamples {
    pub sample_rate: u32,
    pub(crate) samples: Receiver<f32>,
}

impl AudioInput {
    pub fn new() -> Self {
        let mut input = Self {
            host: cpal::default_host(),
            source: AudioSource::default(),
            device_names: Vec::new(),
            selected_device: None,
            stream: None,
            status: InputStatus::Stopped,
            stream_errors: Arc::new(Mutex::new(None)),
            dropped_samples: Arc::new(AtomicUsize::new(0)),
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

    pub fn select_source(&mut self, source: AudioSource) {
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

        if !(1..=2).contains(&channels) {
            return Err(AudioInputError::UnsupportedInput(format!(
                "{channels} チャンネル入力は未対応です。mono または stereo を選んでください"
            )));
        }

        if !matches!(
            sample_format,
            SampleFormat::F32 | SampleFormat::I16 | SampleFormat::U16
        ) {
            return Err(AudioInputError::UnsupportedInput(format!(
                "{sample_format:?} PCM は未対応です。f32、i16、u16 の入力を選んでください"
            )));
        }

        let capacity = (config.sample_rate.0 as usize)
            .saturating_mul(MAX_BUFFERED_SECONDS)
            .clamp(1_024, 192_000);
        let (sender, receiver) = sync_channel(capacity);
        self.dropped_samples.store(0, Ordering::Relaxed);
        if let Ok(mut errors) = self.stream_errors.lock() {
            *errors = None;
        }

        let stream = match sample_format {
            SampleFormat::F32 => build_stream::<f32>(
                &device,
                &config,
                channels,
                sender,
                Arc::clone(&self.dropped_samples),
                Arc::clone(&self.stream_errors),
                normalize_f32,
            )?,
            SampleFormat::I16 => build_stream::<i16>(
                &device,
                &config,
                channels,
                sender,
                Arc::clone(&self.dropped_samples),
                Arc::clone(&self.stream_errors),
                normalize_i16,
            )?,
            SampleFormat::U16 => build_stream::<u16>(
                &device,
                &config,
                channels,
                sender,
                Arc::clone(&self.dropped_samples),
                Arc::clone(&self.stream_errors),
                normalize_u16,
            )?,
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
    channels: usize,
    sender: SyncSender<f32>,
    dropped_samples: Arc<AtomicUsize>,
    stream_errors: Arc<Mutex<Option<String>>>,
    normalize: fn(T) -> f32,
) -> Result<Stream, AudioInputError>
where
    T: SizedSample + Copy + Send + 'static,
{
    device
        .build_input_stream(
            config,
            move |data: &[T], _| {
                enqueue_mono_samples(data, channels, &sender, &dropped_samples, normalize);
            },
            move |error| {
                if let Ok(mut errors) = stream_errors.lock() {
                    *errors = Some(error.to_string());
                }
            },
            None,
        )
        .map_err(AudioInputError::BuildStream)
}

fn enqueue_mono_samples<T: Copy>(
    data: &[T],
    channels: usize,
    sender: &SyncSender<f32>,
    dropped_samples: &AtomicUsize,
    normalize: fn(T) -> f32,
) {
    for frame in data.chunks_exact(channels) {
        let mono = match channels {
            1 => normalize(frame[0]),
            2 => (normalize(frame[0]) + normalize(frame[1])) * 0.5,
            _ => return,
        };

        if sender.try_send(mono).is_err() {
            dropped_samples.fetch_add(1, Ordering::Relaxed);
        }
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

fn normalize_u16(sample: u16) -> f32 {
    (sample as f32 / 32_767.5 - 1.0).clamp(-1.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stereo_capture_averages_channels_before_analysis() {
        let (sender, receiver) = sync_channel(4);
        let dropped = AtomicUsize::new(0);
        enqueue_mono_samples(&[0.8, 0.2, -0.6, 0.2], 2, &sender, &dropped, normalize_f32);
        let samples: Vec<_> = receiver.try_iter().collect();
        assert_eq!(samples.len(), 2);
        assert!((samples[0] - 0.5).abs() < 0.0001);
        assert!((samples[1] + 0.2).abs() < 0.0001);
        assert_eq!(dropped.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn full_capture_buffer_drops_samples_without_blocking() {
        let (sender, receiver) = sync_channel(1);
        let dropped = AtomicUsize::new(0);
        enqueue_mono_samples(&[0.25, 0.5, 0.75], 1, &sender, &dropped, normalize_f32);
        assert_eq!(receiver.try_recv().unwrap(), 0.25);
        assert_eq!(dropped.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn capture_normalizes_integer_pcm_and_rejects_non_finite_samples() {
        assert_eq!(normalize_i16(i16::MIN), -1.0);
        assert_eq!(normalize_u16(0), -1.0);
        assert_eq!(normalize_u16(u16::MAX), 1.0);
        assert_eq!(normalize_f32(f32::NAN), 0.0);
        assert_eq!(normalize_f32(f32::INFINITY), 0.0);
    }
}
