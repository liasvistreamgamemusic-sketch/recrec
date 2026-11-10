//! Crash-tolerant WAV writer with BWF (`bext`) metadata and automatic RF64 upgrade.
//!
//! Layout (see design doc §5): `RIFF/RF64` · `JUNK`(28, reserved for `ds64`) · `fmt ` ·
//! `fact` (float only) · `bext` · `data`. Size fields are rewritten by [`WavWriter::flush_header`]
//! so a file stays readable even if the process dies mid-recording; [`repair`] fixes files whose
//! last header update was lost.

use std::fs::{File, OpenOptions};
use std::io::{self, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;

use chrono::{NaiveDateTime, Timelike};
use serde::{Deserialize, Serialize};

use crate::dither::Tpdf;

const RIFF_HEADER_LEN: u64 = 12;
const DS64_BODY_LEN: u32 = 28;
const FMT_BODY_LEN: u32 = 16;
const FACT_BODY_LEN: u32 = 4;
const BEXT_BODY_LEN: u32 = 602;
const FORMAT_TAG_PCM: u16 = 1;
const FORMAT_TAG_IEEE_FLOAT: u16 = 3;
/// u32 size fields hold this value once the file has become RF64.
const RF64_SIZE_MARKER: u32 = u32::MAX;
const WRITE_BUFFER_BYTES: usize = 256 * 1024;
const SCALE_16: f32 = 32_768.0;
const SCALE_24: f32 = 8_388_608.0;

/// Sample encoding of the files we write.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum SampleFormat {
    Pcm16,
    #[default]
    Pcm24,
    Float32,
}

impl SampleFormat {
    pub const ALL: [SampleFormat; 3] = [Self::Pcm16, Self::Pcm24, Self::Float32];

    pub fn bytes_per_sample(self) -> u16 {
        match self {
            Self::Pcm16 => 2,
            Self::Pcm24 => 3,
            Self::Float32 => 4,
        }
    }

