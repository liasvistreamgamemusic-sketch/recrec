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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Track {
    pub name: String,
    pub source: Source,
    pub armed: bool,
}

impl Track {
    pub fn mono(channel: u16) -> Self {
        Self {
            name: format!("In{}", channel + 1),
            source: Source::Mono(channel),
            armed: true,
        }
    }
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
            Track {
                name: "Synth".into(),
                source: Source::Stereo(1, 2),
                armed: true,
            },
        ];
        let kept = retain_valid(&tracks, 3);
        assert_eq!(kept.len(), 2);
        assert_eq!(kept[1].name, "Synth");
    }

    #[test]
    fn labels_are_one_based() {
        assert_eq!(Source::Mono(0).label(), "In 1");
        assert_eq!(Source::Stereo(2, 3).label(), "In 3+4 (ステレオ)");
    }
}
