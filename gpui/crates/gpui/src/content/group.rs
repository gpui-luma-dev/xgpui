//! Atomic presentation of an ordered set of independently produced images.
use super::*;
use crate::{
    Bounds, ContentMask, Corners, CustomContent, CustomCoverage, CustomDraw, CustomGeometry,
    DevicePixels, Pixels, PreparationRequest, ScaledPixels, Size, point, size,
};
use std::{any::Any, collections::HashMap, ops::Range, sync::Arc};
type Result<T> = std::result::Result<T, ContentError>;
fn invalid(message: &str) -> ContentError {
    ContentError::new(ContentErrorKind::InvalidDescriptor, message)
}

/// Stable atomic paint identity. History is scoped to a window and retired on removal.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ContentGroupId(u64);
impl ContentGroupId {
    /// Allocate once per logical group, rather than once per render.
    pub fn new() -> Self {
        Self(next_content_identity())
    }
}
impl Default for ContentGroupId {
    fn default() -> Self {
        Self::new()
    }
}

/// Owned CPU description of one atomic presentation boundary. Member geometry
/// is local to the group's placement bounds. Members have no independent history.
#[derive(Clone)]
pub struct ContentGroup {
    id: ContentGroupId,
    revision: u64,
    members: Vec<(CustomGeometry, ContentDraw)>,
    pending: PendingPresentation,
    on_error: ContentErrorPresentation,
}
impl std::fmt::Debug for ContentGroup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ContentGroup")
            .field("id", &self.id)
            .field("revision", &self.revision)
            .field("members", &self.members.len())
            .finish()
    }
}
impl ContentGroup {
    /// Start an ordered group, retaining its complete previous presentation while pending.
    pub fn new(id: ContentGroupId) -> Self {
        Self {
            id,
            revision: next_content_identity(),
            members: Vec::new(),
            pending: PendingPresentation::default(),
            on_error: ContentErrorPresentation::default(),
        }
    }
    /// Append a source or configured draw with group-local geometry. Draw mapping
    /// and color output are committed atomically. Independent member slots or
    /// pending/error policies are rejected when the group is described.
    pub fn push(&mut self, geometry: CustomGeometry, content: impl Into<ContentDraw>) {
        self.revision = next_content_identity();
        self.members.push((geometry, content.into()));
    }
    /// Append a mapped image member. Mapping is committed with this group's
    /// geometry and images, and remains unchanged while a replacement is pending.
    pub fn push_mapped(
        &mut self,
        geometry: CustomGeometry,
        content: Content,
        mapping: ImageMapping,
    ) {
        self.push(geometry, ContentDraw::new(content).mapping(mapping));
    }
    /// Set the policy for the whole group, including first presentation and failures.
    pub fn presentation(
        mut self,
        pending: PendingPresentation,
        on_error: ContentErrorPresentation,
    ) -> Self {
        self.revision = next_content_identity();
        self.pending = pending;
        self.on_error = on_error;
        self
    }
    /// Renderer bridge. Applications normally use `Window::paint_content_group`.
    pub fn into_scene_content(self) -> Arc<dyn CustomContent> {
        Arc::new(self)
    }
}
impl CustomContent for ContentGroup {
    fn is_content_group(&self) -> bool {
        true
    }
    fn portable_group(&self, request: &PreparationRequest) -> Result<Option<ContentGroupRequest>> {
        Ok(Some(ContentGroupRequest {
            group: self.clone(),
            scale_factor: request.scale_factor(),
        }))
    }
    fn prepare(&self, _: &dyn Any, _: Size<DevicePixels>) -> anyhow::Result<Box<dyn Any>> {
        Err(ContentError::new(
            ContentErrorKind::UnsupportedCapability,
            "this renderer does not support atomic content groups",
        )
        .into())
    }
}