    fn format_tag(self) -> u16 {
        match self {
            Self::Pcm16 | Self::Pcm24 => FORMAT_TAG_PCM,
            Self::Float32 => FORMAT_TAG_IEEE_FLOAT,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Pcm16 => "16-bit PCM",
            Self::Pcm24 => "24-bit PCM",
            Self::Float32 => "32-bit float",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WavSpec {
    pub channels: u16,
    pub sample_rate: u32,
    pub format: SampleFormat,
}

impl WavSpec {
    pub fn block_align(&self) -> u16 {
        self.channels * self.format.bytes_per_sample()
    }

    pub fn bytes_per_second(&self) -> u64 {
        u64::from(self.sample_rate) * u64::from(self.block_align())
    }
}

/// Broadcast-WAV metadata written to the `bext` chunk.
#[derive(Debug, Clone)]
pub struct Bext {
    pub description: String,
    pub originator: String,
    /// Local wall-clock time of the first sample.
    pub origination: NaiveDateTime,
    /// First sample position counted in samples since local midnight.
    pub time_reference: u64,
}

impl Bext {
    /// Builds metadata whose time reference points at `origination` for the given rate.
    pub fn at(origination: NaiveDateTime, sample_rate: u32, description: String) -> Self {
        let since_midnight = u64::from(origination.num_seconds_from_midnight());
        Self {
            description,
            originator: "recrec".to_owned(),
            origination,
            time_reference: since_midnight * u64::from(sample_rate),
        }
    }
}

/// Byte positions of the size fields, fixed once the header is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Layout {
    data_size_pos: u64,
    fact_pos: Option<u64>,
    data_start: u64,
}

pub struct WavWriter {
    out: BufWriter<File>,
    spec: WavSpec,
    layout: Layout,
    data_bytes: u64,
    rf64: bool,
    rf64_threshold: u64,
    dither: Vec<Tpdf>,
    encode_buf: Vec<u8>,
    finalized: bool,
}

impl WavWriter {
    /// Creates a new file (fails if it already exists) and writes the header.
    /// `dither_seed` decorrelates TPDF noise between files.
    pub fn create(path: &Path, spec: WavSpec, bext: &Bext, dither_seed: u32) -> io::Result<Self> {
        let file = OpenOptions::new().write(true).create_new(true).open(path)?;
        let mut out = BufWriter::with_capacity(WRITE_BUFFER_BYTES, file);
        let layout = write_header(&mut out, &spec, bext)?;
        out.flush()?;
        let dither = (0..u32::from(spec.channels))
            .map(|ch| Tpdf::new(dither_seed.wrapping_mul(64).wrapping_add(ch)))
            .collect();
        Ok(Self {
            out,
            spec,
            layout,
            data_bytes: 0,
            rf64: false,
            rf64_threshold: u64::from(u32::MAX),
            dither,
            encode_buf: Vec::new(),
            finalized: false,
        })
    }

    pub fn spec(&self) -> WavSpec {
        self.spec
    }

    pub fn frames_written(&self) -> u64 {
        self.data_bytes / u64::from(self.spec.block_align())
    }

    pub fn data_bytes(&self) -> u64 {
        self.data_bytes
    }

    /// Appends interleaved samples (`len` must be a multiple of the channel count).
    pub fn write_samples(&mut self, interleaved: &[f32]) -> io::Result<()> {
        let channels = usize::from(self.spec.channels);
        debug_assert_eq!(interleaved.len() % channels, 0);
        self.encode_buf.clear();
        self.encode_buf
            .reserve(interleaved.len() * usize::from(self.spec.format.bytes_per_sample()));
        match self.spec.format {
            SampleFormat::Pcm16 => {
                for frame in interleaved.chunks_exact(channels) {
                    for (x, dither) in frame.iter().zip(self.dither.iter_mut()) {
                        let v = (x * SCALE_16 + dither.sample()).round();
                        let v = v.clamp(i16::MIN as f32, i16::MAX as f32) as i16;
                        self.encode_buf.extend_from_slice(&v.to_le_bytes());
                    }
                }
            }
            SampleFormat::Pcm24 => {
                for x in interleaved {
                    let v = encode_i24(*x);
                    self.encode_buf.extend_from_slice(&v.to_le_bytes()[..3]);
                }
            }
            SampleFormat::Float32 => {
                for x in interleaved {
                    self.encode_buf.extend_from_slice(&x.to_le_bytes());
                }
            }
        }
        self.out.write_all(&self.encode_buf)?;
        self.data_bytes += self.encode_buf.len() as u64;
        Ok(())
    }

    /// Rewrites the size fields so the file is valid up to everything written so far.
    pub fn flush_header(&mut self) -> io::Result<()> {
        self.out.flush()?;
        let file = self.out.get_mut();
        self.rf64 = write_sizes(
            file,
            &self.layout,
            &self.spec,
            self.data_bytes,
            false,
            self.rf64,
            self.rf64_threshold,
        )?;
        file.seek(SeekFrom::End(0))?;
        Ok(())
    }

    /// Pushes written data from the OS cache to the disk.
    pub fn sync(&mut self) -> io::Result<()> {
        self.out.flush()?;
        self.out.get_ref().sync_data()
    }

    /// Writes the pad byte and final sizes, then syncs. Safe to call more than once.
    pub fn finalize(&mut self) -> io::Result<()> {
        if self.finalized {
            return Ok(());
        }
        self.out.flush()?;
        let padded = self.data_bytes % 2 == 1;
        if padded {
            self.out.write_all(&[0])?;
            self.out.flush()?;
        }
        let file = self.out.get_mut();
        self.rf64 = write_sizes(
            file,
            &self.layout,
            &self.spec,
            self.data_bytes,
            padded,
            self.rf64,
            self.rf64_threshold,
        )?;
        file.sync_all()?;
        self.finalized = true;
        Ok(())
    }

    #[cfg(test)]
    fn set_rf64_threshold(&mut self, bytes: u64) {
        self.rf64_threshold = bytes;
    }
}

impl Drop for WavWriter {
    fn drop(&mut self) {
        if !self.finalized
            && let Err(e) = self.finalize()
        {
            log::error!("failed to finalize WAV file on drop: {e}");
        }
    }
}

fn encode_i24(x: f32) -> i32 {
    let max = (1 << 23) - 1;
    let min = -(1 << 23);
    ((x * SCALE_24).round().clamp(min as f32, max as f32)) as i32
}

fn write_header<W: Write>(out: &mut W, spec: &WavSpec, bext: &Bext) -> io::Result<Layout> {
    let has_fact = spec.format == SampleFormat::Float32;
    let mut pos = RIFF_HEADER_LEN;

    out.write_all(b"RIFF")?;
    out.write_all(&0u32.to_le_bytes())?;
    out.write_all(b"WAVE")?;

    out.write_all(b"JUNK")?;
    out.write_all(&DS64_BODY_LEN.to_le_bytes())?;
    out.write_all(&[0u8; DS64_BODY_LEN as usize])?;
    pos += 8 + u64::from(DS64_BODY_LEN);

    out.write_all(b"fmt ")?;
    out.write_all(&FMT_BODY_LEN.to_le_bytes())?;
    out.write_all(&spec.format.format_tag().to_le_bytes())?;
    out.write_all(&spec.channels.to_le_bytes())?;
    out.write_all(&spec.sample_rate.to_le_bytes())?;
    let byte_rate = u32::try_from(spec.bytes_per_second())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "byte rate overflows u32"))?;
    out.write_all(&byte_rate.to_le_bytes())?;
    out.write_all(&spec.block_align().to_le_bytes())?;
    out.write_all(&(spec.format.bytes_per_sample() * 8).to_le_bytes())?;
    pos += 8 + u64::from(FMT_BODY_LEN);

