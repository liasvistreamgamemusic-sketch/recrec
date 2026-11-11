//! Track definitions: which input channels go into which file.

use serde::{Deserialize, Serialize};

/// Input channels feeding a track (0-based device channel indices).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Source {
    Mono(u16),
    Stereo(u16, u16),
}

impl Source {
    pub fn channels(&self) -> Vec<usize> {
        match *self {
            Self::Mono(c) => vec![usize::from(c)],
            Self::Stereo(l, r) => vec![usize::from(l), usize::from(r)],
        }
    }

    pub fn width(&self) -> u16 {
        match self {
            Self::Mono(_) => 1,
            Self::Stereo(..) => 2,
        }
    }

    /// Whether every channel exists on a device with `device_channels` inputs.
    pub fn fits(&self, device_channels: u16) -> bool {
        self.channels()
            .iter()
            .all(|&c| c < usize::from(device_channels))
    }

    /// Human-facing label with 1-based channel numbers.
    pub fn label(&self) -> String {
        match *self {
            Self::Mono(c) => format!("In {}", c + 1),
            Self::Stereo(l, r) => format!("In {}+{} (ステレオ)", l + 1, r + 1),
        }
    }

    /// Every mono input followed by every adjacent stereo pair.
    pub fn options(device_channels: u16) -> Vec<Source> {
        let mono = (0..device_channels).map(Self::Mono);
        let stereo = (1..device_channels).map(|r| Self::Stereo(r - 1, r));
        mono.chain(stereo).collect()
    }
}

pub const MIN_VOLUME_DB: f32 = -60.0;
pub const MAX_VOLUME_DB: f32 = 12.0;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Track {
    pub name: String,
    pub source: Source,
    /// Whether the track is written to disk.
    pub armed: bool,
    /// Whether the track is heard through the app (software monitoring).
    #[serde(default)]
    pub monitor: bool,
    #[serde(default)]
    pub mute: bool,
    #[serde(default)]
    pub solo: bool,
    /// Monitor level in dB; never applied to the recording.
    #[serde(default)]
    pub volume_db: f32,
    /// Monitor pan, -1 (left) to 1 (right).
    #[serde(default)]
    pub pan: f32,
    /// Marks a loopback input (records what the computer plays, the metronome included).
    #[serde(default)]
    pub loopback: bool,
}

impl Track {
    pub fn new(name: String, source: Source) -> Self {
        Self {
            name,
            source,
            armed: true,
            monitor: false,
            mute: false,
            solo: false,
            volume_db: 0.0,
            pan: 0.0,
            loopback: false,
        }
    }

    pub fn mono(channel: u16) -> Self {
        Self::new(format!("In{}", channel + 1), Source::Mono(channel))
    }

    fn audible(&self, any_solo: bool) -> bool {
        self.monitor && !self.mute && (!any_solo || self.solo)
    }
}

/// Monitor gain `(left, right)` of every input channel for the current track settings.
/// A channel used by several audible tracks is heard once per track.
pub fn monitor_gains(tracks: &[Track], in_channels: usize) -> Vec<(f32, f32)> {
    let mut gains = vec![(0.0f32, 0.0f32); in_channels];
    let any_solo = tracks.iter().any(|t| t.monitor && t.solo);
    for t in tracks.iter().filter(|t| t.audible(any_solo)) {
        let volume = 10f32.powf(t.volume_db.clamp(MIN_VOLUME_DB, MAX_VOLUME_DB) / 20.0);
        let pan = t.pan.clamp(-1.0, 1.0);
        let routes = match t.source {
            Source::Mono(c) => {
                // Equal-power pan, scaled so the center position plays at unity on both sides.
                let angle = (pan + 1.0) * std::f32::consts::FRAC_PI_4;
                let norm = std::f32::consts::SQRT_2;
                vec![(c, angle.cos() * norm, angle.sin() * norm)]
            }
            // Stereo sources keep their sides; pan acts as a balance control.
            Source::Stereo(l, r) => vec![
                (l, (1.0 - pan).min(1.0), 0.0),
                (r, 0.0, (1.0 + pan).min(1.0)),
            ],
        };
        for (channel, left, right) in routes {
            if let Some(gain) = gains.get_mut(usize::from(channel)) {
                gain.0 += left * volume;
                gain.1 += right * volume;
            }
        }
    }
    gains
}

