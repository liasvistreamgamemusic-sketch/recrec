//! The output side: monitor mix of the inputs plus the metronome, rendered in the callback.

use std::sync::Arc;

use anyhow::{Context, anyhow, bail};
use cpal::traits::{DeviceTrait, HostTrait};
use cpal::{BufferSize, FromSample, HostId, Sample, SampleFormat, SizedSample, StreamConfig};

use crate::metronome;
use crate::transport::Transport;

/// Time constant of gain changes: long enough to hide clicks, short enough to feel instant.
const SMOOTHING_SECONDS: f32 = 0.005;
/// Monitor backlog (in frames) above which stale input is skipped to keep latency low.
const MAX_BACKLOG_FRAMES: usize = 2048;

/// Mixes monitored inputs and the click into stereo. Real-time safe.
pub struct OutputMixer {
    monitor: rtrb::Consumer<f32>,
    transport: Arc<Transport>,
    sample_rate: u32,
    in_channels: usize,
    /// Smoothed per-channel gains and their targets (left, right).
    gains: Vec<(f32, f32)>,
    targets: Vec<(f32, f32)>,
    click_gain: f32,
    master_gain: f32,
    smoothing: f32,
}

impl OutputMixer {
    pub fn new(monitor: rtrb::Consumer<f32>, transport: Arc<Transport>, sample_rate: u32) -> Self {
        let in_channels = transport.in_channels();
        Self {
            monitor,
            transport,
            sample_rate,
            in_channels,
            gains: vec![(0.0, 0.0); in_channels],
            targets: vec![(0.0, 0.0); in_channels],
            click_gain: 0.0,
            master_gain: 0.0,
            smoothing: 1.0 - (-1.0 / (SMOOTHING_SECONDS * sample_rate as f32)).exp(),
        }
    }

    /// Fills one interleaved output buffer. Channels 1-2 carry the mix; the rest stay silent.
    pub fn render<T>(&mut self, out: &mut [T], out_channels: usize)
    where
        T: Sample + FromSample<f32>,
    {
        let frames = out.len() / out_channels;
        let in_channels = self.in_channels;
        for (target, ch) in self.targets.iter_mut().zip(0..) {
            *target = self.transport.channel_gain(ch);
        }
        let click_target = if self.transport.click_on() {
            self.transport.click_gain()
        } else {
            0.0
        };
        let master_target = self.transport.master_gain();
        let origin = self.transport.origin_out();
        let bpm = self.transport.bpm();
        let beats_per_bar = self.transport.beats_per_bar();
        let beat_unit = self.transport.beat_unit();
        let start = self.transport.out_frames();

        // Skip input that piled up (e.g. after a stall) so monitoring does not lag behind.
        let available = self.monitor.slots() / in_channels;
        if available > frames + MAX_BACKLOG_FRAMES
            && let Ok(stale) = self.monitor.read_chunk((available - frames) * in_channels)
        {
            stale.commit_all();
        }
        let have = (self.monitor.slots() / in_channels).min(frames);
        let chunk = self.monitor.read_chunk(have * in_channels).ok();
        let (a, b) = chunk.as_ref().map_or((&[][..], &[][..]), |c| c.as_slices());
        let mut input = a.iter().chain(b);

        let k = self.smoothing;
        for (i, frame) in out.chunks_exact_mut(out_channels).enumerate() {
            let (mut left, mut right) = (0.0f32, 0.0f32);
            if i < have {
                for (gain, target) in self.gains.iter_mut().zip(&self.targets) {
                    gain.0 += (target.0 - gain.0) * k;
                    gain.1 += (target.1 - gain.1) * k;
                    // `have` whole frames were read, so every channel has a sample.
                    let s = input.next().copied().unwrap_or(0.0);
                    left += s * gain.0;
                    right += s * gain.1;
                }
            }
            self.click_gain += (click_target - self.click_gain) * k;
            self.master_gain += (master_target - self.master_gain) * k;
            if let Some(origin) = origin
                && self.click_gain > 1e-5
            {
                let click = metronome::sample_at(
                    start + i as u64,
                    origin,
                    self.sample_rate,
                    bpm,
                    beats_per_bar,
                    beat_unit,
                ) * self.click_gain;
                left += click;
                right += click;
            }
            left = (left * self.master_gain).clamp(-1.0, 1.0);
            right = (right * self.master_gain).clamp(-1.0, 1.0);
            match frame {
                [mono] => *mono = T::from_sample((left + right) * 0.5),
                [l, r, rest @ ..] => {
                    *l = T::from_sample(left);
                    *r = T::from_sample(right);
                    rest.fill(T::EQUILIBRIUM);
                }
                [] => {}
            }
        }
        drop(input);
        if let Some(chunk) = chunk {
            chunk.commit_all();
        }
        self.transport.add_out_frames(frames as u64);
    }
}

