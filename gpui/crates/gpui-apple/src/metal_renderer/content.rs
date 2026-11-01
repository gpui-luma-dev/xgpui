//! Completed-GPU-image presentation. UI encoding never waits for producer work.
use super::*;
use crate::content_executor::*;
use gpui::content as c;
use std::{
    collections::{HashMap, HashSet},
    sync::{OnceLock, Weak},
};

type Signature = Vec<(u64, c::ContentRequest)>;
struct Pending {
    keys: Vec<c::OutputKey>,
    presentation_slots: Vec<Option<c::PresentationSlotId>>,
    job: MetalGpuJob,
}
/// The worker and exported-output budget are shared by every window on a device.
/// Weak registry entries do not keep unused devices/workers alive.
fn service(device: &metal::Device) -> Result<Arc<MetalContentExecutor>> {
    static SERVICES: OnceLock<std::sync::Mutex<HashMap<u64, Weak<MetalContentExecutor>>>> =
        OnceLock::new();
    let mut services = SERVICES.get_or_init(Default::default).lock().unwrap();
    let key = device.registryID();
    if let Some(service) = services.get(&key).and_then(Weak::upgrade) {
        return Ok(service);
    }
    let service = Arc::new(MetalContentExecutor::on_device(
        MetalContentOptions {
            pending_jobs: 8,
            ..Default::default()
        },
        Some(device.clone()),
    )?);
    services.retain(|_, service| service.strong_count() != 0);
    services.insert(key, Arc::downgrade(&service));
    Ok(service)
}
pub(super) struct ContentPresenter {
    executor: Arc<MetalContentExecutor>,
    wake: Arc<dyn Fn() + Send + Sync>,
    pending: Option<Pending>,
    cache: HashMap<c::OutputKey, MetalGpuImage>,
    slots: HashMap<c::PresentationSlotId, c::OutputKey>,
    failed: Option<(Signature, c::ContentError)>,
    description: Option<(Signature, Arc<c::ContentGraph>)>,
    pub stats: MetalContentStats,
    pub limits: MetalContentLimits,
    requests: usize,
    completions: usize,
    wide: Option<bool>,
}
impl ContentPresenter {
    pub fn new(device: &metal::Device) -> Result<Self> {
        Ok(Self {
            executor: service(device)?,
            wake: Arc::new(|| {}),
            pending: None,
            cache: HashMap::new(),
            slots: HashMap::new(),
            failed: None,
            description: None,
            stats: Default::default(),
            limits: Default::default(),
            requests: 0,
            completions: 0,
            wide: None,
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
            pending: self.pending.is_some(),
            last_error: self.failed.as_ref().map(|(_, e)| e.clone()),
            execution: self.stats.clone(),
        }
    }
    #[cfg(any(test, feature = "test-support", feature = "bench-support"))]
    pub fn require_ready(&self) -> Result<()> {
        if let Some((_, error)) = &self.failed {
            return Err(error.clone().into());
        }
        if self.pending.is_some() {
            return Err(c::ContentError::new(
                c::ContentErrorKind::Pending,
                "requested portable content is pending; retry the snapshot asynchronously",
            )
            .into());
        }
        Ok(())
    }
    pub fn set_wake(&mut self, wake: Arc<dyn Fn() + Send + Sync>) {
        self.wake = wake;
    }
    fn fail(&mut self, signature: Signature, error: c::ContentError) {
        log::error!("portable content generation failed: {error}");
        self.failed = Some((signature, error));
    }
    pub fn update(
        &mut self,
        draws: &[Option<c::ContentDrawRequest>],
        wide: bool,
    ) -> Result<Vec<Option<MetalGpuImage>>> {
        if self.wide != Some(wide) {
            self.wide = Some(wide);
            self.failed = None;
            self.cache.retain(|_, image| {
                wide || !matches!(
                    image.descriptor.interpretation().color,
                    c::ColorEncoding::LinearP3 | c::ColorEncoding::LinearRec2020
                )
            });
            self.slots.retain(|_, key| self.cache.contains_key(key));
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
        }
        let graph = if let Some((_, graph)) = &self.description {
            Some(graph.clone())
        } else if roots.is_empty() || self.failed.is_some() {
            None
        } else {
            match c::ContentGraph::describe(
                &roots,
                self.executor.capabilities().clone(),
                c::GraphLimits::default(),
            ) {
                Ok(graph) => {
                    let graph = Arc::new(graph);
                    self.description = Some((signature.clone(), graph.clone()));
                    Some(graph)
                }
                Err(error) => {
                    self.fail(signature.clone(), error);
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
                        "wide portable content requires WideGamutSdr window presentation",
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
                    // Consume completed work before a newer input can cancel it.
                    // Its destinations may retain these pixels while the newest
                    // generation runs, even if no frame displayed this result yet.
                    for ((key, slot), image) in pending
                        .keys
                        .into_iter()
                        .zip(pending.presentation_slots)
                        .zip(frame.images)
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
        // Let the single admitted graph finish while newer inputs coalesce.
        // Restarting it on each edit can indefinitely starve later outputs.
        // Completed slot history is harvested above before the latest graph
        // is admitted. Removing the scene still cancels unfinished work.
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
    /// Whether a generation is still pending.
    pub pending: bool,
    /// Failure of the current generation, reported once.
    pub last_error: Option<c::ContentError>,
    /// Last successful producer job, excluding compositor validation readback.
    pub execution: MetalContentStats,
}
