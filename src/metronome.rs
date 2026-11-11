//! Metronome timing and click synthesis. Everything is a pure function of the frame position,
//! so clicks land sample-accurately and the audio callback keeps no scheduling state.

use std::f64::consts::TAU;
use std::time::Instant;

const CLICK_SECONDS: f64 = 0.09;
const ATTACK_SECONDS: f64 = 0.0005;
/// Downbeat and other beats (G6 / C6): a fourth apart so the bar start is easy to hear.
const ACCENT_HZ: f64 = 1567.98;
const BEAT_HZ: f64 = 1046.50;
const BODY_DECAY: f64 = 0.016;
/// An inharmonic upper partial gives the click a woodblock-like attack.
const PARTIAL_RATIO: f64 = 2.76;
const PARTIAL_LEVEL: f64 = 0.35;
const PARTIAL_DECAY: f64 = 0.005;
const PEAK: f64 = 0.7;
const TAP_WINDOW: usize = 4;
const TAP_TIMEOUT_SECS: f64 = 2.0;

/// Length of one beat in frames (fractional, so long takes do not drift).
pub fn beat_frames(sample_rate: u32, bpm: f64, beat_unit: u32) -> f64 {
    f64::from(sample_rate) * 60.0 / bpm * 4.0 / f64::from(beat_unit)
}

/// Where a frame falls relative to the metronome origin.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BeatPosition {
    /// 0-based bar.
    pub bar: u64,
    /// 0-based beat inside the bar.
    pub beat: u32,
    /// Frames since the beat started.
    pub offset: f64,
}

pub fn position(frames_since_origin: u64, beat_frames: f64, beats_per_bar: u32) -> BeatPosition {
    let beats = (frames_since_origin as f64 / beat_frames).floor();
    let index = beats as u64;
    let per_bar = u64::from(beats_per_bar.max(1));
    BeatPosition {
        bar: index / per_bar,
        beat: (index % per_bar) as u32,
        offset: frames_since_origin as f64 - beats * beat_frames,
    }
}

/// Click waveform `offset` frames after a beat started; 0 once the click has rung out.
pub fn click_sample(offset: f64, sample_rate: u32, accent: bool) -> f32 {
    let t = offset / f64::from(sample_rate);
    if !(0.0..CLICK_SECONDS).contains(&t) {
        return 0.0;
    }
    let hz = if accent { ACCENT_HZ } else { BEAT_HZ };
    let attack = if t < ATTACK_SECONDS {
        0.5 - 0.5 * (std::f64::consts::PI * t / ATTACK_SECONDS).cos()
    } else {
        1.0
    };
    // Fade the tail to exactly zero so the click never ends on a step.
    let tail = 1.0 - (t / CLICK_SECONDS).powi(4);
    let body = (TAU * hz * t).sin() * (-t / BODY_DECAY).exp();
    let partial = (TAU * hz * PARTIAL_RATIO * t).sin() * PARTIAL_LEVEL * (-t / PARTIAL_DECAY).exp();
    let level = if accent { PEAK } else { PEAK * 0.8 };
    ((body + partial) * attack * tail * level) as f32
}

/// The metronome signal at `frame` of the output timeline (silent before `origin`).
pub fn sample_at(
    frame: u64,
    origin: u64,
    sample_rate: u32,
    bpm: f64,
    beats_per_bar: u32,
    beat_unit: u32,
) -> f32 {
    let Some(since) = frame.checked_sub(origin) else {
        return 0.0;
    };
    let pos = position(
        since,
        beat_frames(sample_rate, bpm, beat_unit),
        beats_per_bar,
    );
    click_sample(pos.offset, sample_rate, pos.beat == 0)
}

/// Tap-tempo: averages the gaps between the most recent taps.
#[derive(Debug, Default)]
pub struct TapTempo {
    taps: Vec<Instant>,
}

