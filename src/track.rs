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

    /// Label built from the driver's channel names (see [`fallback_names`]).
    pub fn label_with(&self, names: &[String]) -> String {
        let name = |c: u16| {
            names
                .get(usize::from(c))
                .cloned()
                .unwrap_or_else(|| format!("In {}", c + 1))
        };
        match *self {
            Self::Mono(c) => name(c),
            Self::Stereo(l, r) => format!("{} + {} (ステレオ)", name(l), name(r)),
        }
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

/// Channel names used when the driver does not report any: "In 1", "In 2", …
pub fn fallback_names(device_channels: u16) -> Vec<String> {
    (1..=device_channels).map(|n| format!("In {n}")).collect()
}

/// Whether a driver channel name denotes a loopback input (it carries what the computer
/// plays instead of a physical input).
pub fn is_loopback_name(name: &str) -> bool {
    name.to_lowercase().contains("loop")
}

/// One armed mono track per physical input. Loopback inputs get no track (they can be added
/// by hand); if every input looks like a loopback, all of them are used.
pub fn default_tracks_for(names: &[String]) -> Vec<Track> {
    let physical = |name: &&String| !is_loopback_name(name);
    let keep_all = !names.iter().any(|n| physical(&n));
    names
        .iter()
        .enumerate()
        .filter(|(_, name)| keep_all || physical(name))
        .map(|(c, name)| {
            let mut t = Track::mono(c as u16);
            t.name = name.split_whitespace().collect();
            t.loopback = is_loopback_name(name);
            t
        })
        .collect()
}

/// Sets the loopback mark on tracks whose source includes a loopback-named channel.
/// Returns true when a mark was added.
pub fn mark_loopback(tracks: &mut [Track], names: &[String]) -> bool {
    let mut changed = false;
    for t in tracks.iter_mut().filter(|t| !t.loopback) {
        let is_loopback = t
            .source
            .channels()
            .iter()
            .any(|c| names.get(*c).is_some_and(|n| is_loopback_name(n)));
        if is_loopback {
            t.loopback = true;
            changed = true;
        }
    }
    changed
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

    fn motu_names() -> Vec<String> {
        [
            "In 1",
            "In 2",
            "In 3",
            "In 4",
            "Loopback 1",
            "Loopback 2",
            "Loop Mix 1",
            "Loop Mix 2",
        ]
        .map(String::from)
        .to_vec()
    }

    #[test]
    fn default_tracks_skip_loopback_inputs() {
        let tracks = default_tracks_for(&motu_names());
        let names: Vec<&str> = tracks.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["In1", "In2", "In3", "In4"]);
        assert_eq!(tracks[3].source, Source::Mono(3));
        assert!(tracks.iter().all(|t| t.armed && !t.loopback));
    }

    #[test]
    fn default_tracks_keep_everything_when_all_inputs_are_loopback() {
        let names = vec!["Loopback L".to_owned(), "Loopback R".to_owned()];
        let tracks = default_tracks_for(&names);
        assert_eq!(tracks.len(), 2);
        assert!(tracks.iter().all(|t| t.loopback));
    }

    #[test]
    fn fallback_names_give_the_same_tracks_as_before() {
        assert_eq!(default_tracks_for(&fallback_names(3)), default_tracks(3));
    }

    #[test]
    fn existing_tracks_on_loopback_channels_get_marked() {
        let mut tracks = vec![
            Track::mono(0),
            Track::mono(4),
            Track::new("Mix".into(), Source::Stereo(6, 7)),
        ];
        assert!(mark_loopback(&mut tracks, &motu_names()));
        assert_eq!(
            tracks.iter().map(|t| t.loopback).collect::<Vec<_>>(),
            [false, true, true]
        );
        assert!(!mark_loopback(&mut tracks, &motu_names()));
    }

    #[test]
    fn labels_use_driver_names_when_given() {
        let names = motu_names();
        assert_eq!(Source::Mono(4).label_with(&names), "Loopback 1");
        assert_eq!(
            Source::Stereo(2, 3).label_with(&names),
            "In 3 + In 4 (ステレオ)"
        );
        assert_eq!(Source::Mono(9).label_with(&names), "In 10");
    }

    #[test]
    fn labels_are_one_based() {
        assert_eq!(Source::Mono(0).label(), "In 1");
        assert_eq!(Source::Stereo(2, 3).label(), "In 3+4 (ステレオ)");
    }
}
