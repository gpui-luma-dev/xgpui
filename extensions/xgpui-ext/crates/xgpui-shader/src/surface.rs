//! Explicit extension containers. Custom materials do not enter native Style/Fill.
use crate::{Paint, WindowPaintExt};
use gpui::*;
use refineable::Refineable;
use std::{cell::Cell, rc::Rc, sync::Arc};

/// A normal layout container with extension-owned background/border materials.
/// Explicit materials override native background/border colors on this container.
/// Material changes are supplied by rebuilding it; native hover refinements do not
/// carry custom materials. Descendant interaction and parent clip/opacity are native.
pub struct Painted {
    root: Div,
    children: Vec<AnyElement>,
    background: Option<Paint>,
    border: Option<Paint>,
    #[cfg(feature = "portable-shaders")]
    shadows: Vec<crate::instanced::ShadowLayer>,
    #[cfg(feature = "portable-shaders")]
    animated_border: Option<crate::instanced::AnimatedBorderLayer>,
}
/// Create an opt-in extension paint container.
pub fn painted() -> Painted {
    Painted {
        root: div(),
        children: Vec::new(),
        background: None,
        border: None,
        #[cfg(feature = "portable-shaders")]
        shadows: Vec::new(),
        #[cfg(feature = "portable-shaders")]
        animated_border: None,
    }
}
/// Compatibility name for the original shader element.
pub type Shader = Painted;
/// Place a reusable producer in an extension-owned styled container.
pub fn shader(content: Arc<dyn CustomContent>) -> Shader {
    painted().bg_paint(content)
}
impl Painted {
    /// Add an immutable programmable shadow layer. Requires uniform corner radii.
    /// Outsets paint before the background; insets before children. Append layers
    /// in back-to-front order within each group. Base geometry/opacity are used;
    /// rebuild this container to update shadow styles.
    #[cfg(feature = "portable-shaders")]
    pub fn shadow_paint(mut self, layer: crate::instanced::ShadowLayer) -> Self {
        self.shadows.push(layer);
        self
    }
    /// Paint an animated ring over the background, before children. Requires uniform
    /// base corner radii. Supply a new phase and request frames from the host view;
    /// a constant phase produces a static/reduced-motion border.
    #[cfg(feature = "portable-shaders")]
    pub fn animated_border(mut self, layer: crate::instanced::AnimatedBorderLayer) -> Self {
        self.animated_border = Some(layer);
        self
    }
    pub fn bg_paint(mut self, paint: impl Into<Paint>) -> Self {
        self.background = Some(paint.into());
        self
    }
    pub fn border_paint(mut self, paint: impl Into<Paint>) -> Self {
        self.border = Some(paint.into());
        self
    }
}
impl Styled for Painted {
    fn style(&mut self) -> &mut StyleRefinement {
        self.root.style()
    }
}
impl ParentElement for Painted {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

struct Geometry {
    bounds: Cell<Bounds<Pixels>>,
    clip_style: Style,
}
impl Geometry {
    fn mask(&self, window: &Window) -> Option<ContentMask<Pixels>> {
        self.clip_style
            .overflow_mask(self.bounds.get(), window.rem_size())
    }
}
/// Retained element implementation for `Painted`; its fields are private.
pub struct PaintedElement {
    root: Div,
    geometry: Rc<Geometry>,
    #[cfg(feature = "portable-shaders")]
    shadows: Vec<crate::instanced::ShadowLayer>,
}
impl IntoElement for PaintedElement {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}
impl IntoElement for Painted {
    type Element = PaintedElement;
    fn into_element(mut self) -> PaintedElement {
        let mut style = Style::default();
        style.refine(self.root.style());
        #[cfg(feature = "portable-shaders")]
        if !self.shadows.is_empty() || self.animated_border.is_some() {
            let r = &style.corner_radii;
            assert!(
                r.top_left == r.top_right
                    && r.top_left == r.bottom_left
                    && r.top_left == r.bottom_right,
                "shader borders and shadows require uniform corner radii; use rounded(...) on the container"
            );
        }
        let custom_background = matches!(self.background, Some(Paint::Custom(_)));
        let overlay_border = self.border.is_some() || custom_background;
        let border = self.border.or_else(|| style.border_color.map(Paint::from));
        let mut clip_style = style.clone();
        if overlay_border {
            let visible = match &border {
                Some(Paint::Native(color)) => !color.is_transparent(),
                Some(Paint::Custom(_)) => true,
                None => false,
            };
            clip_style.border_color = Some(if visible {
                white()
            } else {
                transparent_black()
            });
            // Draw the border after children, outside the content inset mask. Native
            // border widths still participate in Taffy layout; no placeholder pixels.
            self.root.style().border_color = Some(transparent_black());
        }
        let geometry = Rc::new(Geometry {
            bounds: Cell::new(Bounds::default()),
            clip_style,
        });
        match self.background {
            Some(Paint::Native(color)) => self.root.style().background = Some(Fill::Color(color)),
            Some(Paint::Custom(content)) => {
                self.root.style().background = None;
                let inset: Vec<_> = style
                    .box_shadow
                    .iter()
                    .filter(|s| s.inset)
                    .cloned()
                    .collect();
                self.root.style().box_shadow = Some(
                    style
                        .box_shadow
                        .iter()
                        .filter(|s| !s.inset)
                        .cloned()
                        .collect(),
                );
                let geometry = geometry.clone();
                let radii = style.corner_radii;
                self.root = self.root.child(
                    canvas(
                        |_, _, _| (),
                        move |_, (), window, _| {
                            let bounds = geometry.bounds.get();
                            let radii = radii
                                .to_pixels(window.rem_size())
                                .clamp_radii_for_quad_size(bounds.size);
                            window.paint_custom(
                                CustomGeometry::RoundedRect {
                                    bounds,
                                    corner_radii: radii,
                                },
                                content,
                            );
                            window.paint_inset_shadows(bounds, radii, &inset);
                        },
                    )
                    .absolute()
                    .size_0(),
                );
            }
            None => {}
        }
        #[cfg(feature = "portable-shaders")]
        {
            let inset: Vec<_> = self
                .shadows
                .iter()
                .filter(|s| s.is_inset())
                .cloned()
                .collect();
            if !inset.is_empty() {
                let geometry = geometry.clone();
                let radii = style.corner_radii;
                self.root = self.root.child(
                    canvas(
                        |_, _, _| (),
                        move |_, (), window, _| {
                            let bounds = geometry.bounds.get();
                            let radius = radii
                                .to_pixels(window.rem_size())
                                .clamp_radii_for_quad_size(bounds.size)
                                .top_left;
                            // Root Div has applied its own opacity; inherited opacity comes from GPUI.
                            for layer in &inset {
                                layer
                                    .paint(window, bounds, radius, 1.)
                                    .expect("validated inset shadow layer");
                            }
                        },
                    )
                    .absolute()
                    .size_0(),
                );
            }
        }
        #[cfg(feature = "portable-shaders")]
        if let Some(border) = self.animated_border {
            let geometry = geometry.clone();
            let radii = style.corner_radii;
            self.root = self.root.child(
                canvas(
                    |_, _, _| (),
                    move |_, (), window, _| {
                        let bounds = geometry.bounds.get();
                        let radius = radii
                            .to_pixels(window.rem_size())
                            .clamp_radii_for_quad_size(bounds.size)
                            .top_left;
                        border
                            .paint(window, bounds, radius)
                            .expect("validated animated border");
                    },
                )
                .absolute()
                .size_0(),
            );
        }
        for child in self.children {
            self.root = self.root.child(ClippedChild {
                child,
                geometry: geometry.clone(),
            });
        }
        if overlay_border && let Some(border) = border {
            let geometry = geometry.clone();
            let radii = style.corner_radii;
            let widths = style.border_widths;
            let border_style = style.border_style;
            self.root = self.root.child(
                canvas(
                    |_, _, _| (),
                    move |_, (), window, _| {
                        let bounds = geometry.bounds.get();
                        window.paint_quad_with_paints(
                            quad(
                                bounds,
                                radii.to_pixels(window.rem_size()),
                                transparent_black(),
                                widths.to_pixels(window.rem_size()),
                                transparent_black(),
                                border_style,
                            ),
                            transparent_black(),
                            border,
                        );
                    },
                )
                .absolute()
                .size_0(),
            );
        }
        PaintedElement {
            root: self.root,
            geometry,
            #[cfg(feature = "portable-shaders")]
            shadows: self.shadows,
        }
    }
}
impl Element for PaintedElement {
    type RequestLayoutState = <Div as Element>::RequestLayoutState;
    type PrepaintState = <Div as Element>::PrepaintState;
    fn id(&self) -> Option<ElementId> {
        Element::id(&self.root)
    }
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        self.root.source_location()
    }
    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        Element::request_layout(&mut self.root, id, inspector, window, cx)
    }
    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        state: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        self.geometry.bounds.set(bounds);
        Element::prepaint(&mut self.root, id, inspector, bounds, state, window, cx)
    }
    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        #[cfg(feature = "portable-shaders")]
        {
            let style = &self.geometry.clip_style;
            if style.visibility != Visibility::Hidden
                && style.display != Display::None
                && !bounds.is_empty()
            {
                let radius = style
                    .corner_radii
                    .to_pixels(window.rem_size())
                    .clamp_radii_for_quad_size(bounds.size)
                    .top_left;
                // Before Div paints, only ancestor opacity/masks are active. Fold this
                // container's base opacity into the shadow once, without changing GPUI.
                let opacity = style.opacity.unwrap_or(1.).clamp(0., 1.);
                for layer in self.shadows.iter().filter(|s| !s.is_inset()) {
                    layer
                        .paint(window, bounds, radius, opacity)
                        .expect("validated outset shadow layer");
                }
            }
        }
        Element::paint(
            &mut self.root,
            id,
            inspector,
            bounds,
            layout,
            prepaint,
            window,
            cx,
        );
    }
}
// Delegate layout directly, so wrapping does not insert an extra Taffy node or
// change the parent's flex/grid child sizing. Apply the same mask in both phases.
struct ClippedChild {
    child: AnyElement,
    geometry: Rc<Geometry>,
}
impl IntoElement for ClippedChild {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}
impl Element for ClippedChild {
    type RequestLayoutState = ();
    type PrepaintState = ();
    fn id(&self) -> Option<ElementId> {
        None
    }
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }
    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        (self.child.request_layout(window, cx), ())
    }
    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        window.with_content_mask(self.geometry.mask(window), |window| {
            self.child.prepaint(window, cx)
        });
    }
    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        window.with_content_mask(self.geometry.mask(window), |window| {
            self.child.paint(window, cx)
        });
    }
}

#[cfg(test)]
#[path = "surface_shadow_tests.rs"]
mod tests;