/// CPU-only renderer bridge for an atomic group. The snapshot contains no native resources.
#[derive(Clone, Debug)]
pub struct ContentGroupRequest {
    /// Owned immutable members and presentation policy.
    pub group: ContentGroup,
    /// Scale used to derive exact member requests.
    pub scale_factor: f32,
}
impl ContentGroupRequest {
    /// Stable logical identity.
    pub fn id(&self) -> ContentGroupId {
        self.group.id
    }
    /// Output intent for each ordered member, without materializing geometry.
    pub fn color_outputs(&self) -> impl Iterator<Item = ContentColorOutput> + '_ {
        self.group
            .members
            .iter()
            .map(|(_, draw)| draw.presentation.color_output)
    }
    /// Validate all members and construct local physical geometry. An invalid
    /// member rejects the complete group; clipped members still participate.
    pub fn draws(&self) -> Result<Vec<CustomDraw>> {
        let scale = self.scale_factor;
        if !scale.is_finite() || scale <= 0. {
            return Err(invalid("invalid content group scale"));
        }
        self.group
            .members
            .iter()
            .map(|(geometry, content)| {
                if content.presentation.slot.is_some()
                    || content.presentation.pending != PendingPresentation::default()
                    || content.presentation.on_error != ContentErrorPresentation::default()
                {
                    return Err(invalid(
                        "group members cannot have independent slots or presentation policies",
                    ));
                }
                let (logical, radii, coverage) = match geometry {
                    CustomGeometry::RoundedRect {
                        bounds,
                        corner_radii,
                    } => (*bounds, *corner_radii, None),
                    CustomGeometry::Border(quad) => {
                        if ![
                            quad.border_widths.top.0,
                            quad.border_widths.right.0,
                            quad.border_widths.bottom.0,
                            quad.border_widths.left.0,
                        ]
                        .iter()
                        .all(|v| v.is_finite() && *v >= 0.)
                        {
                            return Err(invalid("invalid group border widths"));
                        }
                        (
                            *&quad.bounds,
                            quad.corner_radii,
                            Some(CustomCoverage::Border {
                                widths: quad.border_widths.map(|v| {
                                    ScaledPixels(crate::util::round_stroke_to_device_pixel(
                                        v.0, scale,
                                    ))
                                }),
                                style: quad.border_style,
                            }),
                        )
                    }
                    CustomGeometry::Path(path) => {
                        if path.vertices.is_empty()
                            || path.vertices.len() > 1_000_000
                            || path.vertices.iter().any(|v| {
                                ![
                                    v.xy_position.x.0,
                                    v.xy_position.y.0,
                                    v.st_position.x,
                                    v.st_position.y,
                                ]
                                .iter()
                                .all(|v| v.is_finite())
                            })
                        {
                            return Err(invalid("invalid group path"));
                        }
                        (
                            path.bounds,
                            Corners::default(),
                            Some(CustomCoverage::Path(Arc::new(path.scale(scale)))),
                        )
                    }
                };
                if ![
                    logical.origin.x.0,
                    logical.origin.y.0,
                    logical.size.width.0,
                    logical.size.height.0,
                    radii.top_left.0,
                    radii.top_right.0,
                    radii.bottom_left.0,
                    radii.bottom_right.0,
                ]
                .iter()
                .all(|v| v.is_finite())
                    || logical.size.width.0 <= 0.
                    || logical.size.height.0 <= 0.
                    || [
                        radii.top_left.0,
                        radii.top_right.0,
                        radii.bottom_left.0,
                        radii.bottom_right.0,
                    ]
                    .iter()
                    .any(|v| *v < 0.)
                {
                    return Err(invalid("invalid content group geometry"));
                }
                let bounds = if matches!(coverage, Some(CustomCoverage::Path(_))) {
                    logical.scale(scale)
                } else {
                    let round =
                        |v: Pixels| ScaledPixels(crate::util::round_to_device_pixel(v.0, scale));
                    Bounds::from_corners(
                        point(round(logical.left()), round(logical.top())),
                        point(round(logical.right()), round(logical.bottom())),
                    )
                };
                if ![
                    bounds.origin.x.0,
                    bounds.origin.y.0,
                    bounds.size.width.0,
                    bounds.size.height.0,
                ]
                .iter()
                .all(|v| v.is_finite())
                    || bounds.size.width.0 <= 0.
                    || bounds.size.height.0 <= 0.
                    || bounds.size.width.0 > 16384.
                    || bounds.size.height.0 > 16384.
                {
                    return Err(invalid("invalid physical content group geometry"));
                }
                match &coverage {
                    Some(CustomCoverage::Path(path))
                        if path.vertices.iter().any(|v| {
                            ![v.xy_position.x.0, v.xy_position.y.0]
                                .iter()
                                .all(|v| v.is_finite())
                        }) =>
                    {
                        return Err(invalid("scaled group path overflow"));
                    }
                    Some(CustomCoverage::Border { widths, .. })
                        if ![widths.top.0, widths.right.0, widths.bottom.0, widths.left.0]
                            .iter()
                            .all(|v| v.is_finite()) =>
                    {
                        return Err(invalid("scaled group border overflow"));
                    }
                    _ => {}
                }
                let raster = size(
                    DevicePixels(bounds.size.width.0.ceil() as i32),
                    DevicePixels(bounds.size.height.0.ceil() as i32),
                );
                let _ = ContentRequest::new(logical.size, ImageExtent::try_from(raster)?, scale)?;
                Ok(CustomDraw {
                    order: 0,
                    bounds,
                    content_mask: ContentMask { bounds },
                    corner_radii: radii.clamp_radii_for_quad_size(logical.size).scale(scale),
                    opacity: 1.,
                    content: crate::scene::content_with_preparation_geometry(
                        content
                            .clone()
                            .presentation(ContentPresentation {
                                color_output: content.presentation.color_output,
                                pending: PendingPresentation::Transparent,
                                ..Default::default()
                            })
                            .into_scene_content(),
                        logical.size,
                        scale,
                    ),
                    coverage,
                })
            })
            .collect()
    }
}

