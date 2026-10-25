use crate::scheduler::{Client, Reservation};
use crate::{Job, SvgScheduler};
use crate::{RasterImage, RasterPolicy, RasterQueue, Resolution, Stage, SvgDocument, image_bytes};
use gpui::{prelude::*, *};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

/// Progress for one mounted element. Output budget excludes filter/GPU memory.
#[derive(Clone, Debug, PartialEq)]
pub struct SvgStatus {
    pub phase: SvgPhase,
    pub resolution: Option<Resolution>,
    pub displayed: Option<Size<DevicePixels>>,
    pub raster_ms: Option<f64>,
    pub completed_rasters: usize,
    pub error: Option<String>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SvgPhase {
    Empty,
    Waiting,
    Preview,
    Queued,
    Detail,
    Ready,
    Failed,
}

type StatusListener = Box<dyn Fn(&SvgStatus, &mut Window, &mut App)>;
#[derive(Clone, Copy)]
struct Settings {
    zoom: f32,
    pan: Point<Pixels>,
    logical_size: Option<Size<f32>>,
    policy: RasterPolicy,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            zoom: 1.,
            pan: point(px(0.), px(0.)),
            logical_size: None,
            policy: RasterPolicy::default(),
        }
    }
}

/// Full-color SVG with element-owned progressive rendering. The ID must remain
/// stable and unique among siblings. Set layout dimensions with GPUI's Styled API.
#[derive(IntoElement)]
pub struct SvgImage {
    id: ElementId,
    document: SvgDocument,
    settings: Settings,
    inner: Stateful<Div>,
    listener: Option<StatusListener>,
    scheduler: Option<SvgScheduler>,
}
/// Construct a contained SVG viewport. Transparent backdrop; fills its parent by
/// default. Raster state lives as long as this ID is rendered in consecutive frames.
pub fn svg_image(id: impl Into<ElementId>, document: SvgDocument) -> SvgImage {
    let id = id.into();
    SvgImage {
        inner: div().id(id.clone()).size_full().overflow_hidden(),
        id,
        document,
        settings: Settings::default(),
        listener: None,
        scheduler: None,
    }
}
impl SvgImage {
    /// Visual zoom about the viewport center; does not change layout or hitboxes.
    pub fn zoom(mut self, zoom: f32) -> Self {
        self.settings.zoom = zoom;
        self
    }
    /// Logical-pixel translation from the viewport center. Does not rerasterize.
    pub fn pan(mut self, pan: Point<Pixels>) -> Self {
        self.settings.pan = pan;
        self
    }
    /// Override contain-fit with an explicit logical artwork size at 100% zoom.
    pub fn logical_size(mut self, size: Size<f32>) -> Self {
        self.settings.logical_size = Some(size);
        self
    }
    pub fn policy(mut self, policy: RasterPolicy) -> Self {
        self.settings.policy = policy;
        self
    }
    /// Share concurrency and estimated aggregate image-memory limits across elements.
    pub fn scheduler(mut self, scheduler: SvgScheduler) -> Self {
        self.scheduler = Some(scheduler);
        self
    }
    /// Called after drawing when observable status changes. No queue wiring is
    /// required; use the normal GPUI listener pattern to update optional diagnostics.
    pub fn on_status_change(
        mut self,
        listener: impl Fn(&SvgStatus, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.listener = Some(Box::new(listener));
        self
    }
}
impl Styled for SvgImage {
    fn style(&mut self) -> &mut StyleRefinement {
        self.inner.style()
    }
}
impl InteractiveElement for SvgImage {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.inner.interactivity()
    }
}

