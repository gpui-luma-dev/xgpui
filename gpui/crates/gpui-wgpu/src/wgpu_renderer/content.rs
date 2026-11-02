//! Completed-GPU-image presentation. UI encoding never waits for producer work.
use super::*;
use crate::content_executor::*;
use gpui::content as c;
use std::{
    collections::{HashMap, HashSet},
    sync::Weak,
};

#[cfg(not(target_family = "wasm"))]
use std::sync::OnceLock;
type Signature = Vec<(u64, c::ContentRequest)>;
struct Pending {
    keys: Vec<c::OutputKey>,
    presentation_slots: Vec<Option<c::PresentationSlotId>>,
    job: WgpuContentJob,
    groups: Vec<c::ContentGroupTicket>,
}
/// Shared per-device worker and exported-image budget.
fn service(
    device: &Arc<wgpu::Device>,
    queue: &Arc<wgpu::Queue>,
) -> Result<Arc<WgpuContentExecutor>> {
    type Registry = HashMap<(usize, usize), Weak<WgpuContentExecutor>>;
    #[cfg(not(target_family = "wasm"))]
    static SERVICES: OnceLock<std::sync::Mutex<Registry>> = OnceLock::new();
    #[cfg(target_family = "wasm")]
    thread_local! {static SERVICES:std::cell::RefCell<Registry>=std::cell::RefCell::new(HashMap::new());}
    fn get(
        registry: &mut Registry,
        device: &Arc<wgpu::Device>,
        queue: &Arc<wgpu::Queue>,
    ) -> Result<Arc<WgpuContentExecutor>> {
        let key = (Arc::as_ptr(device) as usize, Arc::as_ptr(queue) as usize);
        if let Some(service) = registry.get(&key).and_then(Weak::upgrade) {
            return Ok(service);
        }
        let service = Arc::new(WgpuContentExecutor::on_device(
            device.clone(),
            queue.clone(),
            WgpuContentOptions::default(),
        )?);
        registry.retain(|_, v| v.strong_count() != 0);
        registry.insert(key, Arc::downgrade(&service));
        Ok(service)
    }
    #[cfg(not(target_family = "wasm"))]
    {
        get(
            &mut SERVICES.get_or_init(Default::default).lock().unwrap(),
            device,
            queue,
        )
    }
    #[cfg(target_family = "wasm")]
    {
        SERVICES.with(|registry| get(&mut registry.borrow_mut(), device, queue))
    }
}
pub(super) struct ContentPresenter {
    executor: Arc<WgpuContentExecutor>,
    wake: Arc<dyn Fn() + Send + Sync>,
    pending: Option<Pending>,
    cache: HashMap<c::OutputKey, GpuImage>,
    slots: HashMap<c::PresentationSlotId, c::OutputKey>,
    failed: Option<(Signature, c::ContentError)>,
    dependencies: c::ContentDependencies,
    description: Option<(Signature, Arc<c::ContentGraph>)>,
    pub stats: WgpuContentStats,
    pub limits: WgpuContentLimits,
    requests: usize,
    completions: usize,
    wide: Option<bool>,
    groups: c::ContentGroupState<GpuImage>,
    redraw: Arc<std::sync::atomic::AtomicBool>,
}
impl ContentPresenter {
    pub fn new(device: &Arc<wgpu::Device>, queue: &Arc<wgpu::Queue>) -> Result<Self> {
        let redraw = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = redraw.clone();
        Ok(Self {
            executor: service(device, queue)?,
            wake: Arc::new(move || {
                flag.store(true, std::sync::atomic::Ordering::Release);
            }),
            pending: None,
            cache: HashMap::new(),
            slots: HashMap::new(),
            failed: None,
            dependencies: c::ContentDependencies::default(),
            description: None,
            stats: Default::default(),
            limits: Default::default(),
            requests: 0,
            completions: 0,
            wide: None,
            groups: Default::default(),
            redraw,
        })
    }
    #[cfg(any(test, feature = "test-support"))]
    pub fn snapshot(&self) -> ContentPresentationStats {
        ContentPresentationStats {
            device_service: Arc::as_ptr(&self.executor) as usize,
            retained_gpu_bytes: self.executor.retained_gpu_bytes(),
            requests: self.requests,
            completions: self.completions,
            cached_images: self.cache.len(),
            retained_slots: self.slots.len(),
            committed_groups: self.groups.committed_count(),
            pending: self.pending.is_some(),
            last_error: self
                .failed
                .as_ref()
                .map(|(_, e)| e.clone())
                .or_else(|| self.groups.last_error().cloned()),
            execution: self.stats.clone(),
        }
    }
    #[cfg(any(test, feature = "test-support", feature = "bench-support"))]
    pub fn require_ready(&self) -> Result<()> {
        if let Some((_, error)) = &self.failed {
            return Err(error.clone().into());
        }
        if self.pending.is_some() || self.dependencies.is_pending() {
            return Err(c::ContentError::new(
                c::ContentErrorKind::Pending,
                "requested portable content is pending; retry the snapshot asynchronously",
            )
            .into());
        }
        self.groups.require_ready()?;
        Ok(())
    }
    pub fn take_redraw(&self) -> bool {
        self.redraw.swap(false, std::sync::atomic::Ordering::AcqRel)
    }
    pub fn set_wake(&mut self, wake: Arc<dyn Fn() + Send + Sync>) {
        let redraw = self.redraw.clone();
        self.wake = Arc::new(move || {
            redraw.store(true, std::sync::atomic::Ordering::Release);
            wake();
        });
    }
    pub fn resolve_scene(
        &mut self,
        scene: &Scene,
        wide: bool,
    ) -> Result<Option<c::ResolvedContentScene<GpuImage>>> {
        let Some(request) = c::ContentGroupScene::describe(scene, |draw| {
            let prep = gpui::PreparationRequest::new(
                draw.bounds.size.map(|d| gpui::px(d.0)),
                crate::custom_draw::extent(draw)?,
                1.,
            )?;
            Ok(draw.content.portable_content(&prep)?)
        })?
        else {
            return Ok(None);
        };
        let tickets = self.groups.observe(&request.groups)?;
        let images = self.update_grouped(&request.draws, wide, &tickets)?;
        request.report_errors(&mut self.groups, &tickets);
        Ok(Some(request.resolve(
            scene,
            &self.groups,
            &tickets,
            &images,
        )?))
    }
    fn fail(&mut self, signature: Signature, error: c::ContentError) {
        log::error!("portable content generation failed: {error}");
        self.failed = Some((signature, error));
    }
    pub fn update(
        &mut self,
        draws: &[Option<c::ContentDrawRequest>],
        wide: bool,
    ) -> Result<Vec<Option<GpuImage>>> {
        self.groups.observe(&[])?;
        self.update_grouped(draws, wide, &[])
    }
    fn update_grouped(
        &mut self,
        draws: &[Option<c::ContentDrawRequest>],
        wide: bool,
        groups: &[c::ContentGroupTicket],
    ) -> Result<Vec<Option<GpuImage>>> {
        if self.wide != Some(wide) {
            self.wide = Some(wide);
            self.failed = None;
            self.groups.clear_errors();
            self.cache.retain(|_, image| {
                wide || !matches!(
                    image.descriptor.interpretation().color,
                    c::ColorEncoding::LinearP3 | c::ColorEncoding::LinearRec2020
                )
            });
            self.slots.retain(|_, key| self.cache.contains_key(key));
            self.groups.retain_images(|image| {
                wide || !matches!(
                    image.descriptor.interpretation().color,
                    c::ColorEncoding::LinearP3 | c::ColorEncoding::LinearRec2020
                )
            });
        }
        let active: Vec<_> = draws.iter().flatten().collect();
        let mut slots = HashSet::new();
        for draw in &active {
            if let Some(slot) = draw.draw.presentation.slot
                && !slots.insert(slot)
            {
                return Err(c::ContentError::new(
                    c::ContentErrorKind::InvalidDescriptor,
                    "duplicate portable presentation slot in one scene",
                )
                .into());
            }
        }
        self.slots.retain(|slot, _| slots.contains(slot));
        let signature: Signature = active
            .iter()
            .map(|draw| (draw.draw.content.id(), draw.request.clone()))
            .collect();
        if self.dependencies.changed() {
            self.description = None;
            self.failed = None;
            self.dependencies = c::ContentDependencies::default();
        }
        if self.failed.as_ref().is_some_and(|(s, _)| *s != signature) {
            self.failed = None;
        }
        let roots: Vec<_> = active
            .iter()
            .map(|draw| c::ContentRoot {
                content: draw.draw.content.clone(),
                request: draw.request.clone(),
            })
            .collect();
        if self
            .description
            .as_ref()
            .is_some_and(|(old, _)| *old != signature)
        {
            self.description = None;
            self.dependencies = c::ContentDependencies::default();
        }
        if roots.is_empty() {
            self.dependencies = c::ContentDependencies::default();
        }
        let graph = if let Some((_, graph)) = &self.description {
            Some(graph.clone())
        } else if roots.is_empty() || self.failed.is_some() {
            None
        } else {
            let (result, dependencies) = c::ContentGraph::describe_tracked(
                &roots,
                self.executor.capabilities().clone(),
                c::GraphLimits::default(),
                self.wake.clone(),
            );
            self.dependencies = dependencies;
            match result {
                Ok(graph) => {
                    let graph = Arc::new(graph);
                    self.description = Some((signature.clone(), graph.clone()));
                    Some(graph)
                }
                Err(error) => {
                    if error.kind != c::ContentErrorKind::Pending {
                        self.fail(signature.clone(), error);
                    }
                    None
                }
            }
        };
        let graph = graph.filter(|graph| {
            let unsupported = !wide
                && graph.roots().iter().any(|id| {
                    let c::ResourceDescriptor::Image(d) =
                        &graph.nodes()[id.node()].outputs()[id.output()]
                    else {
                        unreachable!()
                    };
                    matches!(
                        d.interpretation().color,
                        c::ColorEncoding::LinearP3 | c::ColorEncoding::LinearRec2020
                    )
                });
            if unsupported && self.failed.is_none() {
                self.fail(
                    signature.clone(),
                    c::ContentError::new(
                        c::ContentErrorKind::UnsupportedCapability,
                        "wide portable content requires a wide-capable compositor; wgpu currently presents sRGB",
                    ),
                );
            }
            !unsupported
        });
        let keys: Vec<_> = graph
            .as_ref()
            .map(|graph| {
                graph
                    .roots()
                    .iter()
                    .map(|id| graph.nodes()[id.node()].output_key(id.output()).unwrap())
                    .collect()
            })
            .unwrap_or_default();
        if let Some(pending) = &self.pending
            && let Some(result) = pending.job.take()
        {
            let pending = self.pending.take().unwrap();
            match result {
                Ok(frame) => {
                    self.stats = frame.stats;
                    self.completions += 1;
                    for ticket in &pending.groups {
                        let images = &frame.gpu[ticket.roots.clone()];
                        if wide
                            || images.iter().all(|image| {
                                !matches!(
                                    image.descriptor.interpretation().color,
                                    c::ColorEncoding::LinearP3 | c::ColorEncoding::LinearRec2020
                                )
                            })
                        {
                            self.groups.publish(ticket, images.to_vec());
                        }
                    }
                    // Consume completed work before a newer input can cancel it.
                    // Its destinations may retain these pixels while the newest
                    // generation runs, even if no frame displayed this result yet.
                    for ((key, slot), image) in pending
                        .keys
                        .into_iter()
                        .zip(pending.presentation_slots)
                        .zip(frame.gpu)
                    {
                        if !wide
                            && matches!(
                                image.descriptor.interpretation().color,
                                c::ColorEncoding::LinearP3 | c::ColorEncoding::LinearRec2020
                            )
                        {
                            continue;
                        }
                        if let Some(slot) = slot.filter(|slot| slots.contains(slot)) {
                            self.slots.insert(slot, key.clone());
                        }
                        self.cache.insert(key, image);
                    }
                }
                Err(error) if pending.keys == keys => self.fail(signature.clone(), error),
                Err(_) => {} // A failure of a replaced request is not the new request's failure.
            }
        }
        // Coalesce changing inputs behind the single running graph. Canceling
        // that graph on every frame can starve its later outputs indefinitely,
        // especially on browser devices with asynchronous validation/completion.
        // Harvest its slot history, then submit the latest description above.
        // Scene removal still cancels work and releases subscriptions promptly.
        if active.is_empty() {
            self.pending = None;
        }
        // Evict unused outputs before asking the shared device service for more
        // memory. Slot history and native sampling submissions keep their leases.
        self.cache
            .retain(|key, _| keys.contains(key) || self.slots.values().any(|old| old == key));
        if self.pending.is_none()
            && self.failed.is_none()
            && keys.iter().any(|key| !self.cache.contains_key(key))
            && let Some(graph) = graph
        {
            match self
                .executor
                .submit_gpu(graph, self.limits.clone(), self.wake.clone())
            {
                Ok(job) => {
                    self.requests += 1;
                    self.pending = Some(Pending {
                        groups: groups.to_vec(),
                        keys: keys.clone(),
                        presentation_slots: active
                            .iter()
                            .map(|draw| draw.draw.presentation.slot)
                            .collect(),
                        job,
                    });
                }
                Err(error) => self.fail(signature.clone(), error),
            }
        }
        for ticket in groups {
            if let Some((_, error)) = &self.failed {
                self.groups.fail(ticket, error.clone());
            } else if let Some(images) = keys.get(ticket.roots.clone()).and_then(|keys| {
                keys.iter()
                    .map(|key| self.cache.get(key).cloned())
                    .collect::<Option<Vec<_>>>()
            }) {
                self.groups.publish(ticket, images);
            }
        }
        let mut next_key = keys.iter();
        let mut output = Vec::with_capacity(draws.len());
        for draw in draws {
            let Some(draw) = draw else {
                output.push(None);
                continue;
            };
            let key = next_key.next();
            let policy = &draw.draw.presentation;
            let ready = key.and_then(|key| self.cache.get(key)).cloned();
            if let (Some(slot), Some(key), Some(_)) = (policy.slot, key, &ready) {
                self.slots.insert(slot, key.clone());
            }
            let failed = self.failed.as_ref().map(|(_, error)| error);
            if policy.on_error == c::ContentErrorPresentation::FailFrame
                && let Some(error) = failed
            {
                return Err(error.clone().into());
            }
            let retain = if failed.is_some() {
                policy.on_error == c::ContentErrorPresentation::ReportAndRetainPrevious
            } else {
                policy.pending == c::PendingPresentation::RetainPrevious
            };
            output.push(ready.or_else(|| {
                if retain {
                    policy
                        .slot
                        .and_then(|slot| self.slots.get(&slot))
                        .and_then(|key| self.cache.get(key))
                        .cloned()
                } else {
                    None
                }
            }));
        }
        self.cache
            .retain(|key, _| keys.contains(key) || self.slots.values().any(|old| old == key));
        Ok(output)
    }
}

/// Test/diagnostic snapshot. Producer readbacks must remain zero for presentation.
#[cfg(any(test, feature = "test-support"))]
#[derive(Clone, Debug)]
pub struct ContentPresentationStats {
    /// Opaque observer identity for checking service sharing, not a native handle.
    pub device_service: usize,
    /// Device-service image bytes retained by caches and compositor submissions.
    pub retained_gpu_bytes: u64,
    /// Number of producer jobs accepted for this renderer.
    pub requests: usize,
    /// Requested generations promoted after native completion.
    pub completions: usize,
    /// Retained structural image outputs.
    pub cached_images: usize,
    /// Stable slots with prior valid results.
    pub retained_slots: usize,
    /// Complete atomic groups with retained geometry and image leases.
    pub committed_groups: usize,
    /// Whether a generation is still pending.
    pub pending: bool,
    /// Failure of the current generation, reported once.
    pub last_error: Option<c::ContentError>,
    /// Last successful producer job, excluding compositor validation readback.
    pub execution: WgpuContentStats,
}