    let fact_pos = if has_fact {
        out.write_all(b"fact")?;
        out.write_all(&FACT_BODY_LEN.to_le_bytes())?;
        out.write_all(&0u32.to_le_bytes())?;
        let at = pos + 8;
        pos += 8 + u64::from(FACT_BODY_LEN);
        Some(at)
    } else {
        None
    };

    out.write_all(b"bext")?;
    out.write_all(&BEXT_BODY_LEN.to_le_bytes())?;
    out.write_all(&bext_body(bext))?;
    pos += 8 + u64::from(BEXT_BODY_LEN);

    out.write_all(b"data")?;
    out.write_all(&0u32.to_le_bytes())?;
    let data_size_pos = pos + 4;
    pos += 8;

    Ok(Layout {
        data_size_pos,
        fact_pos,
        data_start: pos,
    })
}

fn bext_body(bext: &Bext) -> Vec<u8> {
    fn fixed(text: &str, len: usize) -> Vec<u8> {
        // bext text fields are ASCII; replace anything else so readers don't choke.
        let mut bytes: Vec<u8> = text
            .chars()
            .map(|c| {
                if c.is_ascii() && !c.is_ascii_control() {
                    c as u8
                } else {
                    b'_'
                }
            })
            .take(len)
            .collect();
        bytes.resize(len, 0);
        bytes
    }
    let mut body = Vec::with_capacity(BEXT_BODY_LEN as usize);
    body.extend(fixed(&bext.description, 256));
    body.extend(fixed(&bext.originator, 32));
    body.extend(fixed("", 32)); // OriginatorReference
    body.extend(fixed(&bext.origination.format("%Y-%m-%d").to_string(), 10));
    body.extend(fixed(&bext.origination.format("%H:%M:%S").to_string(), 8));
    body.extend_from_slice(&(bext.time_reference as u32).to_le_bytes());
    body.extend_from_slice(&((bext.time_reference >> 32) as u32).to_le_bytes());
    body.extend_from_slice(&1u16.to_le_bytes()); // Version
    body.resize(BEXT_BODY_LEN as usize, 0); // UMID, loudness, reserved
    body
}