struct PaintState {
    image: Option<Arc<RenderImage>>,
    bounds: Option<Bounds<Pixels>>,
}
impl RenderOnce for SvgImage {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let document = self.document;
        let state =
            window.use_keyed_state(self.id, cx, |_, cx| ImageState::new(document.clone(), cx));
        state.update(cx, |state, cx| state.bind_scheduler(self.scheduler, cx));
        let painting_state = state.downgrade();
        let settings = self.settings;
        let listener = self.listener;
        self.inner.child(
            canvas(
                move |bounds, window, cx| {
                    let (paint, report) = state.update(cx, |state, cx| {
                        let image_bounds =
                            state.configure(document, settings, bounds, window.scale_factor(), cx);
                        let status = state.status();
                        let report =
                            if listener.is_some() && state.reported.as_ref() != Some(&status) {
                                state.reported = Some(status.clone());
                                Some(status)
                            } else {
                                None
                            };
                        (
                            PaintState {
                                image: state.current.as_ref().map(|r| r.image.clone()),
                                bounds: image_bounds,
                            },
                            report,
                        )
                    });
                    if let (Some(listener), Some(status)) = (listener, report) {
                        window.defer(cx, move |window, cx| listener(&status, window, cx));
                    }
                    paint
                },
                move |bounds, paint, window, cx| {
                    if let (Some(image), Some(image_bounds)) = (paint.image, paint.bounds) {
                        // Keep the SVG root/viewport crop independent of any caller overflow style.
                        let result =
                            window.with_content_mask(Some(ContentMask { bounds }), |window| {
                                window.paint_image(
                                    image_bounds,
                                    image_bounds,
                                    Corners::default(),
                                    image,
                                    0,
                                    false,
                                )
                            });
                        if let Err(error) = result {
                            let _ = painting_state.update(cx, |state, cx| {
                                let message = error.to_string();
                                if state.render_error.as_ref() != Some(&message) {
                                    state.render_error = Some(message);
                                    cx.notify();
                                }
                            });
                        }
                    }
                },
            )
            .size_full(),
        )
    }
}

