//! Level metering: lock-free peak collection on the audio thread and ballistics for display.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

/// |x| at or above this counts as a clip (about -0.01 dBFS).
pub const CLIP_THRESHOLD: f32 = 0.999;
/// Lowest level the meters show.
pub const METER_FLOOR_DB: f32 = -60.0;
const DECAY_DB_PER_SEC: f32 = 20.0;
const PEAK_HOLD_SECS: f64 = 1.5;

/// Per-channel peak and clip state shared between the audio callback and the UI.
///
/// Peaks are stored as f32 bit patterns: for non-negative floats the bit pattern orders the
/// same way as the value, so `fetch_max` on the integer keeps the larger peak.
#[derive(Debug)]
pub struct MeterBank {
    peaks: Vec<AtomicU32>,
    clips: Vec<AtomicBool>,
}

impl MeterBank {
    pub fn new(channels: usize) -> Self {
        Self {
            peaks: (0..channels).map(|_| AtomicU32::new(0)).collect(),
            clips: (0..channels).map(|_| AtomicBool::new(false)).collect(),
        }
    }

    pub fn channels(&self) -> usize {
        self.peaks.len()
    }

    /// Records a block peak (absolute value). Real-time safe.
    #[inline]
    pub fn record(&self, channel: usize, peak_abs: f32) {
        // NaN and negatives would break the bit-order trick.
        if peak_abs.is_nan() || peak_abs <= 0.0 {
            return;
        }
        self.peaks[channel].fetch_max(peak_abs.to_bits(), Ordering::Relaxed);
        if peak_abs >= CLIP_THRESHOLD {
            self.clips[channel].store(true, Ordering::Relaxed);
        }
    }

    /// Returns the highest peak since the previous call and resets it.
    pub fn take_peak(&self, channel: usize) -> f32 {
        f32::from_bits(self.peaks[channel].swap(0, Ordering::Relaxed))
    }

    pub fn clipped(&self, channel: usize) -> bool {
        self.clips[channel].load(Ordering::Relaxed)
    }

    pub fn reset_clip(&self, channel: usize) {
        self.clips[channel].store(false, Ordering::Relaxed);
    }
}

pub fn to_db(linear: f32) -> f32 {
    if linear > 0.0 {
        20.0 * linear.log10()
    } else {
        f32::NEG_INFINITY
    }
}

/// Display ballistics for one meter: instant attack, linear dB decay, timed peak hold.
#[derive(Debug, Clone, Copy)]
pub struct MeterDisplay {
    pub level_db: f32,
    pub hold_db: f32,
    hold_since: f64,
}

impl Default for MeterDisplay {
    fn default() -> Self {
        Self {
            level_db: METER_FLOOR_DB,
            hold_db: METER_FLOOR_DB,
            hold_since: 0.0,
        }
    }
}

impl MeterDisplay {
    /// `now` is a monotonic time in seconds, `dt` the time since the previous update.
    pub fn update(&mut self, peak_linear: f32, now: f64, dt: f32) {
        let db = to_db(peak_linear).max(METER_FLOOR_DB);
        let decayed = (self.level_db - DECAY_DB_PER_SEC * dt).max(METER_FLOOR_DB);
        self.level_db = db.max(decayed);
        if self.level_db >= self.hold_db || now - self.hold_since > PEAK_HOLD_SECS {
            self.hold_db = self.level_db;
            self.hold_since = now;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn take_peak_returns_max_and_resets() {
        let m = MeterBank::new(2);
        m.record(0, 0.25);
        m.record(0, 0.5);
        m.record(0, 0.1);
        assert_eq!(m.take_peak(0), 0.5);
        assert_eq!(m.take_peak(0), 0.0);
        assert_eq!(m.take_peak(1), 0.0);
    }

    #[test]
    fn nan_is_ignored() {
        let m = MeterBank::new(1);
        m.record(0, f32::NAN);
        assert_eq!(m.take_peak(0), 0.0);
        assert!(!m.clipped(0));
    }

    #[test]
    fn clip_latches_until_reset() {
        let m = MeterBank::new(1);
        m.record(0, 0.5);
        assert!(!m.clipped(0));
        m.record(0, 1.0);
        m.take_peak(0);
        assert!(m.clipped(0));
        m.reset_clip(0);
        assert!(!m.clipped(0));
    }

    #[test]
    fn db_conversion() {
        assert!((to_db(1.0)).abs() < 1e-6);
        assert!((to_db(0.5) + 6.0206).abs() < 1e-3);
        assert_eq!(to_db(0.0), f32::NEG_INFINITY);
    }

    #[test]
    fn display_decays_and_holds() {
        let mut d = MeterDisplay::default();
        d.update(1.0, 0.0, 0.0);
        assert_eq!(d.level_db, 0.0);
        d.update(0.0, 0.5, 0.5);
        assert!((d.level_db + 10.0).abs() < 1e-4);
        assert_eq!(d.hold_db, 0.0);
        d.update(0.0, 2.0, 1.5);
        assert!((d.level_db + 40.0).abs() < 1e-4);
        assert_eq!(d.hold_db, d.level_db);
        d.update(0.0, 10.0, 8.0);
        assert_eq!(d.level_db, METER_FLOOR_DB);
    }
}