/// Builds the output stream for `mixer` on `device_name` (falling back to the host default).
/// The caller starts it with `play`.
pub fn open(
    host_id: HostId,
    device_name: &str,
    sample_rate: u32,
    buffer_size: Option<u32>,
    mixer: OutputMixer,
) -> anyhow::Result<cpal::Stream> {
    let host = cpal::host_from_id(host_id)
        .with_context(|| format!("{} を初期化できません", host_id.name()))?;
    let device = host
        .output_devices()
        .ok()
        .and_then(|mut devices| devices.find(|d| d.to_string() == device_name))
        .or_else(|| host.default_output_device())
        .ok_or_else(|| anyhow!("出力デバイスが見つかりません"))?;
    let supported = device
        .default_output_config()
        .context("出力デバイスの設定を取得できません")?;
    let channels = supported.channels();
    let config = StreamConfig {
        channels,
        sample_rate,
        buffer_size: buffer_size.map_or(BufferSize::Default, BufferSize::Fixed),
    };
    let stream = match supported.sample_format() {
        SampleFormat::I16 => build::<i16>(&device, config, mixer),
        SampleFormat::I24 => build::<cpal::I24>(&device, config, mixer),
        SampleFormat::I32 => build::<i32>(&device, config, mixer),
        SampleFormat::F32 => build::<f32>(&device, config, mixer),
        other => bail!("出力のサンプル形式 {other} には対応していません"),
    }
    .with_context(|| format!("出力を {sample_rate} Hz / {channels} ch で開けません"))?;
    log::info!("output opened: {device} ({channels} ch, {sample_rate} Hz)");
    Ok(stream)
}