/// A generation token that cannot publish into a removed/reintroduced group.
#[derive(Clone, Debug)]
pub struct ContentGroupTicket {
    request: ContentGroupRequest,
    epoch: u64,
    generation: u64,
    /// Root indices in the admitted graph, excluding non-portable draws.
    pub roots: Range<usize>,
}
/// One complete committed group, retaining geometry and image leases together.
#[derive(Clone, Debug)]
pub struct ContentGroupSnapshot<T> {
    /// CPU member description from the committed generation.
    pub request: ContentGroupRequest,
    /// Complete native outputs in member order.
    pub images: Vec<T>,
    generation: u64,
}
struct GroupState<T> {
    key: (u64, u32),
    epoch: u64,
    generation: u64,
    snapshot: Option<ContentGroupSnapshot<T>>,
    error: Option<ContentError>,
    empty: bool,
}
/// Backend-independent generation selection. Native executors supply completed
/// image leases; this state machine never waits, submits work, or reads pixels.
pub struct ContentGroupState<T> {
    groups: HashMap<ContentGroupId, GroupState<T>>,
}
impl<T> Default for ContentGroupState<T> {
    fn default() -> Self {
        Self {
            groups: HashMap::new(),
        }
    }
}
impl<T: Clone> ContentGroupState<T> {
    /// Observe the entire active group set before harvesting completion. Removal
    /// retires its epoch; empty groups immediately clear history and invalidate work.
    pub fn observe(
        &mut self,
        requests: &[(ContentGroupRequest, Range<usize>)],
    ) -> Result<Vec<ContentGroupTicket>> {
        let mut ids = std::collections::HashSet::new();
        for (r, _) in requests {
            if !ids.insert(r.id()) {
                return Err(invalid("duplicate content group in one scene"));
            }
        }
        self.groups.retain(|id, _| ids.contains(id));
        Ok(requests
            .iter()
            .map(|(request, roots)| {
                let key = (request.group.revision, request.scale_factor.to_bits());
                let empty = request.group.members.is_empty();
                let state = self
                    .groups
                    .entry(request.id())
                    .or_insert_with(|| GroupState {
                        key,
                        epoch: next_content_identity(),
                        generation: 1,
                        snapshot: None,
                        error: None,
                        empty,
                    });
                if state.key != key {
                    state.key = key;
                    state.generation += 1;
                    state.error = None;
                }
                if empty && !state.empty {
                    state.epoch = next_content_identity();
                }
                state.empty = empty;
                if empty {
                    state.snapshot = None;
                }
                ContentGroupTicket {
                    request: request.clone(),
                    roots: roots.clone(),
                    epoch: state.epoch,
                    generation: state.generation,
                }
            })
            .collect())
    }
    /// Publish a complete admitted generation. Obsolete epochs and rollbacks are ignored.
    pub fn publish(&mut self, ticket: &ContentGroupTicket, images: Vec<T>) {
        if images.len() != ticket.request.group.members.len() {
            return;
        }
        if let Some(state) = self.groups.get_mut(&ticket.request.id())
            && state.epoch == ticket.epoch
            && !state.empty
            && state
                .snapshot
                .as_ref()
                .is_none_or(|s| ticket.generation > s.generation)
        {
            state.snapshot = Some(ContentGroupSnapshot {
                request: ticket.request.clone(),
                images,
                generation: ticket.generation,
            });
        }
    }
    /// Apply a failure only to the generation that requested it.
    pub fn fail(&mut self, ticket: &ContentGroupTicket, error: ContentError) -> bool {
        if let Some(state) = self.groups.get_mut(&ticket.request.id())
            && state.epoch == ticket.epoch
            && state.generation == ticket.generation
            && !state.empty
        {
            let changed = state.error.as_ref() != Some(&error);
            state.error = Some(error);
            return changed;
        }
        false
    }
    /// Select the entire committed snapshot according to the current group's policy.
    pub fn select(&self, ticket: &ContentGroupTicket) -> Result<Option<&ContentGroupSnapshot<T>>> {
        let Some(state) = self
            .groups
            .get(&ticket.request.id())
            .filter(|s| s.epoch == ticket.epoch)
        else {
            return Ok(None);
        };
        if let Some(error) = &state.error {
            return match ticket.request.group.on_error {
                ContentErrorPresentation::FailFrame => Err(error.clone()),
                ContentErrorPresentation::ReportAndTransparent => Ok(None),
                ContentErrorPresentation::ReportAndRetainPrevious => Ok(state.snapshot.as_ref()),
            };
        }
        Ok(state.snapshot.as_ref().filter(|s| {
            s.generation == ticket.generation
                || ticket.request.group.pending == PendingPresentation::RetainPrevious
        }))
    }
    /// Retry validation when the host changes presentation capabilities.
    pub fn clear_errors(&mut self) {
        for state in self.groups.values_mut() {
            state.error = None;
        }
    }
    /// Drop snapshots whose images cannot be presented under the current device/color policy.
    pub fn retain_snapshots(
        &mut self,
        mut supported: impl FnMut(&ContentGroupRequest, &[T]) -> bool,
    ) {
        for state in self.groups.values_mut() {
            if state
                .snapshot
                .as_ref()
                .is_some_and(|s| !supported(&s.request, &s.images))
            {
                state.snapshot = None;
            }
        }
    }
    /// Strict snapshot validation rejects a pending or failed current generation,
    /// even when normal presentation can retain an older complete snapshot.
    pub fn require_ready(&self) -> Result<()> {
        for state in self.groups.values() {
            if let Some(error) = &state.error {
                return Err(error.clone());
            }
            if !state.empty
                && state
                    .snapshot
                    .as_ref()
                    .is_none_or(|s| s.generation != state.generation)
            {
                return Err(ContentError::new(
                    ContentErrorKind::Pending,
                    "atomic content group is pending",
                ));
            }
        }
        Ok(())
    }
    /// Most recent group-local validation failure, for renderer diagnostics.
    pub fn last_error(&self) -> Option<&ContentError> {
        self.groups.values().find_map(|s| s.error.as_ref())
    }
    /// Active committed group count, for diagnostics and teardown tests.
    pub fn committed_count(&self) -> usize {
        self.groups
            .values()
            .filter(|s| s.snapshot.is_some())
            .count()
    }
}
