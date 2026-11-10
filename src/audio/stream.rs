//! An open input stream together with its meters and writer thread.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, bail};
use cpal::traits::{DeviceTrait, StreamTrait};
use cpal::{BufferSize, HostId, SampleFormat, StreamConfig};
use crossbeam_channel::Receiver;

use super::capture::{Capture, SUPPORTED_FORMATS};
use super::devices;
use crate::meter::MeterBank;
use crate::recorder::{Recorder, RecorderConfig, RecorderEvent};

/// Seconds of audio the capture ring holds before frames are dropped.
const RING_SECONDS: usize = 4;
const STREAM_ERROR_QUEUE: usize = 16;

#[derive(Debug, Clone, PartialEq)]
pub struct SessionInfo {
    pub host: String,
    pub device: String,
    pub channels: u16,
    pub sample_rate: u32,
    pub device_format: SampleFormat,
    pub buffer_size: Option<u32>,
}

pub struct InputSession {
    // Declared first so it is dropped (and its take finalized) before the stream.
    recorder: Recorder,
    _stream: cpal::Stream,
    pub info: SessionInfo,
    pub meters: Arc<MeterBank>,
    dropped_frames: Arc<AtomicU64>,
    stream_errors: Receiver<String>,
}

impl InputSession {
    pub fn open(
        host_id: HostId,
        device_name: &str,
        sample_rate: u32,
        buffer_size: Option<u32>,
    ) -> anyhow::Result<Self> {
        let device = devices::find_input_device(host_id, device_name)?;
        let info = devices::describe(&device)
            .with_context(|| format!("「{device_name}」は入力に使えません"))?;
        if !SUPPORTED_FORMATS.contains(&info.sample_format) {
            bail!("サンプル形式 {} には対応していません", info.sample_format);
        }
        let channels = usize::from(info.channels);
        let config = StreamConfig {
            channels: info.channels,
            sample_rate,
            buffer_size: buffer_size.map_or(BufferSize::Default, BufferSize::Fixed),
        };

        let (producer, consumer) =
            rtrb::RingBuffer::new(sample_rate as usize * channels * RING_SECONDS);
        let meters = Arc::new(MeterBank::new(channels));
        let dropped_frames = Arc::new(AtomicU64::new(0));
        let mut capture = Capture::new(producer, meters.clone(), dropped_frames.clone());
        let (err_tx, err_rx) = crossbeam_channel::bounded(STREAM_ERROR_QUEUE);

        let stream = device
            .build_input_stream_raw(
                config,
                info.sample_format,
                move |data: &cpal::Data, _: &cpal::InputCallbackInfo| capture.push_data(data),
                move |err: cpal::Error| {
                    log::error!("stream error: {err}");
                    if err_tx.try_send(err.to_string()).is_err() {
                        log::warn!("stream error queue full; error not shown in UI");
                    }
                },
                None,
            )
            .with_context(|| {
                format!("「{device_name}」を {sample_rate} Hz / {channels} ch で開けません")
            })?;
        stream.play().context("入力ストリームを開始できません")?;

        let recorder = Recorder::spawn(
            consumer,
            RecorderConfig::new(channels, sample_rate),
            dropped_frames.clone(),
        )
        .context("書き込みスレッドを起動できません")?;

        let info = SessionInfo {
            host: host_id.name().to_owned(),
            device: device_name.to_owned(),
            channels: info.channels,
            sample_rate,
            device_format: info.sample_format,
            buffer_size,
        };
        log::info!("input session opened: {info:?}");
        Ok(Self {
            recorder,
            _stream: stream,
            info,
            meters,
            dropped_frames,
            stream_errors: err_rx,
        })
    }

    pub fn recorder(&self) -> &Recorder {
        &self.recorder
    }

    pub fn dropped_frames(&self) -> u64 {
        self.dropped_frames.load(Ordering::Relaxed)
    }

    pub fn try_stream_error(&self) -> Option<String> {
        self.stream_errors.try_recv().ok()
    }

    /// Finalizes any take and closes the stream. Returns the recorder's last events.
    pub fn close(self) -> Vec<RecorderEvent> {
        let Self {
            recorder, _stream, ..
        } = self;
        let events = recorder.shutdown();
        drop(_stream);
        events
    }
}
