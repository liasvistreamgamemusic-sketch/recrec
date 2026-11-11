//! Audio-callback side of the capture path. Real-time safe: no allocation, locking or I/O.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use cpal::{FromSample, I24, Sample, SampleFormat, SizedSample};

use crate::meter::MeterBank;
use crate::transport::Transport;
use crate::waveform::block_frames;

/// Device sample formats the capture path can convert.
pub const SUPPORTED_FORMATS: [SampleFormat; 6] = [
    SampleFormat::I16,
    SampleFormat::I24,
    SampleFormat::I32,
    SampleFormat::F32,
    SampleFormat::F64,
    SampleFormat::U16,
];

/// Converts device buffers to interleaved f32, pushes whole frames into the ring and meters them.
pub struct Capture {
    producer: rtrb::Producer<f32>,
    channels: usize,
    meters: Arc<MeterBank>,
    overrun_frames: Arc<AtomicU64>,
    block_peaks: Vec<f32>,
    taps: Option<Taps>,
}

/// Side outputs of the capture path. They never affect what is recorded: when one of their
/// rings is full the data is simply dropped.
pub struct Taps {
    transport: Arc<Transport>,
    /// Interleaved copy of the input for software monitoring.
    monitor: rtrb::Producer<f32>,
    /// Per-block `min, max` of every channel for the waveform display.
    wave: rtrb::Producer<f32>,
    wave_block: usize,
    wave_fill: usize,
    wave_min: Vec<f32>,
    wave_max: Vec<f32>,
}

impl Taps {
    pub fn new(
        transport: Arc<Transport>,
        monitor: rtrb::Producer<f32>,
        wave: rtrb::Producer<f32>,
        sample_rate: u32,
    ) -> Self {
        let channels = transport.in_channels();
        Self {
            transport,
            monitor,
            wave,
            wave_block: block_frames(sample_rate),
            wave_fill: 0,
            wave_min: vec![0.0; channels],
            wave_max: vec![0.0; channels],
        }
    }

    fn feed<T>(&mut self, samples: &[T], channels: usize, recorded_frames: usize)
    where
        T: Sample,
        f32: FromSample<T>,
    {
        self.transport.add_in_frames(recorded_frames as u64);

        let fit = (self.monitor.slots() / channels).min(samples.len() / channels);
        if fit > 0
            && let Ok(chunk) = self.monitor.write_chunk_uninit(fit * channels)
        {
            chunk.fill_from_iter(
                samples[..fit * channels]
                    .iter()
                    .map(|s| f32::from_sample(*s)),
            );
        }

        for frame in samples.chunks_exact(channels) {
            if self.wave_fill == 0 {
                self.wave_min.fill(f32::INFINITY);
                self.wave_max.fill(f32::NEG_INFINITY);
            }
            for ((min, max), s) in self.wave_min.iter_mut().zip(&mut self.wave_max).zip(frame) {
                let v = f32::from_sample(*s);
                *min = min.min(v);
                *max = max.max(v);
            }
            self.wave_fill += 1;
            if self.wave_fill == self.wave_block {
                self.wave_fill = 0;
                if let Ok(chunk) = self.wave.write_chunk_uninit(channels * 2) {
                    chunk.fill_from_iter(
                        self.wave_min
                            .iter()
                            .zip(&self.wave_max)
                            .flat_map(|(min, max)| [*min, *max]),
                    );
                }
            }
        }
    }
}

impl Capture {
    pub fn new(
        producer: rtrb::Producer<f32>,
        meters: Arc<MeterBank>,
        overrun_frames: Arc<AtomicU64>,
    ) -> Self {
        let channels = meters.channels();
        Self {
            producer,
            channels,
            meters,
            overrun_frames,
            block_peaks: vec![0.0; channels],
            taps: None,
        }
    }

    /// Adds the monitor and waveform outputs.
    pub fn with_taps(mut self, taps: Taps) -> Self {
        self.taps = Some(taps);
        self
    }

    /// Handles one callback buffer of any supported format.
    pub fn push_data(&mut self, data: &cpal::Data) {
        match data.sample_format() {
            SampleFormat::I16 => self.push_typed::<i16>(data),
            SampleFormat::I24 => self.push_typed::<I24>(data),
            SampleFormat::I32 => self.push_typed::<i32>(data),
            SampleFormat::F32 => self.push_typed::<f32>(data),
            SampleFormat::F64 => self.push_typed::<f64>(data),
            SampleFormat::U16 => self.push_typed::<u16>(data),
            // Excluded when the stream is built (see SUPPORTED_FORMATS).
            _ => {}
        }
    }

    fn push_typed<T>(&mut self, data: &cpal::Data)
    where
        T: SizedSample,
        f32: FromSample<T>,
    {
        if let Some(samples) = data.as_slice::<T>() {
            self.push(samples);
        }
    }

