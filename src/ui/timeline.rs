//! The take opened for review: its waveform overview, the visible window and the playhead.

use std::path::{Path, PathBuf};

use crate::peaks::TakePeaks;
use crate::take::{self, TakeMeta};

/// Closest zoom: this many frames per pixel (about 1 ms per pixel at 48 kHz).
const MIN_FRAMES_PER_PX: f64 = 48.0;

pub(super) struct OpenTake {
    pub dir: PathBuf,
    pub name: String,
    pub meta: TakeMeta,
    /// One file per take track, in `meta.tracks` order.
    pub files: Vec<PathBuf>,
    pub peaks: TakePeaks,
    /// Index of the first overview lane of each take track.
    lane_start: Vec<usize>,
    pub frames: u64,
    /// Left edge of the view, in frames (may be fractional while scrolling).
    pub view_start: f64,
    /// Zoom; `None` until the lane width is known, then "whole take".
    pub frames_per_px: Option<f64>,
    /// Where playback starts (and where it stopped).
    pub playhead: u64,
}

impl OpenTake {
    /// Reads the take's metadata and waveform overview (building the overview from the
    /// audio when it was not stored).
    pub fn load(dir: &Path) -> Result<Self, String> {
        let meta = take::read_meta(dir).map_err(|e| format!("take.json を読めません: {e}"))?;
        let files: Vec<PathBuf> = meta.tracks.iter().map(|t| dir.join(&t.file)).collect();
        let lanes: usize = meta.tracks.iter().map(|t| t.inputs.len()).sum();
        let peaks = match TakePeaks::load(dir) {
            Ok(p) if p.lanes.len() == lanes => p,
            _ => {
                let built = TakePeaks::build(&files, meta.sample_rate)
                    .map_err(|e| format!("テイクの音声を読めません: {e}"))?;
                // Storing it is only a speed-up for next time.
                if let Err(e) = built.save(dir) {
                    log::warn!("could not store waveform overview: {e}");
                }
                built
            }
        };
        let mut next = 0;
        let lane_start = meta
            .tracks
            .iter()
            .map(|t| {
                let start = next;
                next += t.inputs.len();
                start
            })
            .collect();
        Ok(Self {
            name: dir
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            dir: dir.to_owned(),
            frames: meta.frames,
            files,
            peaks,
            lane_start,
            meta,
            view_start: 0.0,
            frames_per_px: None,
            playhead: 0,
        })
    }

    /// The take track recorded from exactly these input channels (0-based), if any.
    pub fn track_for(&self, channels: &[usize]) -> Option<usize> {
        self.meta.tracks.iter().position(|t| {
            t.inputs.len() == channels.len()
                && t.inputs
                    .iter()
                    .zip(channels)
                    .all(|(input, c)| usize::from(*input) == c + 1)
        })
    }

    /// Overview lanes of take track `track`.
    pub fn lanes_of(&self, track: usize) -> std::ops::Range<usize> {
        let start = self.lane_start[track];
        start..start + self.meta.tracks[track].inputs.len()
    }

    pub fn duration_secs(&self) -> f64 {
        self.frames as f64 / f64::from(self.meta.sample_rate.max(1))
    }

    fn fit(&self, width: f32) -> f64 {
        (self.frames.max(1) as f64 / f64::from(width.max(1.0))).max(MIN_FRAMES_PER_PX)
    }

    /// Current zoom for a lane `width` pixels wide (fits the whole take the first time).
    pub fn zoom(&mut self, width: f32) -> f64 {
        let fit = self.fit(width);
        *self.frames_per_px.get_or_insert(fit)
    }

    pub fn show_all(&mut self, width: f32) {
        self.frames_per_px = Some(self.fit(width));
        self.view_start = 0.0;
    }

    /// Zooms by `factor` (> 1 zooms in), keeping the frame under `anchor_px` in place.
    pub fn zoom_by(&mut self, factor: f64, anchor_px: f32, width: f32) {
        let old = self.zoom(width);
        let new = (old / factor).clamp(MIN_FRAMES_PER_PX, self.fit(width));
        let anchor_frame = self.view_start + f64::from(anchor_px) * old;
        self.frames_per_px = Some(new);
        self.view_start = anchor_frame - f64::from(anchor_px) * new;
        self.clamp_view(width);
    }

    pub fn scroll_by(&mut self, pixels: f32, width: f32) {
        self.view_start += f64::from(pixels) * self.zoom(width);
        self.clamp_view(width);
    }