impl TapTempo {
    /// Registers a tap; returns the tempo once at least two taps are in the window.
    pub fn tap(&mut self, now: Instant) -> Option<f64> {
        if self
            .taps
            .last()
            .is_some_and(|last| (now - *last).as_secs_f64() > TAP_TIMEOUT_SECS)
        {
            self.taps.clear();
        }
        self.taps.push(now);
        if self.taps.len() > TAP_WINDOW + 1 {
            self.taps.remove(0);
        }
        let first = *self.taps.first()?;
        let gaps = self.taps.len() - 1;
        if gaps == 0 {
            return None;
        }
        let mean = (now - first).as_secs_f64() / gaps as f64;
        (mean > 0.0).then(|| 60.0 / mean)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn beat_length_follows_tempo_and_unit() {
        assert_eq!(beat_frames(48_000, 120.0, 4), 24_000.0);
        assert_eq!(beat_frames(48_000, 120.0, 8), 12_000.0);
        assert_eq!(beat_frames(44_100, 60.0, 4), 44_100.0);
    }

    #[test]
    fn position_counts_bars_and_beats() {
        let b = 24_000.0;
        assert_eq!(
            position(0, b, 4),
            BeatPosition {
                bar: 0,
                beat: 0,
                offset: 0.0
            }
        );
        assert_eq!(
            position(24_010, b, 4),
            BeatPosition {
                bar: 0,
                beat: 1,
                offset: 10.0
            }
        );
        assert_eq!(
            position(4 * 24_000, b, 4),
            BeatPosition {
                bar: 1,
                beat: 0,
                offset: 0.0
            }
        );
        assert_eq!(position(7 * 24_000 + 5, b, 3).bar, 2);
        assert_eq!(position(7 * 24_000 + 5, b, 3).beat, 1);
    }

    #[test]
    fn click_starts_at_zero_and_ends_silent() {
        assert_eq!(click_sample(0.0, 48_000, true), 0.0);
        assert!(click_sample(48.0, 48_000, true).abs() > 0.05);
        assert_eq!(click_sample(0.09 * 48_000.0, 48_000, true), 0.0);
        assert_eq!(click_sample(-1.0, 48_000, false), 0.0);
        let last = click_sample(0.09 * 48_000.0 - 1.0, 48_000, false);
        assert!(last.abs() < 1e-3, "{last}");
    }

    #[test]
    fn click_never_exceeds_full_scale() {
        for accent in [true, false] {
            let peak = (0..4320)
                .map(|i| click_sample(f64::from(i), 48_000, accent).abs())
                .fold(0.0, f32::max);
            assert!(peak > 0.3 && peak < 1.0, "{peak}");
        }
    }

    #[test]
    fn clicks_land_exactly_on_beats_after_the_origin() {
        let sr = 48_000;
        let at = |frame| sample_at(frame, 1000, sr, 120.0, 4, 4);
        assert_eq!(at(999), 0.0);
        assert_eq!(at(1000), 0.0); // first sample of a click is the zero crossing
        assert!(at(1000 + 30).abs() > 0.05);
        assert_eq!(at(1000 + 12_000), 0.0); // between beats
        assert!(at(1000 + 24_000 + 30).abs() > 0.05);
        // Downbeat and off-beat use different pitches.
        assert_ne!(at(1000 + 30), at(1000 + 24_000 + 30));
        assert_eq!(at(1000 + 30), at(1000 + 4 * 24_000 + 30));
    }

    #[test]
    fn tap_tempo_averages_recent_gaps() {
        let mut tap = TapTempo::default();
        let t0 = Instant::now();
        assert_eq!(tap.tap(t0), None);
        let bpm = tap.tap(t0 + Duration::from_millis(500)).unwrap();
        assert!((bpm - 120.0).abs() < 1e-6);
        let bpm = tap.tap(t0 + Duration::from_millis(1000)).unwrap();
        assert!((bpm - 120.0).abs() < 1e-6);
        // A long pause starts a new measurement.
        assert_eq!(tap.tap(t0 + Duration::from_secs(10)), None);
    }
}