struct ImageState {
    document: SvgDocument,
    queue: RasterQueue,
    current: Option<RasterImage>,
    resolution: Option<Resolution>,
    generation: usize,
    epoch: Instant,
    worker: Option<Task<()>>,
    wake: Option<Task<()>>,
    input_error: Option<String>,
    render_error: Option<String>,
    completed: usize,
    reported: Option<SvgStatus>,
    client: Option<Client>,
    lease: Option<Reservation>,
    scheduler_watch: Option<Task<()>>,
    pending_job: Option<Job>,
}
impl ImageState {
    fn new(document: SvgDocument, cx: &mut Context<Self>) -> Self {
        cx.on_release(|state, cx| {
            if let Some(raster) = state.current.take() {
                // The releasing window may still be borrowed by the frame loop.
                let lease = state.lease.take();
                cx.defer(move |cx| {
                    cx.drop_image(raster.image, None);
                    drop(lease);
                });
            }
        })
        .detach();
        Self {
            document,
            queue: RasterQueue::new(RasterPolicy::default()),
            current: None,
            resolution: None,
            generation: 0,
            epoch: cx.background_executor().now(),
            worker: None,
            wake: None,
            input_error: None,
            render_error: None,
            completed: 0,
            reported: None,
            client: None,
            lease: None,
            scheduler_watch: None,
            pending_job: None,
        }
    }
    fn bind_scheduler(&mut self, scheduler: Option<SvgScheduler>, cx: &mut Context<Self>) {
        let same = match (&self.client, &scheduler) {
            (None, None) => true,
            (Some(c), Some(s)) => c.scheduler().ptr_eq(s),
            _ => false,
        };
        if same {
            return;
        }
        self.scheduler_watch.take();
        self.client.take();
        self.queue.invalidate();
        self.cancel_pending_work();
        self.generation += 1;
        self.resolution = None;
        self.discard_current(cx);
        self.client = scheduler.map(|s| s.join());
        if let Some(client) = &self.client {
            let mut revision = client.revision();
            self.scheduler_watch = Some(cx.spawn(async move |this, cx| {
                loop {
                    let future = this
                        .update(cx, |s, _| s.client.as_ref().map(|c| c.changed(revision)))
                        .ok()
                        .flatten();
                    let Some(future) = future else {
                        break;
                    };
                    revision = future.await;
                    if this.update(cx, |_, cx| cx.notify()).is_err() {
                        break;
                    }
                }
            }));
        }
    }
    fn cancel_pending_work(&mut self) {
        if let Some(job) = self.pending_job.take() {
            self.worker.take();
            self.queue.cancel_pending(job);
        }
    }
    fn discard_current(&mut self, cx: &mut Context<Self>) {
        if let Some(raster) = self.current.take() {
            // Wait until the current frame has finished using its atlas entry.
            let lease = self.lease.take();
            cx.defer(move |cx| {
                cx.drop_image(raster.image, None);
                drop(lease);
            });
        }
    }
    fn now(&self, cx: &Context<Self>) -> Duration {
        cx.background_executor()
            .now()
            .saturating_duration_since(self.epoch)
    }
    fn status(&self) -> SvgStatus {
        let error = self
            .input_error
            .clone()
            .or_else(|| self.render_error.clone());
        let phase = if error.is_some() {
            SvgPhase::Failed
        } else if self.resolution.is_none() {
            SvgPhase::Empty
        } else if self.pending_job.is_some() {
            SvgPhase::Queued
        } else if let Some(stage) = self.queue.active_stage() {
            match stage {
                Stage::Preview => SvgPhase::Preview,
                Stage::Detail => SvgPhase::Detail,
            }
        } else if self.queue.ready() {
            SvgPhase::Ready
        } else {
            SvgPhase::Waiting
        };
        SvgStatus {
            phase,
            resolution: self.resolution,
            displayed: self.current.as_ref().map(|r| r.image.size(0)),
            raster_ms: self.current.as_ref().map(|r| r.ms),
            completed_rasters: self.completed,
            error,
        }
    }
    fn configure(
        &mut self,
        document: SvgDocument,
        settings: Settings,
        bounds: Bounds<Pixels>,
        density: f32,
        cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let mut settings = settings;
        if let Some(client) = &self.client {
            settings.policy.max_output_bytes =
                settings.policy.max_output_bytes.min(client.output_budget());
        }
        let changed_document = !self.document.ptr_eq(&document);
        if changed_document || self.queue.policy != settings.policy {
            self.document = document;
            self.queue.policy = settings.policy;
            self.queue.invalidate();
            self.generation += 1;
            self.render_error = None;
            self.resolution = None;
            if changed_document
                || self.current.as_ref().is_some_and(|r| {
                    image_bytes(r.image.size(0)) > settings.policy.max_output_bytes
                })
            {
                self.discard_current(cx);
            }
        }
        self.wake.take();
        if bounds.size.width <= px(0.) || bounds.size.height <= px(0.) {
            self.queue.invalidate();
            self.cancel_pending_work();
            self.resolution = None;
            self.input_error = None;
            return None;
        }
        let result = geometry(&self.document, settings, bounds, density);
        let (image_bounds, resolution) = match result {
            Ok(value) => value,
            Err(error) => {
                self.queue.invalidate();
                self.cancel_pending_work();
                self.resolution = None;
                self.input_error = Some(error.to_string());
                return None;
            }
        };
        self.input_error = None;
        if self
            .resolution
            .is_none_or(|old| old.output != resolution.output)
        {
            self.render_error = None;
        }
        self.resolution = Some(resolution);
        if let Some(current) = &self.current {
            self.queue
                .set_current(self.generation, current.image.size(0));
        }
        self.queue
            .request(self.generation, resolution.output, self.now(cx));
        if self
            .pending_job
            .is_some_and(|job| !self.queue.is_current(job))
        {
            self.cancel_pending_work();
        }
        self.drive(cx);
        Some(image_bounds)
    }
    fn drive(&mut self, cx: &mut Context<Self>) {
        self.wake.take();
        if let Some(job) = self.queue.next(self.now(cx)) {
            let acquire = self
                .client
                .as_ref()
                .map(|c| c.acquire(image_bytes(job.pixels)));
            if acquire.is_some() {
                self.pending_job = Some(job);
            }
            self.worker = Some(cx.spawn(async move |this, cx| {
                let reservation = if let Some(acquire) = acquire {
                    Some(acquire.await)
                } else {
                    None
                };
                let input = this
                    .update(cx, |state, cx| {
                        state.pending_job = None;
                        if !state.queue.is_current(job) {
                            state.queue.cancel_pending(job);
                            state.drive(cx);
                            cx.notify();
                            return None;
                        }
                        Some((
                            cx.svg_renderer(),
                            state.document.clone(),
                            state.queue.policy.max_output_bytes,
                            cx.background_executor().clone(),
                        ))
                    })
                    .ok()
                    .flatten();
                let Some((renderer, document, budget, executor)) = input else {
                    return;
                };
                let work = executor.spawn(async move {
                    let result = document.rasterize(&renderer, job.pixels, budget);
                    (result, reservation)
                });
                let (result, reservation) = work.await;
                let _ = this.update(cx, |state, cx| {
                    if state.queue.complete(job, result.is_ok()) {
                        match result {
                            Ok(image) => {
                                state.discard_current(cx);
                                state.current = Some(image);
                                state.lease = reservation.map(Reservation::retain);
                                state.completed += 1;
                                state.render_error = None;
                            }
                            Err(error) => state.render_error = Some(error.to_string()),
                        }
                    }
                    state.drive(cx);
                    cx.notify();
                });
            }));
        } else if let Some(delay) = self.queue.wake_after(self.now(cx)) {
            let timer = cx
                .background_executor()
                .timer(delay.max(Duration::from_millis(1)));
            self.wake = Some(cx.spawn(async move |this, cx| {
                timer.await;
                let _ = this.update(cx, |state, cx| {
                    state.drive(cx);
                    cx.notify();
                });
            }));
        }
    }
}
fn geometry(
    document: &SvgDocument,
    settings: Settings,
    bounds: Bounds<Pixels>,
    density: f32,
) -> anyhow::Result<(Bounds<Pixels>, Resolution)> {
    anyhow::ensure!(
        [f32::from(settings.pan.x), f32::from(settings.pan.y)]
            .iter()
            .all(|n| n.is_finite()),
        "pan must be finite"
    );
    let logical = if let Some(logical) = settings.logical_size {
        logical
    } else {
        let intrinsic = document.size();
        let factor = (f32::from(bounds.size.width) / intrinsic.width)
            .min(f32::from(bounds.size.height) / intrinsic.height);
        size(intrinsic.width * factor, intrinsic.height * factor)
    };
    let resolution = settings
        .policy
        .resolution(logical, settings.zoom, density)?;
    let image_size = size(
        px(logical.width * settings.zoom),
        px(logical.height * settings.zoom),
    );
    let origin =
        bounds.center() + settings.pan - point(image_size.width / 2., image_size.height / 2.);
    Ok((Bounds::new(origin, image_size), resolution))
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;
    use gpui::{AppContext, TestAppContext};
    use std::{cell::RefCell, rc::Rc};

    fn document(color: &str) -> SvgDocument {
        SvgDocument::parse(&SvgRenderer::new(Arc::new(())),format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="50"><path d="M0 0H100V50H0Z" fill="{color}"/></svg>"#).as_bytes()).unwrap()
    }
    fn settings() -> Settings {
        Settings {
            zoom: 8.,
            logical_size: Some(size(128., 64.)),
            policy: RasterPolicy {
                max_output_bytes: 64 * 1024,
                preview_edge: 64,
                ..Default::default()
            },
            ..Default::default()
        }
    }
    fn bounds() -> Bounds<Pixels> {
        Bounds::new(point(px(0.), px(0.)), size(px(200.), px(100.)))
    }

    #[gpui::test]
    fn owns_preview_timer_detail_and_reuses_pan(cx: &mut TestAppContext) {
        let doc = document("red");
        let state = cx.new(|cx| ImageState::new(doc.clone(), cx));
        state.update(cx, |state, cx| {
            state.configure(doc.clone(), settings(), bounds(), 2., cx);
        });
        cx.run_until_parked();
        state.read_with(cx, |state, _| {
            assert_eq!(state.completed, 1);
            assert_eq!(state.status().phase, SvgPhase::Waiting);
        });
        cx.executor().advance_clock(Duration::from_millis(119));
        cx.run_until_parked();
        assert_eq!(state.read_with(cx, |state, _| state.completed), 1);
        cx.executor().advance_clock(Duration::from_millis(1));
        cx.run_until_parked();
        state.read_with(cx, |state, _| {
            assert_eq!(state.completed, 2);
            assert_eq!(state.status().phase, SvgPhase::Ready);
            assert!(image_bytes(state.current.as_ref().unwrap().image.size(0)) <= 64 * 1024);
        });
        state.update(cx, |state, cx| {
            state.configure(
                doc,
                Settings {
                    pan: point(px(30.), px(-15.)),
                    ..settings()
                },
                bounds(),
                2.,
                cx,
            );
        });
        cx.run_until_parked();
        assert_eq!(state.read_with(cx, |state, _| state.completed), 2);
    }
    #[gpui::test]
    fn document_switch_rejects_old_worker_and_budget_downgrade_drops_large_image(
        cx: &mut TestAppContext,
    ) {
        let red = document("red");
        let blue = document("blue");
        let state = cx.new(|cx| ImageState::new(red.clone(), cx));
        state.update(cx, |state, cx| {
            state.configure(red, settings(), bounds(), 2., cx);
        });
        state.update(cx, |state, cx| {
            state.configure(blue.clone(), settings(), bounds(), 2., cx);
            assert!(state.current.is_none());
        });
        cx.run_until_parked();
        cx.executor().advance_clock(Duration::from_millis(120));
        cx.run_until_parked();
        state.read_with(cx, |state, _| {
            let data = state.current.as_ref().unwrap().image.as_bytes(0).unwrap();
            assert!(
                data.as_chunks::<4>()
                    .0
                    .iter()
                    .all(|p| p[0] == 255 && p[2] == 0)
            );
        });
        state.update(cx, |state, cx| {
            let mut small = settings();
            small.policy.max_output_bytes = 4096;
            state.configure(blue, small, bounds(), 2., cx);
            assert!(state.current.is_none());
        });
        cx.run_until_parked();
        cx.executor().advance_clock(Duration::from_millis(120));
        cx.run_until_parked();
        assert!(
            state.read_with(cx, |s, _| image_bytes(
                s.current.as_ref().unwrap().image.size(0)
            )) <= 4096
        );
    }
    #[gpui::test]
    fn invalid_request_suspends_work_and_recovers_without_spin(cx: &mut TestAppContext) {
        let doc = document("red");
        let state = cx.new(|cx| ImageState::new(doc.clone(), cx));
        state.update(cx, |state, cx| {
            state.configure(doc.clone(), settings(), bounds(), 2., cx);
        });
        state.update(cx, |state, cx| {
            state.configure(
                doc.clone(),
                Settings {
                    zoom: f32::NAN,
                    ..settings()
                },
                bounds(),
                2.,
                cx,
            );
        });
        cx.run_until_parked();
        cx.executor().advance_clock(Duration::from_secs(1));
        cx.run_until_parked();
        state.read_with(cx, |s, _| {
            assert_eq!(s.completed, 0);
            assert_eq!(s.status().phase, SvgPhase::Failed);
            assert!(!s.queue.busy());
        });
        state.update(cx, |state, cx| {
            state.configure(doc, settings(), bounds(), 2., cx);
        });
        cx.run_until_parked();
        cx.executor().advance_clock(Duration::from_millis(120));
        cx.run_until_parked();
        assert_eq!(
            state.read_with(cx, |s, _| s.status().phase),
            SvgPhase::Ready
        );
    }

    #[gpui::test]
    fn density_resize_zero_bounds_and_drop_have_bounded_lifetimes(cx: &mut TestAppContext) {
        let doc = document("red");
        let state = cx.new(|cx| ImageState::new(doc.clone(), cx));
        let settings = Settings {
            logical_size: Some(size(128., 64.)),
            ..Default::default()
        };
        state.update(cx, |s, cx| {
            s.configure(doc.clone(), settings, bounds(), 1., cx);
        });
        cx.run_until_parked();
        assert_eq!(
            state.read_with(cx, |s, _| s.current.as_ref().unwrap().image.size(0)),
            size(DevicePixels(128), DevicePixels(64))
        );
        state.update(cx, |s, cx| {
            s.configure(doc.clone(), settings, bounds(), 2., cx);
        });
        cx.run_until_parked();
        cx.executor().advance_clock(Duration::from_millis(120));
        cx.run_until_parked();
        let completed = state.read_with(cx, |s, _| s.completed);
        assert_eq!(
            state.read_with(cx, |s, _| s.current.as_ref().unwrap().image.size(0)),
            size(DevicePixels(256), DevicePixels(128))
        );
        state.update(cx, |s, cx| {
            s.configure(doc.clone(), settings, Bounds::default(), 2., cx);
            assert_eq!(s.status().phase, SvgPhase::Empty);
        });
        state.update(cx, |s, cx| {
            s.configure(doc.clone(), settings, bounds(), 2., cx);
        });
        cx.run_until_parked();
        assert_eq!(state.read_with(cx, |s, _| s.completed), completed);
        state.update(cx, |s, cx| {
            s.configure(
                doc,
                Settings {
                    zoom: 4.,
                    ..settings
                },
                bounds(),
                2.,
                cx,
            );
        });
        let weak = state.downgrade();
        drop(state);
        cx.run_until_parked();
        cx.executor().advance_clock(Duration::from_secs(1));
        cx.run_until_parked();
        assert!(weak.upgrade().is_none()); // state owns timer/worker; neither retains its entity
    }

    struct Example {
        document: SvgDocument,
        pan: Point<Pixels>,
        status: Rc<RefCell<Vec<SvgStatus>>>,
    }
    impl Render for Example {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let status = self.status.clone();
            svg_image("mounted-svg", self.document.clone())
                .w(px(160.))
                .h(px(100.))
                .pan(self.pan)
                .bg(rgb(0xffffff))
                .rounded_md()
                .on_status_change(move |value, _, _| status.borrow_mut().push(value.clone()))
        }
    }
    #[gpui::test]
    fn declarative_element_retains_state_and_refreshes_its_host(cx: &mut TestAppContext) {
        let statuses = Rc::new(RefCell::new(Vec::new()));
        let record = statuses.clone();
        let doc = document("#00aa88");
        let window = cx.open_window(size(px(300.), px(200.)), move |_, _| Example {
            document: doc,
            pan: point(px(0.), px(0.)),
            status: record,
        });
        cx.run_until_parked();
        window
            .update(cx, |_, window, cx| window.simulate_next_frame(cx))
            .unwrap();
        cx.run_until_parked();
        assert!(statuses.borrow().iter().any(|s| s.phase == SvgPhase::Ready));
        let completed = statuses.borrow().last().unwrap().completed_rasters;
        assert_eq!(completed, 1); // small first output is an immediate preview, already sufficient
        window
            .update(cx, |view, _, cx| {
                view.pan = point(px(15.), px(-10.));
                cx.notify();
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |_, window, cx| window.simulate_next_frame(cx))
            .unwrap();
        cx.run_until_parked();
        assert_eq!(
            statuses.borrow().last().unwrap().completed_rasters,
            completed
        );
    }
    struct AtlasFixture {
        state: Option<Entity<ImageState>>,
    }
    impl Render for AtlasFixture {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let image = self
                .state
                .as_ref()
                .and_then(|s| s.read(cx).current.as_ref().map(|r| r.image.clone()));
            canvas(
                |_, _, _| (),
                move |bounds, _, window, _| {
                    if let Some(image) = image {
                        window
                            .paint_image(bounds, bounds, Corners::default(), image, 0, false)
                            .unwrap();
                    }
                },
            )
            .size_full()
        }
    }
    #[gpui::test]
    fn replaced_and_released_images_leave_the_sprite_atlas(cx: &mut TestAppContext) {
        let doc = document("red");
        let state = cx.new(|cx| ImageState::new(doc.clone(), cx));
        let window = cx.open_window(size(px(200.), px(100.)), {
            let state = state.clone();
            move |_, _| AtlasFixture { state: Some(state) }
        });
        let config = Settings {
            logical_size: Some(size(100., 50.)),
            ..Default::default()
        };
        state.update(cx, |s, cx| {
            s.configure(doc.clone(), config, bounds(), 1., cx);
        });
        cx.run_until_parked();
        let first = state.read_with(cx, |s, _| s.current.as_ref().unwrap().image.clone());
        window.update(cx, |_, _, cx| cx.notify()).unwrap();
        cx.run_until_parked();
        window
            .update(cx, |_, w, _| assert!(w.has_image_atlas_entry(&first)))
            .unwrap();
        state.update(cx, |s, cx| {
            s.configure(doc, Settings { zoom: 2., ..config }, bounds(), 1., cx);
        });
        cx.executor().advance_clock(Duration::from_millis(120));
        cx.run_until_parked();
        window.update(cx, |_, _, cx| cx.notify()).unwrap();
        cx.run_until_parked();
        let second = state.read_with(cx, |s, _| s.current.as_ref().unwrap().image.clone());
        window
            .update(cx, |_, w, _| {
                assert!(!w.has_image_atlas_entry(&first));
                assert!(w.has_image_atlas_entry(&second));
            })
            .unwrap();
        window
            .update(cx, |v, _, cx| {
                v.state = None;
                cx.notify();
            })
            .unwrap();
        cx.update(|_| drop(state));
        cx.run_until_parked();
        window
            .update(cx, |_, w, _| assert!(!w.has_image_atlas_entry(&second)))
            .unwrap();
    }
    #[gpui::test]
    fn queued_document_replacement_and_zero_bounds_cancel_without_blocking_peers(
        cx: &mut TestAppContext,
    ) {
        use std::{
            future::Future,
            pin::Pin,
            task::{Context as TaskContext, Poll, Waker},
        };
        let pool = SvgScheduler::new(crate::SchedulerLimits {
            max_jobs: 1,
            max_bytes: 64 * 1024,
        })
        .unwrap();
        let blocker = pool.join();
        let mut acquire = blocker.acquire(1024);
        let held = match Pin::new(&mut acquire).poll(&mut TaskContext::from_waker(Waker::noop())) {
            Poll::Ready(held) => held,
            _ => panic!("expected capacity"),
        };
        let red = document("red");
        let state = cx.new(|cx| ImageState::new(red.clone(), cx));
        state.update(cx, |s, cx| {
            s.bind_scheduler(Some(pool.clone()), cx);
            s.configure(red.clone(), settings(), bounds(), 1., cx);
        });
        cx.run_until_parked();
        assert_eq!(pool.stats().queued_jobs, 1);
        state.update(cx, |s, cx| {
            s.configure(red.clone(), settings(), Bounds::default(), 1., cx);
        });
        cx.run_until_parked();
        assert_eq!(pool.stats().queued_jobs, 0);
        state.update(cx, |s, cx| {
            s.configure(red, settings(), bounds(), 1., cx);
        });
        cx.run_until_parked();
        assert_eq!(pool.stats().queued_jobs, 1);
        let green = document("#00ff00");
        state.update(cx, |s, cx| {
            s.configure(green, settings(), bounds(), 1., cx);
        });
        cx.run_until_parked();
        assert_eq!(pool.stats().queued_jobs, 1);
        drop(held);
        cx.run_until_parked();
        cx.executor().advance_clock(Duration::from_millis(120));
        cx.run_until_parked();
        state.read_with(cx, |s, _| {
            assert_eq!(s.status().phase, SvgPhase::Ready);
            assert_eq!(
                s.current.as_ref().unwrap().image.as_bytes(0).unwrap()[1],
                255
            );
        });
        assert_eq!(pool.stats().active_jobs, 0);
        assert_eq!(pool.stats().queued_jobs, 0);
    }
    struct SharedGallery {
        document: SvgDocument,
        scheduler: SvgScheduler,
        count: usize,
        statuses: Rc<RefCell<Vec<Option<SvgStatus>>>>,
    }
    impl Render for SharedGallery {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().flex().children((0..self.count).map(|i| {
                let statuses = self.statuses.clone();
                svg_image(("shared", i), self.document.clone())
                    .scheduler(self.scheduler.clone())
                    .w(px(128.))
                    .h(px(64.))
                    .zoom(16.)
                    .on_status_change(move |s, _, _| statuses.borrow_mut()[i] = Some(s.clone()))
            }))
        }
    }
    #[gpui::test]
    fn shared_gallery_refines_rebalances_and_releases_all_reservations(cx: &mut TestAppContext) {
        let scheduler = SvgScheduler::new(crate::SchedulerLimits {
            max_jobs: 1,
            max_bytes: 1024 * 1024,
        })
        .unwrap();
        let statuses = Rc::new(RefCell::new(vec![None; 4]));
        let window = cx.open_window(size(px(600.), px(200.)), {
            let scheduler = scheduler.clone();
            let statuses = statuses.clone();
            move |_, _| SharedGallery {
                document: document("purple"),
                scheduler,
                count: 1,
                statuses,
            }
        });
        for _ in 0..4 {
            cx.run_until_parked();
            cx.executor().advance_clock(Duration::from_millis(120));
            window
                .update(cx, |_, w, cx| w.simulate_next_frame(cx))
                .unwrap();
        }
        cx.run_until_parked();
        assert_eq!(
            statuses.borrow()[0].as_ref().unwrap().phase,
            SvgPhase::Ready
        );
        let first = statuses.borrow()[0].as_ref().unwrap().displayed.unwrap();
        window
            .update(cx, |v, _, cx| {
                v.count = 4;
                cx.notify();
            })
            .unwrap();
        for _ in 0..8 {
            cx.run_until_parked();
            cx.executor().advance_clock(Duration::from_millis(120));
            window
                .update(cx, |_, w, cx| w.simulate_next_frame(cx))
                .unwrap();
        }
        cx.run_until_parked();
        assert!(
            statuses
                .borrow()
                .iter()
                .all(|s| s.as_ref().is_some_and(|s| s.phase == SvgPhase::Ready))
        );
        assert!(
            image_bytes(statuses.borrow()[0].as_ref().unwrap().displayed.unwrap())
                < image_bytes(first)
        );
        assert_eq!(scheduler.stats().members, 4);
        assert_eq!(scheduler.stats().peak_active_jobs, 1);
        assert!(scheduler.stats().peak_managed_bytes <= 1024 * 1024);
        window
            .update(cx, |v, _, cx| {
                v.count = 0;
                cx.notify();
            })
            .unwrap();
        for _ in 0..3 {
            cx.run_until_parked();
            window
                .update(cx, |_, w, cx| w.simulate_next_frame(cx))
                .unwrap();
        }
        cx.run_until_parked();
        assert_eq!(scheduler.stats().members, 0);
        assert_eq!(scheduler.stats().managed_bytes, 0);
        assert_eq!(scheduler.stats().active_jobs, 0);
        assert_eq!(scheduler.stats().queued_jobs, 0);
    }
    struct Pair {
        document: SvgDocument,
        status: Rc<RefCell<[Option<SvgStatus>; 2]>>,
    }
    impl Render for Pair {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let mut root = div().flex();
            for (i, id) in ["left-svg", "right-svg"].into_iter().enumerate() {
                let statuses = self.status.clone();
                root = root.child(
                    svg_image(id, self.document.clone())
                        .w(px(128.))
                        .h(px(64.))
                        .zoom(if i == 0 { 1. } else { 2. })
                        .on_status_change(move |status, _, _| {
                            statuses.borrow_mut()[i] = Some(status.clone())
                        }),
                );
            }
            root
        }
    }
    #[gpui::test]
    fn sibling_ids_keep_independent_resolution_and_worker_state(cx: &mut TestAppContext) {
        let statuses = Rc::new(RefCell::new([None, None]));
        let record = statuses.clone();
        let doc = document("green");
        let window = cx.open_window(size(px(300.), px(100.)), move |_, _| Pair {
            document: doc,
            status: record,
        });
        cx.run_until_parked();
        window
            .update(cx, |_, window, cx| window.simulate_next_frame(cx))
            .unwrap();
        cx.run_until_parked();
        let snapshot = statuses.borrow();
        let left = snapshot[0].as_ref().unwrap();
        let right = snapshot[1].as_ref().unwrap();
        assert_eq!(left.phase, SvgPhase::Ready);
        assert_eq!(right.phase, SvgPhase::Ready);
        assert_eq!(
            i32::from(left.displayed.unwrap().width) * 2,
            i32::from(right.displayed.unwrap().width)
        );
        assert_eq!(left.completed_rasters, 1);
        assert_eq!(right.completed_rasters, 1);
    }
}
