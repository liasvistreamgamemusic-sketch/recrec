//! Live waveform history of the inputs: min/max per block of frames, kept at several
//! resolutions so both the last seconds and a whole long take can be drawn cheaply.

use crate::peaks::{Pyramid, quantize};

/// Blocks per second of audio (one block ≈ 5 ms).
pub const BLOCKS_PER_SECOND: u32 = 200;

pub fn block_frames(sample_rate: u32) -> usize {
    (sample_rate / BLOCKS_PER_SECOND).max(1) as usize
}

/// Lowest and highest sample of a stretch of audio.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Extent {
    pub min: f32,
    pub max: f32,
}

impl Extent {
    pub fn merge(self, other: Extent) -> Extent {
        Extent {
            min: self.min.min(other.min),
            max: self.max.max(other.max),
        }
    }
}

/// UI-side history of every input channel.
#[derive(Debug)]
pub struct WaveHistory {
    channels: Vec<Pyramid>,
    /// Blocks dropped from the front (index of the oldest block still held).
    first: u64,
    /// Blocks received since the stream opened (index of the next block).
    total: u64,
}

impl WaveHistory {
    pub fn new(channels: usize) -> Self {
        Self {
            channels: vec![Pyramid::default(); channels],
            first: 0,
            total: 0,
        }
    }

    pub fn total_blocks(&self) -> u64 {
        self.total
    }

    /// Reads every complete block (`2 × channels` floats: min, max per channel) from the ring.
    pub fn drain(&mut self, ring: &mut rtrb::Consumer<f32>) {
        let per_block = self.channels.len() * 2;
        if per_block == 0 {
            return;
        }
        let available = ring.slots();
        let n = available - available % per_block;
        let Ok(chunk) = ring.read_chunk(n) else {
            return;
        };
        let (a, b) = chunk.as_slices();
        let mut values = a.iter().chain(b).copied();
        for _ in 0..n / per_block {
            for ch in &mut self.channels {
                // `n` is a multiple of the block size, so both values exist.
                let (Some(min), Some(max)) = (values.next(), values.next()) else {
                    break;
                };
                ch.push(quantize(min, max));
            }
            self.total += 1;
        }
        chunk.commit_all();
    }

    /// Forgets all but the newest `keep` blocks once the history holds twice as many
    /// (called while idle; during a take the whole take is kept).
    pub fn trim(&mut self, keep: usize) {
        let held = (self.total - self.first) as usize;
        if held > keep * 2 {
            let drop = held - keep;
            for ch in &mut self.channels {
                ch.drop_front(drop);
            }
            self.first += drop as u64;
        }
    }

    /// `columns` extents of `channel` for blocks `start .. start + span`, where `start` is
    /// an absolute block index (fractional). Columns without data are `None`.
    pub fn columns(
        &self,
        channel: usize,
        start: f64,
        span: f64,
        columns: usize,
    ) -> Vec<Option<Extent>> {
        match self.channels.get(channel) {
            Some(lane) => lane.columns(start - self.first as f64, span, columns),
            None => vec![None; columns],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn history_with(blocks: &[(f32, f32)]) -> WaveHistory {
        let (mut p, mut c) = rtrb::RingBuffer::new(4096);
        for (min, max) in blocks {
            p.push(*min).unwrap();
            p.push(*max).unwrap();
        }
        let mut h = WaveHistory::new(1);
        h.drain(&mut c);
        h
    }

    fn max_of(e: Option<Extent>) -> i32 {
        (e.unwrap().max * 127.0).round() as i32
    }

    #[test]
    fn block_size_is_about_five_ms() {
        assert_eq!(block_frames(48_000), 240);
        assert_eq!(block_frames(44_100), 220);
    }

    #[test]
    fn drain_reads_whole_blocks_only() {
        let (mut p, mut c) = rtrb::RingBuffer::new(64);
        for v in [-0.1, 0.2, -0.3, 0.4, -0.5] {
            p.push(v).unwrap();
        }
        let mut h = WaveHistory::new(2);
        h.drain(&mut c);
        assert_eq!(h.total_blocks(), 1);
        assert_eq!(c.slots(), 1);
        let col = h.columns(1, 0.0, 1.0, 1)[0].unwrap();
        assert!((col.min + 0.3).abs() < 0.01 && (col.max - 0.4).abs() < 0.01);
    }

    #[test]
    fn columns_address_blocks_by_absolute_index() {
        let h = history_with(&[(0.0, 0.1), (0.0, 0.2), (0.0, 0.3), (0.0, 0.4)]);
        // The newest two blocks, one per column.
        let cols = h.columns(0, 2.0, 2.0, 2);
        assert_eq!((max_of(cols[0]), max_of(cols[1])), (39, 51));
        // A window reaching before the start leaves the oldest columns empty.
        let cols = h.columns(0, -4.0, 8.0, 4);
        assert_eq!(cols[0], None);
        assert_eq!(cols[1], None);
        assert_eq!(max_of(cols[3]), 51);
    }

    #[test]
    fn trimming_keeps_indices_stable() {
        let blocks: Vec<(f32, f32)> = (0..100).map(|i| (0.0, i as f32 / 127.0)).collect();
        let mut h = history_with(&blocks);
        h.trim(60); // 100 held is not yet twice 60
        assert_eq!(max_of(h.columns(0, 0.0, 1.0, 1)[0]), 0);
        h.trim(30);
        assert_eq!(h.total_blocks(), 100);
        // Blocks before index 70 are gone; later ones keep their absolute positions.
        assert_eq!(h.columns(0, 10.0, 1.0, 1)[0], None);
        assert_eq!(max_of(h.columns(0, 70.0, 1.0, 1)[0]), 70);
        assert_eq!(max_of(h.columns(0, 99.0, 1.0, 1)[0]), 99);
    }
}
