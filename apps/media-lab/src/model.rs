use crate::fixtures::Fixtures;
use gpui::{Pixels, Point, point, px};
use std::{collections::VecDeque, time::Instant};
use xgpui_media::{
    FrameUploadLimits, ImageJob, LoadStatus, MediaImage, MediaLimits, MediaLoader, UploadLimits,
    UploadStatus,
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Waiting,
    Decoding,
    Staging,
    Ready,
    Retired,
    Failed,
}
impl Phase {
    pub fn label(self) -> &'static str {
        match self {
            Self::Waiting => "WAIT",
            Self::Decoding => "DECODE",
            Self::Staging => "STAGE",
            Self::Ready => "READY",
            Self::Retired => "DONE",
            Self::Failed => "ERROR",
        }
    }
}
pub struct Slot {
    pub job: Option<ImageJob>,
    pub image: Option<MediaImage>,
    pub phase: Phase,
    pub error: Option<String>,
}
impl Slot {
    pub fn waiting() -> Self {
        Self {
            job: None,
            image: None,
            phase: Phase::Waiting,
            error: None,
        }
    }
    fn poll(&mut self) {
        if let Some(job) = self.job.as_mut() {
            if let Some(image) = job.try_take() {
                self.image = Some(image);
                self.phase = Phase::Staging;
                self.job = None;
            } else if let LoadStatus::Failed(error) = job.status() {
                self.phase = Phase::Failed;
                self.error = Some(error);
                self.job = None;
            }
        }
        if let Some(image) = &self.image {
            match image.upload_status() {
                UploadStatus::Ready => self.phase = Phase::Ready,
                UploadStatus::Failed(error) => {
                    self.phase = Phase::Failed;
                    self.error = Some(error);
                }
                _ => self.phase = Phase::Staging,
            }
        }
    }
    pub fn pending(&self) -> bool {
        matches!(
            self.phase,
            Phase::Waiting | Phase::Decoding | Phase::Staging
        )
    }
}
pub struct Lab {
    pub ui_perf: Option<crate::ui_perf::Run>,
    pub continuous_redraw: bool,
    pub video_mode: bool,
    #[cfg(target_os = "macos")]
    pub video_format: crate::video_lab::SourceFormat,
    #[cfg(target_os = "macos")]
    pub video_demo: Option<crate::video_lab::Demo>,
    pub path_mode: bool,
    pub path_animate: bool,
    pub path_trim: usize,
    pub path_dash: usize,
    pub path_phase: f32,
    pub path_width: usize,
    pub path_samples: Vec<xgpui_svg::path_effects::MeasuredPath>,
    pub fixtures: Fixtures,
    pub selected: usize,
    pub loader: MediaLoader,
    pub primary: Slot,
    pub burst: Vec<Slot>,
    pub zoom: f32,
    pub pan: Point<Pixels>,
    pub drag: Option<(Point<Pixels>, Point<Pixels>)>,
    pub opacity: f32,
    pub filters: [usize; 5],
    pub radius: f32,
    pub sweep: bool,
    pub started: Instant,
    pub count_limit: usize,
    pub byte_limit: usize,
    pub updated: Instant,
    pub settle_frames: u8,
    pub intervals: VecDeque<f32>,
    pub admission: String,
}
impl Lab {
    pub fn new(fixtures: Fixtures) -> anyhow::Result<Self> {
        let selected = if fixtures.samples.len() > 3 { 3 } else { 0 };
        let loader = Self::loader(1, 16 * 1024 * 1024)?;
        let mut lab = Self {
            ui_perf: None,
            continuous_redraw: false,
            video_mode: false,
            #[cfg(target_os = "macos")]
            video_format: Default::default(),
            #[cfg(target_os = "macos")]
            video_demo: None,
            path_mode: false,
            path_animate: false,
            path_trim: 1,
            path_dash: 1,
            path_phase: 0.,
            path_width: 0,
            path_samples: crate::paths::samples()?,
            fixtures,
            selected,
            loader,
            primary: Slot::waiting(),
            burst: vec![],
            zoom: 0.72,
            pan: point(px(0.), px(0.)),
            drag: None,
            opacity: 1.,
            filters: [0; 5],
            radius: 16.,
            sweep: false,
            started: Instant::now(),
            count_limit: 1,
            byte_limit: 16 * 1024 * 1024,
            updated: Instant::now(),
            settle_frames: 0,
            intervals: VecDeque::new(),
            admission: String::new(),
        };
        lab.reload();
        Ok(lab)
    }
    pub fn image_filter(&self) -> xgpui_media::ImageFilter {
        xgpui_media::ImageFilter::new()
            .brightness([1., 0.6, 1.4][self.filters[0]])
            .contrast([1., 0.6, 1.5][self.filters[1]])
            .saturation([1., 0., 1.8][self.filters[2]])
            .hue_rotate([0., 90., 180.][self.filters[3]])
            .tint([[0., 0., 0., 0.], [1., 0.4, 0.1, 0.3], [0.1, 0.6, 1., 0.3]][self.filters[4]])
    }
    fn loader(count: usize, bytes: usize) -> anyhow::Result<MediaLoader> {
        MediaLoader::with_upload_policy(
            MediaLimits::default(),
            UploadLimits::default(),
            FrameUploadLimits {
                max_uploads: count,
                max_bytes: bytes,
            },
        )
    }
    pub fn select(&mut self, index: usize) {
        self.selected = index;
        self.reset_view();
        self.reload();
    }
    pub fn reset_view(&mut self) {
        self.zoom = 1.;
        self.pan = point(px(0.), px(0.));
        self.drag = None;
    }
    pub fn reload(&mut self) {
        self.burst.clear();
        self.primary = Slot::waiting();
        self.admission.clear();
    }
    pub fn burst(&mut self) {
        self.burst = (0..12).map(|_| Slot::waiting()).collect();
        self.intervals.clear();
    }
    pub fn policy(&mut self, count: usize, bytes: usize) {
        // These bounded presets are valid. Retired snapshots keep their old loader
        // alive until their workers/commands finish; stale results are not displayed.
        self.loader = Self::loader(count, bytes).expect("valid lab upload policy");
        self.count_limit = count;
        self.byte_limit = bytes;
        self.reload();
    }
    pub fn poll(&mut self) -> bool {
        let now = Instant::now();
        let dt = now.duration_since(self.updated).as_secs_f32() * 1000.;
        self.updated = now;
        if self.continuous_redraw {
            self.intervals.push_back(dt);
            if self.intervals.len() > 120 {
                self.intervals.pop_front();
            }
        }
        self.admission.clear();
        self.primary.poll();
        let path = &self.fixtures.samples[self.selected].path;
        if self.primary.phase == Phase::Waiting {
            match self.loader.load(path) {
                Ok(job) => {
                    self.primary.job = Some(job);
                    self.primary.phase = Phase::Decoding;
                }
                Err(error) => self.admission = error.to_string(),
            }
        }
        for slot in &mut self.burst {
            slot.poll();
        }
        // Retain completed previews within admission limits. Retire older ready sources when
        // more work is waiting, leaving payload room for the next decode jobs.
        if self.burst.iter().any(|s| s.phase == Phase::Waiting) {
            let ready: Vec<_> = self
                .burst
                .iter()
                .enumerate()
                .filter(|(_, s)| s.phase == Phase::Ready)
                .map(|(i, _)| i)
                .collect();
            for &index in ready.iter().take(ready.len().saturating_sub(1)) {
                self.burst[index].image = None;
                self.burst[index].phase = Phase::Retired;
            }
        }
        for slot in &mut self.burst {
            if slot.phase == Phase::Waiting {
                match self.loader.load(path) {
                    Ok(job) => {
                        slot.job = Some(job);
                        slot.phase = Phase::Decoding;
                    }
                    Err(error) => {
                        self.admission = error.to_string();
                        break;
                    }
                }
            }
        }
        let pending = self.primary.pending() || self.burst.iter().any(Slot::pending);
        if pending {
            self.settle_frames = 6;
        } else {
            self.settle_frames = self.settle_frames.saturating_sub(1);
        }
        pending || self.settle_frames > 0
    }
    pub fn p95(&self) -> Option<f32> {
        if self.intervals.len() < 3 {
            return None;
        }
        let mut values: Vec<_> = self.intervals.iter().copied().collect();
        values.sort_by(f32::total_cmp);
        Some(values[(values.len() as f32 * 0.95).ceil() as usize - 1])
    }
}