    /// Keeps the view inside the take.
    pub fn clamp_view(&mut self, width: f32) {
        let span = self.zoom(width) * f64::from(width);
        let max_start = (self.frames as f64 - span).max(0.0);
        self.view_start = self.view_start.clamp(0.0, max_start);
    }

    /// Scrolls so that `frame` is visible (pages forward during playback).
    pub fn follow(&mut self, frame: u64, width: f32) {
        let span = self.zoom(width) * f64::from(width);
        let f = frame as f64;
        if f < self.view_start || f > self.view_start + span {
            self.view_start = f - span * 0.1;
            self.clamp_view(width);
        }
    }

    pub fn frame_at(&mut self, px: f32, width: f32) -> u64 {
        let frame = self.view_start + f64::from(px) * self.zoom(width);
        frame.clamp(0.0, self.frames as f64) as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::take::TrackMeta;
    use crate::wav::SampleFormat;

    fn take(frames: u64) -> OpenTake {
        let track = |inputs: Vec<u16>| TrackMeta {
            name: "t".into(),
            inputs,
            file: "f.wav".into(),
        };
        OpenTake {
            dir: PathBuf::new(),
            name: String::new(),
            meta: TakeMeta {
                app_version: String::new(),
                host: String::new(),
                device: String::new(),
                sample_rate: 48_000,
                format: SampleFormat::Pcm24,
                started_at: String::new(),
                tracks: vec![track(vec![1]), track(vec![3, 4]), track(vec![2])],
                frames,
                preroll_frames: 0,
                dropped_frames: 0,
                recovered: false,
                note: String::new(),
                tempo: None,
                markers: Vec::new(),
                skipped_silent: Vec::new(),
            },
            files: Vec::new(),
            peaks: TakePeaks {
                block: 240,
                lanes: Vec::new(),
            },
            lane_start: vec![0, 1, 3],
            frames,
            view_start: 0.0,
            frames_per_px: None,
            playhead: 0,
        }
    }

    #[test]
    fn rows_find_their_take_track_by_inputs() {
        let t = take(1000);
        assert_eq!(t.track_for(&[0]), Some(0));
        assert_eq!(t.track_for(&[2, 3]), Some(1));
        assert_eq!(t.track_for(&[1]), Some(2));
        assert_eq!(t.track_for(&[2]), None);
        assert_eq!(t.track_for(&[3, 2]), None);
        assert_eq!(t.lanes_of(1), 1..3);
        assert_eq!(t.lanes_of(2), 3..4);
    }

    #[test]
    fn first_zoom_fits_the_whole_take() {
        let mut t = take(480_000);
        assert_eq!(t.zoom(1000.0), 480.0);
        assert_eq!(t.frame_at(500.0, 1000.0), 240_000);
    }

    #[test]
    fn zooming_keeps_the_anchor_and_stays_in_bounds() {
        let mut t = take(480_000);
        t.zoom(1000.0);
        t.zoom_by(2.0, 500.0, 1000.0);
        assert_eq!(t.frames_per_px, Some(240.0));
        assert_eq!(t.frame_at(500.0, 1000.0), 240_000);
        // Cannot zoom out past "whole take" or in past the limit.
        t.zoom_by(0.01, 0.0, 1000.0);
        assert_eq!(t.frames_per_px, Some(480.0));
        assert_eq!(t.view_start, 0.0);
        t.zoom_by(1e9, 0.0, 1000.0);
        assert_eq!(t.frames_per_px, Some(MIN_FRAMES_PER_PX));
    }

    #[test]
    fn scrolling_and_following_stay_inside_the_take() {
        let mut t = take(480_000);
        t.zoom(1000.0);
        t.zoom_by(4.0, 0.0, 1000.0); // 120 frames per px: the view spans 120 000 frames
        t.scroll_by(-50.0, 1000.0);
        assert_eq!(t.view_start, 0.0);
        t.scroll_by(1e6, 1000.0);
        assert_eq!(t.view_start, 360_000.0);
        t.follow(10_000, 1000.0);
        assert_eq!(t.view_start, 0.0);
        t.follow(200_000, 1000.0);
        assert_eq!(t.view_start, 188_000.0);
        // Already visible: no jump.
        t.follow(250_000, 1000.0);
        assert_eq!(t.view_start, 188_000.0);
    }
}