fn build<T>(
    device: &cpal::Device,
    config: StreamConfig,
    mut mixer: OutputMixer,
) -> Result<cpal::Stream, cpal::Error>
where
    T: SizedSample + FromSample<f32>,
{
    let channels = usize::from(config.channels);
    device.build_output_stream(
        config,
        move |out: &mut [T], _: &cpal::OutputCallbackInfo| mixer.render(out, channels),
        |err| log::error!("output stream error: {err}"),
        None,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: u32 = 48_000;

    fn mixer(in_channels: usize) -> (OutputMixer, rtrb::Producer<f32>, Arc<Transport>) {
        let (p, c) = rtrb::RingBuffer::new(1 << 16);
        let transport = Arc::new(Transport::new(in_channels));
        (OutputMixer::new(c, transport.clone(), SR), p, transport)
    }

    /// Renders long enough for the gain smoothing to settle, then returns one more buffer.
    fn settled(m: &mut OutputMixer, p: &mut rtrb::Producer<f32>, frame: &[f32]) -> Vec<f32> {
        let mut out = vec![0.0f32; 2 * 4800];
        for _ in 0..4800 {
            for s in frame {
                p.push(*s).unwrap();
            }
        }
        m.render(&mut out, 2);
        for s in frame {
            p.push(*s).unwrap();
        }
        let mut last = vec![0.0f32; 2];
        m.render(&mut last, 2);
        last
    }

    #[test]
    fn silent_until_a_channel_is_monitored() {
        let (mut m, mut p, _t) = mixer(2);
        let out = settled(&mut m, &mut p, &[0.5, 0.25]);
        assert_eq!(out, vec![0.0, 0.0]);
    }

    #[test]
    fn channel_gains_route_inputs_to_left_and_right() {
        let (mut m, mut p, t) = mixer(2);
        t.set_channel_gains(&[(1.0, 0.0), (0.5, 0.5)]);
        let out = settled(&mut m, &mut p, &[0.4, 0.2]);
        assert!((out[0] - 0.5).abs() < 1e-3, "{out:?}");
        assert!((out[1] - 0.1).abs() < 1e-3, "{out:?}");
    }

    #[test]
    fn gain_changes_fade_instead_of_jumping() {
        let (mut m, mut p, t) = mixer(1);
        t.set_channel_gains(&[(1.0, 1.0)]);
        settled(&mut m, &mut p, &[0.5]);
        t.set_channel_gains(&[(0.0, 0.0)]);
        for _ in 0..3 {
            p.push(0.5).unwrap();
        }
        let mut out = vec![0.0f32; 6];
        m.render(&mut out, 2);
        assert!(out[0] > 0.45 && out[0] < 0.5, "{out:?}");
        assert!(out[4] < out[0]);
    }

    #[test]
    fn missing_input_renders_silence_not_garbage() {
        let (mut m, _p, t) = mixer(1);
        t.set_channel_gains(&[(1.0, 1.0)]);
        let mut out = vec![1.0f32; 8];
        m.render(&mut out, 2);
        assert_eq!(out, vec![0.0; 8]);
        assert_eq!(t.out_frames(), 4);
    }

    #[test]
    fn click_plays_on_the_beat_and_only_when_enabled() {
        let (mut m, _p, t) = mixer(1);
        t.set_tempo(120.0, 4, 4);
        t.set_origin_out(0);
        let mut out = vec![0.0f32; 2 * 48_000];
        m.render(&mut out, 2);
        assert!(out.iter().all(|s| *s == 0.0), "click is off");

        t.set_click(true, 1.0);
        m.render(&mut out, 2);
        // The second buffer starts at frame 48 000 = beat 2; beats are 24 000 frames apart.
        let peak = |from: usize, to: usize| {
            out[from * 2..to * 2]
                .iter()
                .fold(0.0f32, |a, s| a.max(s.abs()))
        };
        assert!(peak(0, 2000) > 0.2);
        assert_eq!(peak(6000, 23_000), 0.0);
        assert!(peak(24_000, 26_000) > 0.2);
        assert_eq!(out[100], out[101], "click is centered");
    }

    #[test]
    fn extra_output_channels_stay_silent_and_mono_is_summed() {
        let (mut m, mut p, t) = mixer(1);
        t.set_channel_gains(&[(1.0, 0.0)]);
        settled(&mut m, &mut p, &[0.8]);
        p.push(0.8).unwrap();
        let mut four = vec![9.0f32; 4];
        m.render(&mut four, 4);
        assert!((four[0] - 0.8).abs() < 1e-3);
        assert_eq!(&four[1..], &[0.0, 0.0, 0.0]);
        p.push(0.8).unwrap();
        let mut mono = vec![9.0f32; 1];
        m.render(&mut mono, 1);
        assert!((mono[0] - 0.4).abs() < 1e-3);
    }

    #[test]
    fn stale_monitor_backlog_is_skipped() {
        let (mut m, mut p, t) = mixer(1);
        t.set_channel_gains(&[(1.0, 1.0)]);
        for _ in 0..(MAX_BACKLOG_FRAMES + 100) {
            p.push(0.0).unwrap();
        }
        p.push(0.5).unwrap();
        // Enough output to settle the gain, ending on the newest sample.
        for _ in 0..10 {
            let mut out = vec![0.0f32; 2];
            m.render(&mut out, 2);
        }
        assert!(p.slots() >= (1 << 16) - 1, "backlog was drained");
    }
}
