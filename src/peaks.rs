//! Waveform overview of a whole take: min/max per block for every file channel ("lane"),
//! stored next to the audio as `peaks.bin` so a take opens instantly.

use std::fs;
use std::io::{self, Read, Write};
use std::path::Path;

use crate::wav::WavReader;
use crate::waveform::{Extent, block_frames};

pub const PEAKS_FILE: &str = "peaks.bin";
const MAGIC: &[u8; 8] = b"RRPEAKS1";
const HEADER_LEN: usize = 8 + 4 + 4 + 8;
const READ_CHUNK_FRAMES: usize = 1 << 16;
const SCALE: f32 = 127.0;

/// One block of one lane, quantized to 8 bits per bound (plenty for drawing).
pub type Block = [i8; 2];

fn quantize(min: f32, max: f32) -> Block {
    [
        (min * SCALE).floor().clamp(-SCALE, SCALE) as i8,
        (max * SCALE).ceil().clamp(-SCALE, SCALE) as i8,
    ]
}

/// Turns a stream of samples of one channel into blocks.
#[derive(Debug, Clone)]
pub struct LaneBuilder {
    block: usize,
    fill: usize,
    min: f32,
    max: f32,
    blocks: Vec<Block>,
}

impl LaneBuilder {
    pub fn new(block: usize) -> Self {
        Self {
            block: block.max(1),
            fill: 0,
            min: f32::INFINITY,
            max: f32::NEG_INFINITY,
            blocks: Vec::new(),
        }
    }

    /// Continues a lane that already covers `frames` frames (appending to a take). A partial
    /// last block is reopened so the new samples keep the block grid.
    pub fn resume(block: usize, mut blocks: Vec<Block>, frames: u64) -> Self {
        let mut b = Self::new(block);
        let partial = (frames % b.block as u64) as usize;
        blocks.truncate(frames.div_ceil(b.block as u64) as usize);
        if partial > 0
            && let Some([min, max]) = blocks.pop()
        {
            b.fill = partial;
            b.min = f32::from(min) / SCALE;
            b.max = f32::from(max) / SCALE;
        }
        b.blocks = blocks;
        b
    }

    pub fn push(&mut self, sample: f32) {
        self.min = self.min.min(sample);
        self.max = self.max.max(sample);
        self.fill += 1;
        if self.fill == self.block {
            self.flush();
        }
    }

    fn flush(&mut self) {
        if self.fill > 0 {
            self.blocks.push(quantize(self.min, self.max));
            self.fill = 0;
            self.min = f32::INFINITY;
            self.max = f32::NEG_INFINITY;
        }
    }

    /// Finishes the last partial block and returns all blocks.
    pub fn finish(mut self) -> Vec<Block> {
        self.flush();
        self.blocks
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TakePeaks {
    /// Frames per block.
    pub block: u32,
    /// One lane per file channel, in track order.
    pub lanes: Vec<Vec<Block>>,
}

impl TakePeaks {
    pub fn blocks(&self) -> usize {
        self.lanes.iter().map(Vec::len).max().unwrap_or(0)
    }

    pub fn save(&self, dir: &Path) -> io::Result<()> {
        let blocks = self.blocks();
        let mut bytes = Vec::with_capacity(HEADER_LEN + self.lanes.len() * blocks * 2);
        bytes.extend_from_slice(MAGIC);
        bytes.extend_from_slice(&self.block.to_le_bytes());
        bytes.extend_from_slice(&(self.lanes.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&(blocks as u64).to_le_bytes());
        for lane in &self.lanes {
            for i in 0..blocks {
                let [min, max] = lane.get(i).copied().unwrap_or([0, 0]);
                bytes.extend_from_slice(&[min as u8, max as u8]);
            }
        }
        let tmp = dir.join(format!("{PEAKS_FILE}.tmp"));
        fs::File::create(&tmp)?.write_all(&bytes)?;
        fs::rename(tmp, dir.join(PEAKS_FILE))
    }

    pub fn load(dir: &Path) -> io::Result<Self> {
        let invalid = |msg: &str| io::Error::new(io::ErrorKind::InvalidData, msg.to_owned());
        let mut bytes = Vec::new();
        fs::File::open(dir.join(PEAKS_FILE))?.read_to_end(&mut bytes)?;
        if bytes.len() < HEADER_LEN || &bytes[..8] != MAGIC {
            return Err(invalid("not a peaks file"));
        }
        let u32_at =
            |i: usize| u32::from_le_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]);
        let block = u32_at(8);
        let lanes = u32_at(12) as usize;
        let mut count = [0u8; 8];
        count.copy_from_slice(&bytes[16..24]);
        let blocks = u64::from_le_bytes(count) as usize;
        let body = &bytes[HEADER_LEN..];
        if block == 0
            || lanes.checked_mul(blocks).and_then(|n| n.checked_mul(2)) != Some(body.len())
        {
            return Err(invalid("peaks file has the wrong size"));
        }
        let lanes = (0..lanes)
            .map(|l| {
                body[l * blocks * 2..(l + 1) * blocks * 2]
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|b| [b[0] as i8, b[1] as i8])
                    .collect()
            })
            .collect();
        Ok(Self { block, lanes })
    }

