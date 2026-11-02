//! CPU-only scene expansion shared by portable presentation backends. Native
//! image owners stay in renderer-owned vectors, never in Send/Sync scene content.
use super::*;
use crate::{CustomCoverage, CustomDraw, DevicePixels, PreparationRequest, Scene, size};
use std::{
    collections::{HashMap, VecDeque},
    ops::Range,
    sync::Arc,
};

type DrawIdentity = (usize, u32, [u32; 4]);
fn identity(draw: &CustomDraw) -> DrawIdentity {
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
struct Entry {
    draw: CustomDraw,
    group: Option<usize>,
    flat: usize,
}
/// Flattened requests and original paint boundaries for one window scene.
/// Construction and expansion do not allocate, submit or wait on GPU work.
pub struct ContentGroupScene {
    /// Per-member/independent requests in custom paint order, including native holes.
    pub draws: Vec<Option<ContentDrawRequest>>,
    /// Active groups and their root ranges, excluding native holes.
    pub groups: Vec<(ContentGroupRequest, Range<usize>)>,
    entries: Vec<Entry>,
    errors: Vec<(usize, ContentError)>,
}
/// Render-only selected scene and image leases aligned with its custom-draw array.
pub struct ResolvedContentScene<T> {
    /// Original scene order with each group replaced by one complete snapshot.
    pub scene: Scene,
    /// Native outputs, kept outside the CPU scene's thread-safe content descriptions.
    pub images: Vec<Option<T>>,
}
impl ContentGroupScene {
    /// Return None for an independent-only scene so existing fast paths remain
    /// available. The callback performs the backend's normal request validation.
    pub fn describe(
        scene: &Scene,
        mut request: impl FnMut(&CustomDraw) -> anyhow::Result<Option<ContentDrawRequest>>,
    ) -> anyhow::Result<Option<Self>> {
        if !scene
            .custom_draws
            .iter()
            .any(|draw| draw.content.is_content_group())
        {
            return Ok(None);
        }
        let mut result = Self {
            draws: Vec::new(),
            groups: Vec::new(),
            entries: Vec::new(),
            errors: Vec::new(),
        };
        let mut roots = 0;
        for draw in &scene.custom_draws {
            let flat = result.draws.len();
            let prep = PreparationRequest::new(
                draw.bounds.size.map(|d| crate::px(d.0)),
                size(
                    DevicePixels(draw.bounds.size.width.0.ceil() as i32),
                    DevicePixels(draw.bounds.size.height.0.ceil() as i32),
                ),
                1.,
            )?;
            let group = if let Some(group) = draw.content.portable_group(&prep)? {
                let start = roots;
                let index = result.groups.len();
                // Materialize the entire group before admitting any subset.
                let members = group.draws().and_then(|members| {
                    members
                        .into_iter()
                        .map(|member| {
                            request(&member)
                                .map_err(|e| {
                                    e.downcast_ref::<ContentError>()
                                        .cloned()
                                        .unwrap_or_else(|| {
                                            ContentError::new(
                                                ContentErrorKind::InvalidDescriptor,
                                                e.to_string(),
                                            )
                                        })
                                })
                                .and_then(|r| {
                                    r.ok_or_else(|| {
                                        ContentError::new(
                                            ContentErrorKind::InvalidDescriptor,
                                            "group member must be portable",
                                        )
                                    })
                                })
                        })
                        .collect::<std::result::Result<Vec<_>, _>>()
                });
                match members {
                    Ok(members) => {
                        roots += members.len();
                        result.draws.extend(members.into_iter().map(Some));
                    }
                    Err(error) => result.errors.push((index, error)),
                }
                result.groups.push((group, start..roots));
                Some(index)
            } else {
                let r = request(draw)?;
                roots += usize::from(r.is_some());
                result.draws.push(r);
                None
            };
            result.entries.push(Entry {
                draw: draw.clone(),
                group,
                flat,
            });
        }
        Ok(Some(result))
    }
    /// Apply member validation failures after harvesting older admitted work.
    pub fn report_errors<T: Clone>(
        &self,
        state: &mut ContentGroupState<T>,
        tickets: &[ContentGroupTicket],
    ) {
        for (index, error) in &self.errors {
            if state.fail(&tickets[*index], error.clone()) {
                log::error!("portable content group failed: {error}");
            }
        }
    }
    /// Expand only complete committed snapshots, then align image leases with the
    /// scene's stable custom-draw order. All unrelated primitives keep their order.
    pub fn resolve<T: Clone>(
        &self,
        scene: &Scene,
        state: &ContentGroupState<T>,
        tickets: &[ContentGroupTicket],
        images: &[Option<T>],
    ) -> anyhow::Result<ResolvedContentScene<T>> {
        anyhow::ensure!(
            tickets.len() == self.groups.len() && images.len() == self.draws.len(),
            ContentError::new(
                ContentErrorKind::InvalidDescriptor,
                "atomic scene selections do not match the requested draw/group counts"
            )
        );
        let mut replacements: HashMap<DrawIdentity, VecDeque<Vec<(CustomDraw, Option<T>)>>> =
            HashMap::new();
        for entry in &self.entries {
            let selected = if let Some(index) = entry.group {
                let mut selected = Vec::new();
                if let Some(snapshot) = state.select(&tickets[index])? {
                    let mut request = snapshot.request.clone();
                    request.scale_factor = self.groups[index].0.scale_factor;
                    for (mut member, image) in request.draws()?.into_iter().zip(&snapshot.images) {
                        place(&mut member, &entry.draw);
                        selected.push((member, Some(image.clone())));
                    }
                }
                selected
            } else {
                vec![(
                    entry.draw.clone(),
                    images
                        .get(entry.flat)
                        .cloned()
                        .ok_or_else(|| anyhow::anyhow!("missing independent image selection"))?,
                )]
            };
            replacements
                .entry(identity(&entry.draw))
                .or_default()
                .push_back(selected);
        }
        let mut ordered = Vec::new();
        let resolved = scene.resolve_custom_draws(|draw| {
            let selected = replacements
                .get_mut(&identity(draw))
                .and_then(VecDeque::pop_front)
                .ok_or_else(|| anyhow::anyhow!("missing atomic content boundary"))?;
            Ok(selected
                .into_iter()
                .map(|(member, image)| {
                    ordered.push((draw.order, image));
                    member
                })
                .collect())
        })?;
        anyhow::ensure!(
            replacements.values().all(VecDeque::is_empty),
            ContentError::new(
                ContentErrorKind::InvalidDescriptor,
                "atomic group scene requires retained insertion/replay records"
            )
        );
        // Scene::finish sorts stably by order, including children of explicit layers.
        ordered.sort_by_key(|(order, _)| *order);
        let images = ordered.into_iter().map(|(_, image)| image).collect();
        Ok(ResolvedContentScene {
            scene: resolved,
            images,
        })
    }
}
fn place(member: &mut CustomDraw, placement: &CustomDraw) {
    let origin = placement.bounds.origin;
    member.bounds.origin += origin;
    member.content_mask = crate::ContentMask {
        bounds: placement.content_mask.bounds.intersect(&placement.bounds),
    };
    member.opacity *= placement.opacity;
    if let Some(CustomCoverage::Path(path)) = &mut member.coverage {
        let path = Arc::make_mut(path);
        path.bounds.origin += origin;
        path.content_mask = member.content_mask;
        for vertex in &mut path.vertices {
            vertex.xy_position += origin;
            vertex.content_mask = member.content_mask;
        }
    }
}
