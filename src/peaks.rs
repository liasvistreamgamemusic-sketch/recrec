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

pub fn quantize(min: f32, max: f32) -> Block {
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

fn merge(a: Block, b: Block) -> Block {
    [a[0].min(b[0]), a[1].max(b[1])]
}

fn extent(b: Block) -> Extent {
    Extent {
        min: f32::from(b[0]) / SCALE,
        max: f32::from(b[1]) / SCALE,
    }
}

/// Blocks of one lane plus coarser copies (each level merges pairs of the one below), so
/// any stretch of the lane can be summarized in a few steps however long the recording is.
#[derive(Debug, Clone, Default)]
pub struct Pyramid {
    /// `levels[0]` are the blocks themselves; `levels[k][i]` covers blocks `i << k ..`.
    levels: Vec<Vec<Block>>,
}

impl Pyramid {
    pub fn from_blocks(blocks: &[Block]) -> Self {
        let mut p = Self::default();
        for b in blocks {
            p.push(*b);
        }
        p
    }

    pub fn len(&self) -> usize {
        self.levels.first().map_or(0, Vec::len)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn push(&mut self, block: Block) {
        if self.levels.is_empty() {
            self.levels.push(Vec::new());
        }
        self.levels[0].push(block);
        let mut k = 0;
        // Whenever a level gains a full pair, the level above gains their merge.
        while self.levels[k].len().is_multiple_of(2) {
            let n = self.levels[k].len();
            let merged = merge(self.levels[k][n - 2], self.levels[k][n - 1]);
            if self.levels.len() == k + 1 {
                self.levels.push(Vec::new());
            }
            self.levels[k + 1].push(merged);
            k += 1;
        }
    }

    /// Drops the oldest `count` blocks.
    pub fn drop_front(&mut self, count: usize) {
        let keep = self
            .levels
            .first()
            .map(|base| base[count.min(base.len())..].to_vec())
            .unwrap_or_default();
        *self = Self::from_blocks(&keep);
    }

    /// Extent of blocks `lo..hi` (`None` when the range is empty or outside the lane).
    pub fn range(&self, lo: usize, hi: usize) -> Option<Extent> {
        let hi = hi.min(self.len());
        let mut at = lo;
        let mut acc: Option<Block> = None;
        while at < hi {
            // The largest aligned chunk that starts here, fits, and exists at its level.
            let mut k = (at.trailing_zeros() as usize).min(self.levels.len() - 1);
            while k > 0 && (at + (1 << k) > hi || (at >> k) >= self.levels[k].len()) {
                k -= 1;
            }
            let b = self.levels[k][at >> k];
            acc = Some(acc.map_or(b, |a| merge(a, b)));
            at += 1 << k;
        }
        acc.map(extent)
    }

    /// `columns` extents covering blocks `start .. start + span` (fractional). Columns that
    /// lie outside the lane are `None`.
    pub fn columns(&self, start: f64, span: f64, columns: usize) -> Vec<Option<Extent>> {
        let per_column = span / columns as f64;
        let len = self.len() as f64;
        (0..columns)
            .map(|col| {
                let from = start + col as f64 * per_column;
                let to = from + per_column;
                if to <= 0.0 || from >= len {
                    return None;
                }
                let lo = from.max(0.0) as usize;
                let hi = (to.ceil() as usize).max(lo + 1);
                self.range(lo.min(self.len() - 1), hi)
            })
            .collect()
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

    fn ext(min: i8, max: i8) -> Option<Extent> {
        Some(extent([min, max]))
    }

    #[test]
    fn pyramid_ranges_match_a_plain_scan() {
        let blocks: Vec<Block> = (0..37i32)
            .map(|i| [-(i % 11 + 1) as i8, ((i * 7) % 13 + 1) as i8])
            .collect();
        let p = Pyramid::from_blocks(&blocks);
        assert_eq!(p.len(), 37);
        for lo in 0..37 {
            for hi in lo + 1..=37 {
                let plain = blocks[lo..hi].iter().copied().reduce(merge).unwrap();
                assert_eq!(p.range(lo, hi), Some(extent(plain)), "{lo}..{hi}");
            }
        }
        assert_eq!(p.range(5, 5), None);
        assert_eq!(p.range(40, 50), None);
        assert_eq!(p.range(30, 99), p.range(30, 37));
        assert_eq!(Pyramid::default().range(0, 4), None);
    }

    #[test]
    fn pyramid_columns_cover_the_requested_window() {
        let p = Pyramid::from_blocks(&[[-10, 20], [-30, 40], [0, 5]]);
        assert_eq!(p.columns(0.0, 3.0, 3)[1], ext(-30, 40));
        // Zoomed out: several blocks per column are merged.
        assert_eq!(p.columns(0.0, 4.0, 2)[0], ext(-30, 40));
        // Before the start and after the end there is nothing to draw.
        let around = p.columns(-2.0, 8.0, 8);
        assert_eq!(around[0], None);
        assert_eq!(around[2], ext(-10, 20));
        assert_eq!(around[7], None);
        // More columns than blocks: every block still shows.
        let wide = p.columns(0.0, 3.0, 6);
        assert_eq!(wide[0], ext(-10, 20));
        assert_eq!(wide[5], ext(0, 5));
    }

    #[test]
    fn dropping_the_front_keeps_the_rest_queryable() {
        let blocks: Vec<Block> = (0..20).map(|i| [-(i as i8), i as i8]).collect();
        let mut p = Pyramid::from_blocks(&blocks);
        p.drop_front(7);
        assert_eq!(p.len(), 13);
        assert_eq!(p.range(0, 13), ext(-19, 19));
        assert_eq!(p.range(0, 1), ext(-7, 7));
        p.drop_front(100);
        assert!(p.is_empty());
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
}