/// Writes RIFF/data/fact sizes for `data_bytes` of audio. Returns whether the file is RF64.
fn write_sizes(
    file: &mut File,
    layout: &Layout,
    spec: &WavSpec,
    data_bytes: u64,
    padded: bool,
    already_rf64: bool,
    rf64_threshold: u64,
) -> io::Result<bool> {
    let riff_size = layout.data_start + data_bytes + u64::from(padded) - 8;
    let frames = data_bytes / u64::from(spec.block_align());
    let rf64 = already_rf64 || riff_size > rf64_threshold;

    let mut put = |pos: u64, bytes: &[u8]| -> io::Result<()> {
        file.seek(SeekFrom::Start(pos))?;
        file.write_all(bytes)
    };
    if rf64 {
        put(0, b"RF64")?;
        put(4, &RF64_SIZE_MARKER.to_le_bytes())?;
        let mut ds64 = Vec::with_capacity(8 + DS64_BODY_LEN as usize);
        ds64.extend_from_slice(b"ds64");
        ds64.extend_from_slice(&DS64_BODY_LEN.to_le_bytes());
        ds64.extend_from_slice(&riff_size.to_le_bytes());
        ds64.extend_from_slice(&data_bytes.to_le_bytes());
        ds64.extend_from_slice(&frames.to_le_bytes());
        ds64.extend_from_slice(&0u32.to_le_bytes()); // table length
        put(RIFF_HEADER_LEN, &ds64)?;
        put(layout.data_size_pos, &RF64_SIZE_MARKER.to_le_bytes())?;
    } else {
        // Not RF64, so riff_size (and therefore data_bytes) fits in u32.
        put(4, &(riff_size as u32).to_le_bytes())?;
        put(layout.data_size_pos, &(data_bytes as u32).to_le_bytes())?;
    }
    if let Some(fact_pos) = layout.fact_pos {
        put(
            fact_pos,
            &u32::try_from(frames).unwrap_or(u32::MAX).to_le_bytes(),
        )?;
    }
    Ok(rf64)
}

/// What a WAV header says, as found by walking its chunks.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Scanned {
    rf64: bool,
    format_tag: u16,
    channels: u16,
    sample_rate: u32,
    bits: u16,
    block_align: u16,
    layout: Layout,
}

fn scan(file: &mut File) -> io::Result<Scanned> {
    let invalid = |msg: &str| io::Error::new(io::ErrorKind::InvalidData, msg.to_owned());
    let mut header = [0u8; RIFF_HEADER_LEN as usize];
    file.seek(SeekFrom::Start(0))?;
    file.read_exact(&mut header)?;
    let rf64 = match &header[0..4] {
        b"RIFF" => false,
        b"RF64" => true,
        _ => return Err(invalid("not a RIFF/RF64 file")),
    };
    if &header[8..12] != b"WAVE" {
        return Err(invalid("not a WAVE file"));
    }

    let file_len = file.metadata()?.len();
    let mut pos = RIFF_HEADER_LEN;
    let mut fmt: Option<(u16, u16, u32, u16, u16)> = None;
    let mut fact_pos = None;
    while pos + 8 <= file_len {
        let mut chunk = [0u8; 8];
        file.seek(SeekFrom::Start(pos))?;
        file.read_exact(&mut chunk)?;
        let size = u64::from(u32::from_le_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]));
        match &chunk[0..4] {
            b"data" => {
                let (format_tag, channels, sample_rate, block_align, bits) =
                    fmt.ok_or_else(|| invalid("data chunk before fmt chunk"))?;
                if block_align == 0 {
                    return Err(invalid("block align is zero"));
                }
                return Ok(Scanned {
                    rf64,
                    format_tag,
                    channels,
                    sample_rate,
                    bits,
                    block_align,
                    layout: Layout {
                        data_size_pos: pos + 4,
                        fact_pos,
                        data_start: pos + 8,
                    },
                });
            }
            b"fmt " => {
                let mut body = [0u8; FMT_BODY_LEN as usize];
                file.read_exact(&mut body)?;
                let u16_at = |i: usize| u16::from_le_bytes([body[i], body[i + 1]]);
                let sample_rate = u32::from_le_bytes([body[4], body[5], body[6], body[7]]);
                fmt = Some((u16_at(0), u16_at(2), sample_rate, u16_at(12), u16_at(14)));
            }
            b"fact" => fact_pos = Some(pos + 8),
            _ => {}
        }
        pos += 8 + size + (size & 1);
    }
    Err(invalid("no data chunk"))
}