/// One armed mono track per input channel.
pub fn default_tracks(device_channels: u16) -> Vec<Track> {
    (0..device_channels).map(Track::mono).collect()
}

/// Drops tracks that reference channels the device does not have.
pub fn retain_valid(tracks: &[Track], device_channels: u16) -> Vec<Track> {
    tracks
        .iter()
        .filter(|t| t.source.fits(device_channels))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_one_armed_mono_track_per_input() {
        let tracks = default_tracks(4);
        assert_eq!(tracks.len(), 4);
        assert_eq!(tracks[1].name, "In2");
        assert_eq!(tracks[1].source, Source::Mono(1));
        assert!(tracks.iter().all(|t| t.armed));
    }

    #[test]
    fn options_include_mono_and_adjacent_pairs() {
        let opts = Source::options(3);
        assert_eq!(
            opts,
            vec![
                Source::Mono(0),
                Source::Mono(1),
                Source::Mono(2),
                Source::Stereo(0, 1),
                Source::Stereo(1, 2),
            ]
        );
    }

    #[test]
    fn retain_valid_removes_out_of_range_tracks() {
        let tracks = vec![
            Track::mono(0),
            Track::mono(3),
            Track::new("Synth".into(), Source::Stereo(1, 2)),
        ];
        let kept = retain_valid(&tracks, 3);
        assert_eq!(kept.len(), 2);
        assert_eq!(kept[1].name, "Synth");
    }

    fn close(a: (f32, f32), b: (f32, f32)) -> bool {
        (a.0 - b.0).abs() < 1e-4 && (a.1 - b.1).abs() < 1e-4
    }

    #[test]
    fn nothing_is_heard_until_a_track_is_monitored() {
        let tracks = default_tracks(2);
        assert_eq!(monitor_gains(&tracks, 2), vec![(0.0, 0.0); 2]);
    }

    #[test]
    fn mono_tracks_are_centered_and_follow_pan_and_volume() {
        let mut t = Track::mono(1);
        t.monitor = true;
        assert!(close(monitor_gains(&[t.clone()], 2)[1], (1.0, 1.0)));
        t.pan = -1.0;
        assert!(close(
            monitor_gains(&[t.clone()], 2)[1],
            (std::f32::consts::SQRT_2, 0.0)
        ));
        t.pan = 0.0;
        t.volume_db = -6.0206;
        assert!(close(monitor_gains(&[t], 2)[1], (0.5, 0.5)));
    }

    #[test]
    fn stereo_tracks_keep_their_sides() {
        let mut t = Track::new("Synth".into(), Source::Stereo(2, 3));
        t.monitor = true;
        let g = monitor_gains(&[t.clone()], 4);
        assert!(close(g[2], (1.0, 0.0)) && close(g[3], (0.0, 1.0)));
        t.pan = 1.0;
        let g = monitor_gains(&[t], 4);
        assert!(close(g[2], (0.0, 0.0)) && close(g[3], (0.0, 1.0)));
    }

    #[test]
    fn mute_and_solo_decide_what_is_audible() {
        let mut a = Track::mono(0);
        let mut b = Track::mono(1);
        a.monitor = true;
        b.monitor = true;
        b.mute = true;
        let g = monitor_gains(&[a.clone(), b.clone()], 2);
        assert!(close(g[0], (1.0, 1.0)) && close(g[1], (0.0, 0.0)));
        b.mute = false;
        b.solo = true;
        let g = monitor_gains(&[a.clone(), b.clone()], 2);
        assert!(close(g[0], (0.0, 0.0)) && close(g[1], (1.0, 1.0)));
        // Solo on a track that is not monitored silences nobody.
        b.monitor = false;
        assert!(close(monitor_gains(&[a, b], 2)[0], (1.0, 1.0)));
    }

    #[test]
    fn old_configs_without_the_new_fields_still_load() {
        let t: Track =
            serde_json::from_str(r#"{"name":"A","source":{"Mono":0},"armed":true}"#).unwrap();
        let mut expected = Track::mono(0);
        expected.name = "A".into();
        assert_eq!(t, expected);
    }

    #[test]
    fn labels_are_one_based() {
        assert_eq!(Source::Mono(0).label(), "In 1");
        assert_eq!(Source::Stereo(2, 3).label(), "In 3+4 (ステレオ)");
    }
}
