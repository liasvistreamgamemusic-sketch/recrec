//! State shared between the audio callbacks, the writer thread and the UI (all lock-free).

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

/// Tempo is stored as hundredths of a BPM so it fits an integer atomic.
const BPM_SCALE: f64 = 100.0;
pub const MIN_BPM: f64 = 30.0;
pub const MAX_BPM: f64 = 300.0;
/// "No origin": the metronome is silent until an origin is set.
const NO_ORIGIN: u64 = u64::MAX;

fn store_f32(a: &AtomicU32, v: f32) {
    a.store(v.to_bits(), Ordering::Relaxed);
}

fn load_f32(a: &AtomicU32) -> f32 {
    f32::from_bits(a.load(Ordering::Relaxed))
}

/// Left/right monitor gain of one input channel.
#[derive(Debug, Default)]
pub struct ChannelGain {
    left: AtomicU32,
    right: AtomicU32,
}

#[derive(Debug)]
pub struct Transport {
    /// Frames the input callback has pushed into the record ring (the recorder's timeline).
    in_frames: AtomicU64,
    /// Frames the output callback has rendered (the metronome's timeline).
    out_frames: AtomicU64,
    /// Output frame of bar 1, beat 1.
    origin_out: AtomicU64,
    bpm: AtomicU32,
    beats_per_bar: AtomicU32,
    /// Note value of one beat: 4 (quarter) or 8 (eighth).
    beat_unit: AtomicU32,
    click_on: AtomicBool,
    click_gain: AtomicU32,
    master_gain: AtomicU32,
    gains: Box<[ChannelGain]>,
}

impl Transport {
    pub fn new(in_channels: usize) -> Self {
        let t = Self {
            in_frames: AtomicU64::new(0),
            out_frames: AtomicU64::new(0),
            origin_out: AtomicU64::new(NO_ORIGIN),
            bpm: AtomicU32::new(0),
            beats_per_bar: AtomicU32::new(4),
            beat_unit: AtomicU32::new(4),
            click_on: AtomicBool::new(false),
            click_gain: AtomicU32::new(0),
            master_gain: AtomicU32::new(0),
            gains: (0..in_channels).map(|_| ChannelGain::default()).collect(),
        };
        t.set_tempo(120.0, 4, 4);
        t.set_master_gain(1.0);
        t
    }

    pub fn in_channels(&self) -> usize {
        self.gains.len()
    }

    pub fn add_in_frames(&self, frames: u64) {
        self.in_frames.fetch_add(frames, Ordering::Relaxed);
    }

    pub fn in_frames(&self) -> u64 {
        self.in_frames.load(Ordering::Relaxed)
    }

    pub fn add_out_frames(&self, frames: u64) {
        self.out_frames.fetch_add(frames, Ordering::Relaxed);
    }

    pub fn out_frames(&self) -> u64 {
        self.out_frames.load(Ordering::Relaxed)
    }

    pub fn set_tempo(&self, bpm: f64, beats_per_bar: u32, beat_unit: u32) {
        let bpm = bpm.clamp(MIN_BPM, MAX_BPM);
        self.bpm
            .store((bpm * BPM_SCALE).round() as u32, Ordering::Relaxed);
        self.beats_per_bar
            .store(beats_per_bar.max(1), Ordering::Relaxed);
        self.beat_unit.store(beat_unit.max(1), Ordering::Relaxed);
    }

    pub fn bpm(&self) -> f64 {
        f64::from(self.bpm.load(Ordering::Relaxed)) / BPM_SCALE
    }

    pub fn beats_per_bar(&self) -> u32 {
        self.beats_per_bar.load(Ordering::Relaxed)
    }

    pub fn beat_unit(&self) -> u32 {
        self.beat_unit.load(Ordering::Relaxed)
    }

    /// Sets the output frame at which bar 1 starts.
    pub fn set_origin_out(&self, frame: u64) {
        self.origin_out.store(frame, Ordering::Relaxed);
    }

    pub fn origin_out(&self) -> Option<u64> {
        Some(self.origin_out.load(Ordering::Relaxed)).filter(|f| *f != NO_ORIGIN)
    }

    pub fn set_click(&self, on: bool, gain: f32) {
        self.click_on.store(on, Ordering::Relaxed);
        store_f32(&self.click_gain, gain);
    }

    pub fn click_on(&self) -> bool {
        self.click_on.load(Ordering::Relaxed)
    }

    pub fn click_gain(&self) -> f32 {
        load_f32(&self.click_gain)
    }

    pub fn set_master_gain(&self, gain: f32) {
        store_f32(&self.master_gain, gain);
    }

    pub fn master_gain(&self) -> f32 {
        load_f32(&self.master_gain)
    }

    /// Sets the monitor gains of every input channel (`gains[ch] = (left, right)`).
    pub fn set_channel_gains(&self, gains: &[(f32, f32)]) {
        for (slot, (l, r)) in self.gains.iter().zip(gains) {
            store_f32(&slot.left, *l);
            store_f32(&slot.right, *r);
        }
    }

    pub fn channel_gain(&self, channel: usize) -> (f32, f32) {
        let g = &self.gains[channel];
        (load_f32(&g.left), load_f32(&g.right))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tempo_is_clamped_and_roundtrips() {
        let t = Transport::new(2);
        t.set_tempo(123.45, 3, 8);
        assert_eq!(t.bpm(), 123.45);
        assert_eq!((t.beats_per_bar(), t.beat_unit()), (3, 8));
        t.set_tempo(1000.0, 0, 4);
        assert_eq!(t.bpm(), MAX_BPM);
        assert_eq!(t.beats_per_bar(), 1);
    }

    #[test]
    fn origin_is_unset_until_stored() {
        let t = Transport::new(1);
        assert_eq!(t.origin_out(), None);
        t.set_origin_out(480);
        assert_eq!(t.origin_out(), Some(480));
    }

    #[test]
    fn channel_gains_roundtrip() {
        let t = Transport::new(2);
        assert_eq!(t.channel_gain(1), (0.0, 0.0));
        t.set_channel_gains(&[(0.5, 0.25), (1.0, 0.0)]);
        assert_eq!(t.channel_gain(0), (0.5, 0.25));
        assert_eq!(t.channel_gain(1), (1.0, 0.0));
    }
}