/// Result of [`repair`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Repaired {
    pub frames: u64,
    pub data_bytes: u64,
}

/// Recomputes the size fields of a file whose writer did not finish (crash, power loss).
/// Trailing bytes that do not form a whole frame are cut off.
pub fn repair(path: &Path) -> io::Result<Repaired> {
    let mut file = OpenOptions::new().read(true).write(true).open(path)?;
    let scanned = scan(&mut file)?;
    let block_align = u64::from(scanned.block_align);
    let file_len = file.metadata()?.len();
    let available = file_len.saturating_sub(scanned.layout.data_start);
    let data_bytes = available - available % block_align;
    let padded = data_bytes % 2 == 1;
    file.set_len(scanned.layout.data_start + data_bytes)?;
    if padded {
        file.seek(SeekFrom::End(0))?;
        file.write_all(&[0])?;
    }
    let format = match (scanned.format_tag, scanned.bits) {
        (FORMAT_TAG_PCM, 16) => SampleFormat::Pcm16,
        (FORMAT_TAG_PCM, 24) => SampleFormat::Pcm24,
        (FORMAT_TAG_IEEE_FLOAT, 32) => SampleFormat::Float32,
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "unsupported format tag {} with {} bits",
                    scanned.format_tag, scanned.bits
                ),
            ));
        }
    };
    let spec = WavSpec {
        channels: scanned.channels,
        sample_rate: scanned.sample_rate,
        format,
    };
    write_sizes(
        &mut file,
        &scanned.layout,
        &spec,
        data_bytes,
        padded,
        scanned.rf64,
        u64::from(u32::MAX),
    )?;
    file.sync_all()?;
    Ok(Repaired {
        frames: data_bytes / block_align,
        data_bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;
    use std::fs;

    fn bext() -> Bext {
        let t = NaiveDate::from_ymd_opt(2026, 11, 10)
            .unwrap()
            .and_hms_opt(1, 0, 0)
            .unwrap();
        Bext::at(t, 48_000, "Guitar".into())
    }

    fn spec(channels: u16, format: SampleFormat) -> WavSpec {
        WavSpec {
            channels,
            sample_rate: 48_000,
            format,
        }
    }

    fn u32_at(bytes: &[u8], pos: u64) -> u32 {
        let p = pos as usize;
        u32::from_le_bytes(bytes[p..p + 4].try_into().unwrap())
    }

    fn u64_at(bytes: &[u8], pos: u64) -> u64 {
        let p = pos as usize;
        u64::from_le_bytes(bytes[p..p + 8].try_into().unwrap())
    }

    fn scan_path(path: &Path) -> Scanned {
        scan(&mut File::open(path).unwrap()).unwrap()
    }

    fn data_of(path: &Path) -> Vec<u8> {
        let bytes = fs::read(path).unwrap();
        let s = scan_path(path);
        let size = if s.rf64 {
            u64_at(&bytes, RIFF_HEADER_LEN + 16)
        } else {
            u64::from(u32_at(&bytes, s.layout.data_size_pos))
        };
        let start = s.layout.data_start as usize;
        bytes[start..start + size as usize].to_vec()
    }

    #[test]
    fn header_fields_are_correct_for_24bit_mono() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.wav");
        let mut w = WavWriter::create(&path, spec(1, SampleFormat::Pcm24), &bext(), 0).unwrap();
        w.write_samples(&[0.0, 0.5, -0.5, 0.25]).unwrap();
        w.finalize().unwrap();

        let bytes = fs::read(&path).unwrap();
        let s = scan_path(&path);
        assert!(!s.rf64);
        assert_eq!(
            (
                s.format_tag,
                s.channels,
                s.sample_rate,
                s.bits,
                s.block_align
            ),
            (1, 1, 48_000, 24, 3)
        );
        assert_eq!(&bytes[12..16], b"JUNK");
        assert_eq!(u32_at(&bytes, s.layout.data_size_pos), 12);
        assert_eq!(u32_at(&bytes, 4) as usize, bytes.len() - 8);
        assert_eq!(bytes.len() % 2, 0);
        assert_eq!(u32_at(&bytes, 64), 144_000); // byte rate: fmt chunk at 48, body at 56
    }

    #[test]
    fn bext_chunk_carries_time_reference_and_date() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.wav");
        let mut w = WavWriter::create(&path, spec(1, SampleFormat::Pcm24), &bext(), 0).unwrap();
        w.finalize().unwrap();
        let bytes = fs::read(&path).unwrap();
        let at = bytes.windows(4).position(|c| c == b"bext").unwrap() as u64;
        assert_eq!(u32_at(&bytes, at + 4), BEXT_BODY_LEN);
        let body = at + 8;
        assert_eq!(&bytes[body as usize..body as usize + 6], b"Guitar");
        assert_eq!(
            &bytes[(body + 320) as usize..(body + 330) as usize],
            b"2026-11-10"
        );
        assert_eq!(
            &bytes[(body + 330) as usize..(body + 338) as usize],
            b"01:00:00"
        );
        assert_eq!(u64_at(&bytes, body + 338), 3600 * 48_000);
    }

    #[test]
    fn pcm24_is_bit_exact_for_24bit_sources() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.wav");
        let originals: Vec<i32> = vec![0, 1, -1, 8_388_607, -8_388_608, 123_456, -654_321];
        let samples: Vec<f32> = originals.iter().map(|&v| v as f32 / SCALE_24).collect();
        let mut w = WavWriter::create(&path, spec(1, SampleFormat::Pcm24), &bext(), 0).unwrap();
        w.write_samples(&samples).unwrap();
        w.finalize().unwrap();

        let data = data_of(&path);
        let decoded: Vec<i32> = data
            .as_chunks::<3>()
            .0
            .iter()
            .map(|b| i32::from_le_bytes([0, b[0], b[1], b[2]]) >> 8)
            .collect();
        assert_eq!(decoded, originals);
    }

    #[test]
    fn pcm24_clips_out_of_range_values() {
        assert_eq!(encode_i24(1.0), 8_388_607);
        assert_eq!(encode_i24(2.0), 8_388_607);
        assert_eq!(encode_i24(-1.0), -8_388_608);
        assert_eq!(encode_i24(-3.0), -8_388_608);
        assert_eq!(encode_i24(f32::NAN), 0);
    }

    #[test]
    fn pcm16_stays_within_one_lsb_of_input() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.wav");
        let samples: Vec<f32> = (0..1000).map(|i| ((i as f32) * 0.01).sin() * 0.8).collect();
        let mut w = WavWriter::create(&path, spec(1, SampleFormat::Pcm16), &bext(), 3).unwrap();
        w.write_samples(&samples).unwrap();
        w.finalize().unwrap();

        let data = data_of(&path);
        for (b, x) in data.as_chunks::<2>().0.iter().zip(&samples) {
            let v = i16::from_le_bytes(*b) as f32;
            assert!((v - x * SCALE_16).abs() <= 1.5, "{v} vs {}", x * SCALE_16);
        }
    }

    #[test]
    fn float32_keeps_values_above_full_scale_and_has_fact() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.wav");
        let samples = [0.1f32, -1.5, 2.25, 0.0];
        let mut w = WavWriter::create(&path, spec(2, SampleFormat::Float32), &bext(), 0).unwrap();
        w.write_samples(&samples).unwrap();
        w.finalize().unwrap();

        let s = scan_path(&path);
        assert_eq!((s.format_tag, s.bits, s.block_align), (3, 32, 8));
        let bytes = fs::read(&path).unwrap();
        assert_eq!(u32_at(&bytes, s.layout.fact_pos.unwrap()), 2);
        let data = data_of(&path);
        let decoded: Vec<f32> = data
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| f32::from_le_bytes(*b))
            .collect();
        assert_eq!(decoded, samples);
    }

    #[test]
    fn odd_data_length_gets_a_pad_byte() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.wav");
        let mut w = WavWriter::create(&path, spec(1, SampleFormat::Pcm24), &bext(), 0).unwrap();
        w.write_samples(&[0.1]).unwrap();
        w.finalize().unwrap();
        let bytes = fs::read(&path).unwrap();
        let s = scan_path(&path);
        assert_eq!(u32_at(&bytes, s.layout.data_size_pos), 3);
        assert_eq!(bytes.len() as u64, s.layout.data_start + 4);
        assert_eq!(u32_at(&bytes, 4) as usize, bytes.len() - 8);
    }

    #[test]
    fn flushed_header_is_valid_while_still_recording() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.wav");
        let mut w = WavWriter::create(&path, spec(1, SampleFormat::Pcm24), &bext(), 0).unwrap();
        w.write_samples(&[0.1, 0.2]).unwrap();
        w.flush_header().unwrap();
        assert_eq!(data_of(&path).len(), 6);
        w.write_samples(&[0.3, 0.4]).unwrap();
        w.flush_header().unwrap();
        assert_eq!(data_of(&path).len(), 12);
        assert_eq!(w.frames_written(), 4);
    }

    #[test]
    fn switches_to_rf64_past_threshold() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.wav");
        let mut w = WavWriter::create(&path, spec(1, SampleFormat::Pcm24), &bext(), 0).unwrap();
        w.set_rf64_threshold(1000);
        w.write_samples(&vec![0.25; 400]).unwrap();
        w.finalize().unwrap();

        let bytes = fs::read(&path).unwrap();
        let s = scan_path(&path);
        assert!(s.rf64);
        assert_eq!(&bytes[12..16], b"ds64");
        assert_eq!(u32_at(&bytes, 4), u32::MAX);
        assert_eq!(u32_at(&bytes, s.layout.data_size_pos), u32::MAX);
        assert_eq!(u64_at(&bytes, 20), bytes.len() as u64 - 8);
        assert_eq!(u64_at(&bytes, 28), 1200);
        assert_eq!(u64_at(&bytes, 36), 400);
        assert_eq!(data_of(&path).len(), 1200);
    }

    #[test]
    fn create_refuses_to_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.wav");
        fs::write(&path, b"existing").unwrap();
        assert!(WavWriter::create(&path, spec(1, SampleFormat::Pcm24), &bext(), 0).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"existing");
    }

    #[test]
    fn repair_recovers_data_written_after_last_header_update() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.wav");
        let mut w = WavWriter::create(&path, spec(2, SampleFormat::Pcm24), &bext(), 0).unwrap();
        w.write_samples(&[0.1, 0.2]).unwrap();
        w.flush_header().unwrap();
        w.write_samples(&[0.3, 0.4, 0.5, 0.6]).unwrap();
        w.sync().unwrap();
        // Simulate a crash: the final header update never happens.
        w.finalized = true;
        drop(w);
        // A torn write leaves half a frame at the end.
        let mut f = OpenOptions::new().append(true).open(&path).unwrap();
        f.write_all(&[1, 2, 3, 4]).unwrap();
        drop(f);
        assert_eq!(data_of(&path).len(), 6);

        let r = repair(&path).unwrap();
        assert_eq!(
            r,
            Repaired {
                frames: 3,
                data_bytes: 18
            }
        );
        let bytes = fs::read(&path).unwrap();
        assert_eq!(data_of(&path).len(), 18);
        assert_eq!(u32_at(&bytes, 4) as usize, bytes.len() - 8);
    }

    #[test]
    fn repair_rejects_non_wav_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.wav");
        fs::write(&path, b"hello world, not a wav").unwrap();
        assert!(repair(&path).is_err());
    }

    #[test]
    fn dropping_an_unfinalized_writer_finalizes_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.wav");
        let mut w = WavWriter::create(&path, spec(1, SampleFormat::Pcm24), &bext(), 0).unwrap();
        w.write_samples(&[0.1, 0.2, 0.3]).unwrap();
        drop(w);
        assert_eq!(data_of(&path).len(), 9);
    }
}
