//! Audio-callback side of the capture path. Real-time safe: no allocation, locking or I/O.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use cpal::{FromSample, I24, Sample, SampleFormat, SizedSample};

use crate::meter::MeterBank;

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
        }
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
        if fit == 0 {
            return;
        }
        if let Ok(chunk) = self.producer.write_chunk_uninit(fit * channels) {
            chunk.fill_from_iter(
                samples[..fit * channels]
                    .iter()
                    .map(|s| f32::from_sample(*s)),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