    /// Scans the take's WAV files (used for takes recorded before peaks were stored, or
    /// after a crash).
    pub fn build(files: &[impl AsRef<Path>], sample_rate: u32) -> io::Result<Self> {
        let block = block_frames(sample_rate);
        let mut lanes = Vec::new();
        let mut buf = Vec::new();
        for file in files {
            let mut reader = WavReader::open(file.as_ref())?;
            let channels = usize::from(reader.spec().channels);
            let mut builders = vec![LaneBuilder::new(block); channels];
            let mut at = 0;
            loop {
                buf.clear();
                let n = reader.read(at, READ_CHUNK_FRAMES, &mut buf)?;
                if n == 0 {
                    break;
                }
                at += n as u64;
                for frame in buf.chunks_exact(channels) {
                    for (b, s) in builders.iter_mut().zip(frame) {
                        b.push(*s);
                    }
                }
            }
            lanes.extend(builders.into_iter().map(LaneBuilder::finish));
        }
        Ok(Self {
            block: block as u32,
            lanes,
        })
    }

    /// `columns` extents of `lane` covering blocks `start .. start + span` (fractional).
    /// Columns outside the take are `None`.
    pub fn columns(
        &self,
        lane: usize,
        start: f64,
        span: f64,
        columns: usize,
    ) -> Vec<Option<Extent>> {
        let Some(blocks) = self.lanes.get(lane) else {
            return vec![None; columns];
        };
        let per_column = span / columns as f64;
        (0..columns)
            .map(|col| {
                let from = start + col as f64 * per_column;
                let to = from + per_column;
                if to <= 0.0 || from >= blocks.len() as f64 {
                    return None;
                }
                let lo = from.max(0.0) as usize;
                let hi = (to.ceil() as usize).clamp(lo + 1, blocks.len());
                blocks[lo.min(blocks.len() - 1)..hi]
                    .iter()
                    .map(|b| Extent {
                        min: f32::from(b[0]) / SCALE,
                        max: f32::from(b[1]) / SCALE,
                    })
                    .reduce(Extent::merge)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wav::{Bext, SampleFormat, WavSpec, WavWriter};
    use chrono::NaiveDate;

    fn peaks() -> TakePeaks {
        TakePeaks {
            block: 240,
            lanes: vec![vec![[-10, 20], [-30, 40], [0, 5]], vec![[-1, 1], [-2, 2]]],
        }
    }

    #[test]
    fn builder_groups_samples_into_blocks_and_keeps_the_partial_tail() {
        let mut b = LaneBuilder::new(2);
        for s in [0.5, -0.25, 1.0, 0.0, -1.0] {
            b.push(s);
        }
        assert_eq!(b.finish(), vec![[-32, 64], [0, 127], [-127, -127]]);
    }

    #[test]
    fn resumed_lanes_keep_the_block_grid() {
        // 5 frames in 2-frame blocks: two full blocks and one holding a single frame.
        let mut b = LaneBuilder::resume(2, vec![[-32, 64], [0, 127], [-127, -127]], 5);
        b.push(0.5);
        b.push(0.25);
        assert_eq!(b.finish(), vec![[-32, 64], [0, 127], [-127, 64], [31, 32]]);
        // Blocks beyond the stated length are dropped.
        let b = LaneBuilder::resume(2, vec![[1, 1], [2, 2], [3, 3]], 2);
        assert_eq!(b.finish(), vec![[1, 1]]);
    }

    #[test]
    fn quantization_never_shrinks_the_extent() {
        assert_eq!(quantize(-0.001, 0.001), [-1, 1]);
        assert_eq!(quantize(-2.0, 2.0), [-127, 127]);
    }

    #[test]
    fn save_and_load_roundtrip_padding_short_lanes() {
        let dir = tempfile::tempdir().unwrap();
        peaks().save(dir.path()).unwrap();
        let loaded = TakePeaks::load(dir.path()).unwrap();
        assert_eq!(loaded.block, 240);
        assert_eq!(loaded.lanes[0], peaks().lanes[0]);
        assert_eq!(loaded.lanes[1], vec![[-1, 1], [-2, 2], [0, 0]]);
    }

    #[test]
    fn damaged_files_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        assert!(TakePeaks::load(dir.path()).is_err());
        fs::write(dir.path().join(PEAKS_FILE), b"RRPEAKS1 short").unwrap();
        assert!(TakePeaks::load(dir.path()).is_err());
        peaks().save(dir.path()).unwrap();
        let mut bytes = fs::read(dir.path().join(PEAKS_FILE)).unwrap();
        bytes.pop();
        fs::write(dir.path().join(PEAKS_FILE), bytes).unwrap();
        assert!(TakePeaks::load(dir.path()).is_err());
    }

    #[test]
    fn build_reads_every_channel_of_every_file() {
        let dir = tempfile::tempdir().unwrap();
        let t = NaiveDate::from_ymd_opt(2026, 1, 1)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap();
        let path = dir.path().join("a.wav");
        // 400 Hz gives 2-frame blocks.
        let spec = WavSpec {
            channels: 2,
            sample_rate: 400,
            format: SampleFormat::Float32,
        };
        let mut w = WavWriter::create(&path, spec, &Bext::at(t, 400, String::new()), 0).unwrap();
        w.write_samples(&[0.5, -0.5, 1.0, 0.0, -1.0, 0.25]).unwrap();
        w.finalize().unwrap();
        let p = TakePeaks::build(&[&path], 400).unwrap();
        assert_eq!(p.block, 2);
        assert_eq!(
            p.lanes,
            vec![vec![[63, 127], [-127, -127]], vec![[-64, 0], [31, 32]]]
        );
    }

    #[test]
    fn columns_cover_the_requested_window() {
        let p = peaks();
        let cols = p.columns(0, 0.0, 3.0, 3);
        assert_eq!(
            cols[1],
            Some(Extent {
                min: -30.0 / 127.0,
                max: 40.0 / 127.0
            })
        );
        // Zoomed out: several blocks per column are merged.
        let merged = p.columns(0, 0.0, 4.0, 2);
        assert_eq!(
            merged[0],
            Some(Extent {
                min: -30.0 / 127.0,
                max: 40.0 / 127.0
            })
        );
        // Before the start and after the end there is nothing to draw.
        let around = p.columns(0, -2.0, 8.0, 8);
        assert_eq!(around[0], None);
        assert!(around[2].is_some());
        assert_eq!(around[7], None);
        assert_eq!(p.columns(9, 0.0, 3.0, 2), vec![None, None]);
    }
}
