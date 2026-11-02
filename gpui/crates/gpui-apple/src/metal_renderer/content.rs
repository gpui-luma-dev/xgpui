//! Completed-GPU-image presentation. UI encoding never waits for producer work.
use super::*;
use crate::content_executor::*;
use gpui::content as c;
use std::{
    collections::{HashMap, HashSet},
    sync::{OnceLock, Weak},
};

type SourceSignature = Vec<(u64, c::ContentRequest)>;
type Signature = Vec<(
    u64,
    c::ContentRequest,
    c::ImageMapping,
    c::ContentColorOutput,
    c::ImageColorTransform,
)>;
struct Pending {
    color_outputs: Vec<c::ContentColorOutput>,
    keys: Vec<c::OutputKey>,
    presentation_slots: Vec<Option<c::PresentationSlotId>>,
    job: MetalGpuJob,
    groups: Vec<c::ContentGroupTicket>,
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
    dependencies: c::ContentDependencies,
    description: Option<(SourceSignature, Arc<c::ContentGraph>)>,
    pub stats: MetalContentStats,
    pub limits: MetalContentLimits,
    requests: usize,
    completions: usize,
    wide: Option<bool>,
    groups: c::ContentGroupState<MetalGpuImage>,
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
            dependencies: c::ContentDependencies::default(),
            description: None,
            stats: Default::default(),
            limits: Default::default(),
            requests: 0,
            completions: 0,
            wide: None,
            groups: Default::default(),
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

    /// Describe current members before selection, then replay complete committed
    /// snapshots as native image draws. No group-wide intermediate is allocated.
    pub fn resolve_scene(
        &mut self,
        scene: &Scene,
        target: Option<&metal::TextureRef>,
        wide: bool,
    ) -> Result<Option<Scene>> {
        if !scene
            .custom_draws
            .iter()
            .any(|draw| draw.content.is_content_group())
        {
            return Ok(None);
        }
        let mut requests = Vec::new();
        let mut group_requests = Vec::new();
        let mut entries = Vec::new();
        let mut invalid = Vec::new();
        let mut roots = 0;
        for draw in &scene.custom_draws {
            let index = requests.len();
            let prep = gpui::PreparationRequest::new(
                draw.bounds.size.map(|d| gpui::px(d.0)),
                size(
                    DevicePixels(draw.bounds.size.width.0.ceil() as i32),
                    DevicePixels(draw.bounds.size.height.0.ceil() as i32),
                ),
                1.,
            )?;
            if let Some(group) = draw.content.portable_group(&prep)? {
                let start = roots;
                let group_index = group_requests.len();
                match group.draws() {
                    Ok(members) => {
                        for member in members {
                            let request = draw_request(&member)?;
                            anyhow::ensure!(request.is_some(), "group member must be portable");
                            requests.push(request);
                            roots += 1;
                        }
                    }
                    Err(error) => invalid.push((group_index, error)),
                }
                group_requests.push((group, start..roots));
                entries.push((draw, Some(group_index), index));
            } else {
                let request = draw_request(draw)?;
                roots += usize::from(request.is_some());
                requests.push(request);
                entries.push((draw, None, index));
            }
        }
        let tickets = self.groups.observe(&group_requests)?;
        let images = self.update_grouped(&requests, wide, &tickets)?;
        // Geometry errors reject the group, not individual members. Apply these
        // after harvesting GPU work so an old success cannot erase a new failure.
        for (index, error) in invalid {
            if self.groups.fail(&tickets[index], error.clone()) {
                log::error!("portable content group failed: {error}");
            }
        }
        let Some(target) = target else {
            return Ok(Some(Scene::default()));
        };
        let mut replacements = HashMap::new();
        for (draw, group_index, index) in entries {
            let selected = if let Some(group_index) = group_index {
                let mut selected = Vec::new();
                if let Some(snapshot) = self.groups.select(&tickets[group_index])? {
                    let mut request = snapshot.request.clone();
                    request.scale_factor = group_requests[group_index].0.scale_factor;
                    for (mut member, image) in request.draws()?.into_iter().zip(&snapshot.images) {
                        translate_member(&mut member, draw);
                        member.content = Arc::new(ResolvedContent::new(
                            Some(image.clone()),
                            target,
                            member.content.clone(),
                            draw_request(&member)?.expect("portable group member").draw,
                        ));
                        selected.push(member);
                    }
                }
                selected
            } else {
                let mut member = draw.clone();
                if requests[index].is_some() {
                    member.content = Arc::new(ResolvedContent::new(
                        images[index].clone(),
                        target,
                        member.content.clone(),
                        draw_request(&member)?
                            .expect("portable independent draw")
                            .draw,
                    ));
                }
                vec![member]
            };
            replacements
                .entry(draw_identity(draw))
                .or_insert_with(std::collections::VecDeque::new)
                .push_back(selected);
        }
        Ok(Some(scene.resolve_custom_draws(|draw| {
            replacements
                .get_mut(&draw_identity(draw))
                .and_then(|queue| queue.pop_front())
                .ok_or_else(|| anyhow::anyhow!("missing resolved content boundary"))
        })?))
    }