    /// Pushes interleaved samples. Frames that do not fit are dropped whole and counted,
    /// so channel order in the ring never shifts.
    pub fn push<T>(&mut self, samples: &[T])
    where
        T: Sample,
        f32: FromSample<T>,
    {
        let channels = self.channels;
        let frames = samples.len() / channels;

        self.block_peaks.fill(0.0);
        for frame in samples.chunks_exact(channels) {
            for (peak, s) in self.block_peaks.iter_mut().zip(frame) {
                *peak = peak.max(f32::from_sample(*s).abs());
            }
        }
        for (ch, peak) in self.block_peaks.iter().enumerate() {
            self.meters.record(ch, *peak);
        }

        let fit = (self.producer.slots() / channels).min(frames);
        if fit < frames {
            self.overrun_frames
                .fetch_add((frames - fit) as u64, Ordering::Relaxed);
        }
        if fit > 0
            && let Ok(chunk) = self.producer.write_chunk_uninit(fit * channels)
        {
            chunk.fill_from_iter(
                samples[..fit * channels]
                    .iter()
                    .map(|s| f32::from_sample(*s)),
            );
        }
        if let Some(taps) = &mut self.taps {
            taps.feed(samples, channels, fit);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn taps_copy_input_and_summarize_blocks_without_touching_the_recording() {
        let (p, mut rec) = rtrb::RingBuffer::new(4096);
        let (mon_p, mut mon) = rtrb::RingBuffer::new(4096);
        let (wave_p, mut wave) = rtrb::RingBuffer::new(64);
        let meters = Arc::new(MeterBank::new(2));
        let transport = Arc::new(Transport::new(2));
        // 400 Hz "sample rate" gives 2-frame waveform blocks.
        let taps = Taps::new(transport.clone(), mon_p, wave_p, 400);
        let mut cap = Capture::new(p, meters, Arc::new(AtomicU64::new(0))).with_taps(taps);
        let input = [0.1f32, -0.1, 0.3, -0.5, 0.2, 0.0];
        cap.push(&input);

        assert_eq!(transport.in_frames(), 3);
        assert_eq!(drain(&mut rec), input);
        assert_eq!(drain(&mut mon), input);
        // One complete block (frames 0-1): ch0 min/max, ch1 min/max.
        assert_eq!(drain(&mut wave), vec![0.1, 0.3, -0.5, -0.1]);
    }

    #[test]
    fn full_tap_rings_do_not_lose_recorded_frames() {
        let (p, mut rec) = rtrb::RingBuffer::new(64);
        let (mon_p, _mon) = rtrb::RingBuffer::new(1);
        let (wave_p, _wave) = rtrb::RingBuffer::new(1);
        let transport = Arc::new(Transport::new(1));
        let overrun = Arc::new(AtomicU64::new(0));
        let taps = Taps::new(transport, mon_p, wave_p, 200);
        let mut cap = Capture::new(p, Arc::new(MeterBank::new(1)), overrun.clone()).with_taps(taps);
        cap.push(&[0.1f32, 0.2, 0.3, 0.4]);
        assert_eq!(drain(&mut rec), vec![0.1, 0.2, 0.3, 0.4]);
        assert_eq!(overrun.load(Ordering::Relaxed), 0);
    }

    fn capture(
        channels: usize,
        capacity: usize,
    ) -> (Capture, rtrb::Consumer<f32>, Arc<MeterBank>, Arc<AtomicU64>) {
        let (p, c) = rtrb::RingBuffer::new(capacity);
        let meters = Arc::new(MeterBank::new(channels));
        let overrun = Arc::new(AtomicU64::new(0));
        (
            Capture::new(p, meters.clone(), overrun.clone()),
            c,
            meters,
            overrun,
        )
    }

    fn drain(c: &mut rtrb::Consumer<f32>) -> Vec<f32> {
        let n = c.slots();
        let chunk = c.read_chunk(n).unwrap();
        let v: Vec<f32> = chunk.into_iter().collect();
        v
    }

    #[test]
    fn converts_integer_formats_exactly() {
        let (mut cap, mut c, _, _) = capture(2, 64);
        cap.push(&[i16::MIN, 16384]);
        cap.push(&[I24::new(-8_388_608).unwrap(), I24::new(4_194_304).unwrap()]);
        cap.push(&[i32::MIN, 1 << 30]);
        assert_eq!(drain(&mut c), vec![-1.0, 0.5, -1.0, 0.5, -1.0, 0.5]);
    }

    #[test]
    fn i24_values_survive_conversion_bit_exactly() {
        let (mut cap, mut c, _, _) = capture(1, 64);
        let raw = [1, -1, 8_388_607, -8_388_608, 1_234_567];
        let input: Vec<I24> = raw.iter().map(|&v| I24::new(v).unwrap()).collect();
        cap.push(&input);
        let back: Vec<i32> = drain(&mut c)
            .iter()
            .map(|x| (x * 8_388_608.0).round() as i32)
            .collect();
        assert_eq!(back, raw);
    }

    #[test]
    fn drops_whole_frames_when_ring_is_full() {
        let (mut cap, mut c, _, overrun) = capture(2, 5);
        cap.push(&[0.1f32, 0.2, 0.3, 0.4, 0.5, 0.6]);
        // 5 slots fit only 2 frames of 2 channels.
        assert_eq!(drain(&mut c), vec![0.1, 0.2, 0.3, 0.4]);
        assert_eq!(overrun.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn meters_see_every_frame_even_when_dropped() {
        let (mut cap, _c, meters, _) = capture(2, 2);
        cap.push(&[0.1f32, -0.2, -0.9, 0.3]);
        assert_eq!(meters.take_peak(0), 0.9);
        assert_eq!(meters.take_peak(1), 0.3);
    }
}
