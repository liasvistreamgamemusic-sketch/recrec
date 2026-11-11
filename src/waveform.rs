//! Scrolling waveform overview: min/max pairs per block of input frames.

use std::collections::VecDeque;

/// Blocks per second of audio (one block ≈ 5 ms).
pub const BLOCKS_PER_SECOND: u32 = 200;
const HISTORY_SECONDS: usize = 60;

pub fn block_frames(sample_rate: u32) -> usize {
    (sample_rate / BLOCKS_PER_SECOND).max(1) as usize
}

/// Lowest and highest sample of one block.
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

/// UI-side history of every input channel, newest block last.
#[derive(Debug)]
pub struct WaveHistory {
    channels: Vec<VecDeque<Extent>>,
    capacity: usize,
    /// Blocks received since the stream opened (index of the next block).
    total: u64,
}

impl WaveHistory {
    pub fn new(channels: usize) -> Self {
        let capacity = HISTORY_SECONDS * BLOCKS_PER_SECOND as usize;
        Self {
            channels: (0..channels)
                .map(|_| VecDeque::with_capacity(capacity))
                .collect(),
            capacity,
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
                if ch.len() == self.capacity {
                    ch.pop_front();
                }
                ch.push_back(Extent { min, max });
            }
            self.total += 1;
        }
        chunk.commit_all();
    }

    /// The newest `blocks` blocks of `channel` reduced to `columns` extents (oldest first).
    /// Columns before the start of the history are `None`.
    pub fn columns(&self, channel: usize, blocks: usize, columns: usize) -> Vec<Option<Extent>> {
        let Some(history) = self.channels.get(channel) else {
            return vec![None; columns];
        };
        let len = history.len();
        (0..columns)
            .map(|col| {
                // Block range of this column, counted back from the newest block.
                let from_end_hi = blocks - col * blocks / columns;
                let from_end_lo = blocks - (col + 1) * blocks / columns;
                let lo = len.checked_sub(from_end_hi);
                let hi = len.saturating_sub(from_end_lo).max(lo.map_or(0, |l| l + 1));
                let lo = lo?;
                history
                    .range(lo..hi.min(len))
                    .copied()
                    .reduce(Extent::merge)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn history_with(blocks: &[(f32, f32)]) -> WaveHistory {
        let (mut p, mut c) = rtrb::RingBuffer::new(1024);
        for (min, max) in blocks {
            p.push(*min).unwrap();
            p.push(*max).unwrap();
        }
        let mut h = WaveHistory::new(1);
        h.drain(&mut c);
        h
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
        assert_eq!(
            h.columns(1, 1, 1),
            vec![Some(Extent {
                min: -0.3,
                max: 0.4
            })]
        );
    }

    #[test]
    fn columns_merge_blocks_and_pad_missing_history() {
        let h = history_with(&[(-0.1, 0.1), (-0.5, 0.2), (-0.2, 0.9), (0.0, 0.3)]);
        assert_eq!(
            h.columns(0, 4, 2),
            vec![
                Some(Extent {
                    min: -0.5,
                    max: 0.2
                }),
                Some(Extent {
                    min: -0.2,
                    max: 0.9
                })
            ]
        );
        // Asking for more history than exists leaves the oldest columns empty.
        let cols = h.columns(0, 8, 4);
        assert_eq!(cols[0], None);
        assert_eq!(cols[1], None);
        assert_eq!(
            cols[3],
            Some(Extent {
                min: -0.2,
                max: 0.9
            })
        );
    }

    #[test]
    fn more_columns_than_blocks_still_shows_every_block() {
        let h = history_with(&[(-0.4, 0.4), (-0.1, 0.1)]);
        let cols = h.columns(0, 2, 4);
        assert_eq!(cols.len(), 4);
        assert!(cols.iter().flatten().any(|e| e.max == 0.4));
        assert!(cols.iter().flatten().any(|e| e.max == 0.1));
    }
}