    pub fn set_wake(&mut self, wake: Arc<dyn Fn() + Send + Sync>) {
        self.wake = wake;
    }
    fn fail(&mut self, signature: Signature, error: c::ContentError) {
        if self
            .failed
            .as_ref()
            .is_some_and(|(old, previous)| *old == signature && *previous == error)
        {
            return;
        }
        log::error!("portable content generation failed: {error}");
        self.failed = Some((signature, error));
    }
    pub fn update(
        &mut self,
        draws: &[Option<c::ContentDrawRequest>],
        wide: bool,
    ) -> Result<Vec<Option<MetalGpuImage>>> {
        self.groups.observe(&[])?;
        self.update_grouped(draws, wide, &[])
    }
    fn update_grouped(
        &mut self,
        draws: &[Option<c::ContentDrawRequest>],
        wide: bool,
        groups: &[c::ContentGroupTicket],
    ) -> Result<Vec<Option<MetalGpuImage>>> {
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
            self.groups.retain_snapshots(|request, images| {
                images
                    .iter()
                    .zip(request.color_outputs())
                    .all(|(image, output)| {
                        wide || (output != c::ContentColorOutput::WideSdr
                            && (output == c::ContentColorOutput::SrgbPreview
                                || !matches!(
                                    image.descriptor.interpretation().color,
                                    c::ColorEncoding::LinearP3 | c::ColorEncoding::LinearRec2020
                                )))
                    })
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
            .map(|draw| {
                (
                    draw.draw.content.id(),
                    draw.request.clone(),
                    draw.draw.mapping,
                    draw.draw.presentation.color_output,
                    draw.draw.color_transform,
                )
            })
            .collect();
        let source_signature: SourceSignature = active
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
        if !wide
            && active
                .iter()
                .any(|draw| draw.draw.presentation.color_output == c::ContentColorOutput::WideSdr)
        {
            self.fail(
                signature.clone(),
                c::ContentError::new(
                    c::ContentErrorKind::UnsupportedCapability,
                    "explicit wide content requires an accepted wide SDR output",
                ),
            );
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
            .is_some_and(|(old, _)| *old != source_signature)
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
                    self.description = Some((source_signature.clone(), graph.clone()));
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
                && graph.roots().iter().zip(&active).any(|(id, draw)| {
                    let c::ResourceDescriptor::Image(d) =
                        &graph.nodes()[id.node()].outputs()[id.output()]
                    else {
                        unreachable!()
                    };
                    draw.draw.presentation.color_output == c::ContentColorOutput::WideSdr
                        || (draw.draw.presentation.color_output
                            != c::ContentColorOutput::SrgbPreview
                            && matches!(
                                d.interpretation().color,
                                c::ColorEncoding::LinearP3 | c::ColorEncoding::LinearRec2020
                            ))
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
        // Mapping validation belongs to presentation, never to upload identity.
        // Reject the whole requested group before it can publish an invalid snapshot.
        let graph = graph.filter(|graph| {
            for (draw, root) in active.iter().zip(graph.roots()) {
                let c::ResourceDescriptor::Image(descriptor) =
                    &graph.nodes()[root.node()].outputs()[root.output()]
                else {
                    continue;
                };
                if let Err(error) = draw.draw.color_transform.validate(
                    descriptor,
                    wide && draw.draw.presentation.color_output
                        != c::ContentColorOutput::SrgbPreview,
                ) {
                    self.fail(signature.clone(), error);
                    return false;
                }
                let destination = draw.request.logical_size();
                if let Err(error) = draw.draw.mapping.resolve(
                    descriptor,
                    [f32::from(destination.width), f32::from(destination.height)],
                ) {
                    self.fail(signature.clone(), error);
                    return false;
                }
            }
            true
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
                        let images = &frame.images[ticket.roots.clone()];
                        if wide
                            || images
                                .iter()
                                .zip(&pending.color_outputs[ticket.roots.clone()])
                                .all(|(image, output)| {
                                    *output != c::ContentColorOutput::WideSdr
                                        && (*output == c::ContentColorOutput::SrgbPreview
                                            || !matches!(
                                                image.descriptor.interpretation().color,
                                                c::ColorEncoding::LinearP3
                                                    | c::ColorEncoding::LinearRec2020
                                            ))
                                })
                        {
                            self.groups.publish(ticket, images.to_vec());
                        }
                    }
                    // Consume completed work before a newer input can cancel it.
                    // Its destinations may retain these pixels while the newest
                    // generation runs, even if no frame displayed this result yet.
                    for (((key, slot), output), image) in pending
                        .keys
                        .into_iter()
                        .zip(pending.presentation_slots)
                        .zip(pending.color_outputs)
                        .zip(frame.images)
                    {
                        if !wide
                            && (output == c::ContentColorOutput::WideSdr
                                || (output != c::ContentColorOutput::SrgbPreview
                                    && matches!(
                                        image.descriptor.interpretation().color,
                                        c::ColorEncoding::LinearP3
                                            | c::ColorEncoding::LinearRec2020
                                    )))
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
                        color_outputs: active
                            .iter()
                            .map(|draw| draw.draw.presentation.color_output)
                            .collect(),
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
            output.push(
                ready
                    .or_else(|| {
                        if retain {
                            policy
                                .slot
                                .and_then(|slot| self.slots.get(&slot))
                                .and_then(|key| self.cache.get(key))
                                .cloned()
                        } else {
                            None
                        }
                    })
                    .filter(|image| {
                        if !wide
                            && draw.draw.presentation.color_output == c::ContentColorOutput::WideSdr
                        {
                            return false;
                        }
                        if !wide
                            && draw.draw.presentation.color_output
                                != c::ContentColorOutput::SrgbPreview
                            && matches!(
                                image.descriptor.interpretation().color,
                                c::ColorEncoding::LinearP3 | c::ColorEncoding::LinearRec2020
                            )
                        {
                            return false;
                        }
                        if draw
                            .draw
                            .color_transform
                            .validate(
                                &image.descriptor,
                                wide && draw.draw.presentation.color_output
                                    != c::ContentColorOutput::SrgbPreview,
                            )
                            .is_err()
                        {
                            return false;
                        }
                        // A valid crop on the replacement may not fit a retained source.
                        // Such history is unavailable until compatible pixels are ready.
                        let destination = draw.request.logical_size();
                        draw.draw
                            .mapping
                            .resolve(
                                &image.descriptor,
                                [f32::from(destination.width), f32::from(destination.height)],
                            )
                            .is_ok()
                    }),
            );
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
    /// Complete atomic groups whose native images and geometry are retained.
    pub committed_groups: usize,
    /// Whether a generation is still pending.
    pub pending: bool,
    /// Failure of the current generation, reported once.
    pub last_error: Option<c::ContentError>,
    /// Last successful producer job, excluding compositor validation readback.
    pub execution: MetalContentStats,
}

fn draw_identity(draw: &gpui::CustomDraw) -> (usize, u32, [u32; 4]) {
    (
        Arc::as_ptr(&draw.content) as *const () as usize,
        draw.order,
        [
            draw.bounds.origin.x.0.to_bits(),
            draw.bounds.origin.y.0.to_bits(),
            draw.bounds.size.width.0.to_bits(),
            draw.bounds.size.height.0.to_bits(),
        ],
    )
}
fn draw_request(draw: &gpui::CustomDraw) -> Result<Option<c::ContentDrawRequest>> {
    let request = gpui::PreparationRequest::new(
        draw.bounds.size.map(|d| gpui::px(d.0)),
        crate::custom_draw::CustomCompositor::extent(draw)?,
        1.,
    )?;
    Ok(draw.content.portable_content(&request)?)
}
fn translate_member(member: &mut gpui::CustomDraw, placement: &gpui::CustomDraw) {
    let origin = placement.bounds.origin;
    member.bounds.origin += origin;
    member.content_mask = gpui::ContentMask {
        bounds: placement.content_mask.bounds.intersect(&placement.bounds),
    };
    member.opacity *= placement.opacity;
    if let Some(gpui::CustomCoverage::Path(path)) = &mut member.coverage {
        let path = Arc::make_mut(path);
        path.bounds.origin += origin;
        path.content_mask = member.content_mask;
        for vertex in &mut path.vertices {
            vertex.xy_position += origin;
            vertex.content_mask = member.content_mask;
        }
    }
}
// SAFETY: these retained Metal handles are only cloned/released here. Image
// pixels are immutable and leased through sampling completion; the unused target
// is never sampled (invisible output). This wrapper never records or mutates GPU work.
unsafe impl Send for ResolvedContent {}
unsafe impl Sync for ResolvedContent {}
struct ResolvedContent {
    draw: c::ContentDraw,
    image: Option<MetalGpuImage>,
    unused: Option<metal::Texture>,
    source: Arc<dyn gpui::CustomContent>,
}
impl ResolvedContent {
    fn new(
        image: Option<MetalGpuImage>,
        target: &metal::TextureRef,
        source: Arc<dyn gpui::CustomContent>,
        draw: c::ContentDraw,
    ) -> Self {
        let unused = image.is_none().then(|| target.into());
        Self {
            draw,
            image,
            unused,
            source,
        }
    }
}
impl std::fmt::Debug for ResolvedContent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ResolvedMetalContent")
    }
}
impl gpui::CustomContent for ResolvedContent {
    fn image_color_output(&self) -> c::ContentColorOutput {
        self.draw.presentation.color_output
    }
    fn image_mapping(&self) -> Option<c::ImageMapping> {
        Some(self.draw.mapping)
    }
    fn image_color_transform(&self) -> [[f32; 4]; 3] {
        self.draw.color_transform.encoded_rows()
    }
    fn image_source_size(&self) -> Option<Size<DevicePixels>> {
        self.source.image_source_size()
    }
    fn image_sample_opacity(&self) -> f32 {
        self.source.image_sample_opacity()
    }
    fn image_color_mode(&self) -> gpui::ImageColorMode {
        self.source.image_color_mode()
    }
    fn image_linear_adjustments(&self) -> Option<gpui::LinearImagePaint> {
        self.draw.color_transform.linear_paint()
    }
    fn prepare(
        &self,
        _: &dyn std::any::Any,
        _: Size<DevicePixels>,
    ) -> Result<Box<dyn std::any::Any>> {
        let mut output = crate::custom_draw::PreparedMetalOutput::portable(
            self.image.clone(),
            self.image
                .as_ref()
                .map(|image| image.texture())
                .unwrap_or_else(|| self.unused.as_ref().unwrap()),
        );
        output.color_transform = Some(self.draw.color_transform);
        output.color_output = self.draw.presentation.color_output;
        Ok(Box::new(output))
    }
}
