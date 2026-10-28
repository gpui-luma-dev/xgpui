//! Explicit extension containers. Custom materials do not enter native Style/Fill.
use crate::{Paint, WindowPaintExt};
use gpui::*;
use refineable::Refineable;
use std::{cell::Cell, rc::Rc, sync::Arc};

enum SurfacePaint {
    Paint(Paint),
    #[cfg(feature = "portable-shaders")]
    Brush(crate::Brush),
}
impl SurfacePaint {
    fn visible(&self) -> bool {
        match self {
            Self::Paint(Paint::Native(color)) => !color.is_transparent(),
            _ => true,
        }
    }
}

/// A normal layout container with extension-owned background/border materials.
/// Explicit materials override native background/border colors on this container.
/// Material changes are supplied by rebuilding it; native hover refinements do not
/// carry custom materials. Descendant interaction and parent clip/opacity are native.
pub struct Painted {
    root: Div,
    children: Vec<AnyElement>,
    background: Option<SurfacePaint>,
    border: Option<SurfacePaint>,
    #[cfg(feature = "portable-shaders")]
    shadows: Vec<crate::instanced::ShadowLayer>,
    #[cfg(feature = "portable-shaders")]
    animated_border: Option<crate::instanced::AnimatedBorderLayer>,
    #[cfg(feature = "portable-shaders")]
    text_brush: Option<Arc<dyn GlyphBrush>>,
    #[cfg(feature = "portable-shaders")]
    backdrop: Option<crate::BackdropBlur>,
    #[cfg(feature = "portable-shaders")]
    layer_effect: Option<crate::LayerEffect>,
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
        #[cfg(feature = "portable-shaders")]
        text_brush: None,
        #[cfg(feature = "portable-shaders")]
        backdrop: None,
        #[cfg(feature = "portable-shaders")]
        layer_effect: None,
    }
}
/// Compatibility name for the original shader element.
pub type Shader = Painted;
/// Place a reusable producer in an extension-owned styled container.
pub fn shader(content: Arc<dyn CustomContent>) -> Shader {
    painted().bg_paint(content)
}
impl Painted {
    /// Isolate this bounded surface and its children, then apply a group effect.
    /// Overflow is clipped; deferred overlays paint outside the capture.
    #[cfg(feature = "portable-shaders")]
    pub fn layer_effect(mut self, effect: crate::LayerEffect) -> Self {
        self.layer_effect = Some(effect);
        self
    }

    /// Explicit bounded backdrop pass, before this surface and its children.
    #[cfg(feature = "portable-shaders")]
    pub fn backdrop_blur(mut self, effect: crate::BackdropBlur) -> Self {
        self.backdrop = Some(effect);
        self
    }
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
    /// Color descendant monochrome glyphs with a continuous field over this
    /// container's full bounds. Emoji retain their native color; nested text
    /// brush containers override this scope. Uses grayscale atlas coverage.
    #[cfg(feature = "portable-shaders")]
    pub fn text_brush(mut self, brush: crate::Brush) -> Self {
        self.text_brush = Some(brush.text_brush().expect("validated text brush"));
        self
    }
    /// Main-pass brush over the full background. Overrides earlier bg_paint/bg_brush.
    #[cfg(feature = "portable-shaders")]
    pub fn bg_brush(mut self, brush: crate::Brush) -> Self {
        self.background = Some(SurfacePaint::Brush(brush));
        self
    }
    /// Main-pass solid-style border brush. Uses layout border widths; dashed
    /// borders require border_paint. Overrides earlier border_paint/border_brush.
    #[cfg(feature = "portable-shaders")]
    pub fn border_brush(mut self, brush: crate::Brush) -> Self {
        self.border = Some(SurfacePaint::Brush(brush));
        self
    }
    pub fn bg_paint(mut self, paint: impl Into<Paint>) -> Self {
        self.background = Some(SurfacePaint::Paint(paint.into()));
        self
    }
    pub fn border_paint(mut self, paint: impl Into<Paint>) -> Self {
        self.border = Some(SurfacePaint::Paint(paint.into()));
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
    #[cfg(feature = "portable-shaders")]
    text_brush: Option<Arc<dyn GlyphBrush>>,
    #[cfg(feature = "portable-shaders")]
    backdrop: Option<crate::BackdropBlur>,
    #[cfg(feature = "portable-shaders")]
    layer_effect: Option<crate::LayerEffect>,
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
        let custom_background = self
            .background
            .as_ref()
            .is_some_and(|paint| !matches!(paint, SurfacePaint::Paint(Paint::Native(_))));
        let overlay_border = self.border.is_some() || custom_background;
        let border = self.border.or_else(|| {
            style
                .border_color
                .map(|color| SurfacePaint::Paint(Paint::from(color)))
        });
        #[cfg(feature = "portable-shaders")]
        if matches!(border, Some(SurfacePaint::Brush(_))) {
            assert!(
                style.border_style == BorderStyle::Solid,
                "main-pass brushes require solid borders; use border_paint for dashed borders"
            );
        }
        let mut clip_style = style.clone();
        if overlay_border {
            let visible = border.as_ref().is_some_and(SurfacePaint::visible);
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
            Some(SurfacePaint::Paint(Paint::Native(color))) => {
                self.root.style().background = Some(Fill::Color(color))
            }
            Some(background) => {
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
                            match background {
                                SurfacePaint::Paint(Paint::Custom(content)) => window.paint_custom(
                                    CustomGeometry::RoundedRect {
                                        bounds,
                                        corner_radii: radii,
                                    },
                                    content,
                                ),
                                #[cfg(feature = "portable-shaders")]
                                SurfacePaint::Brush(brush) => brush
                                    .paint_background(window, bounds, radii)
                                    .expect("validated background brush"),
                                SurfacePaint::Paint(Paint::Native(_)) => unreachable!(),
                            }
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
                        let quad = quad(
                            bounds,
                            radii.to_pixels(window.rem_size()),
                            transparent_black(),
                            widths.to_pixels(window.rem_size()),
                            transparent_black(),
                            border_style,
                        );
                        match border {
                            SurfacePaint::Paint(paint) => {
                                window.paint_quad_with_paints(quad, transparent_black(), paint)
                            }
                            #[cfg(feature = "portable-shaders")]
                            SurfacePaint::Brush(brush) => brush
                                .paint_border(window, &quad)
                                .expect("validated border brush"),
                        }
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
            #[cfg(feature = "portable-shaders")]
            text_brush: self.text_brush,
            #[cfg(feature = "portable-shaders")]
            backdrop: self.backdrop,
            #[cfg(feature = "portable-shaders")]
            layer_effect: self.layer_effect,
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
        #[cfg(feature = "portable-shaders")]
        let captured = self.layer_effect.is_some();
        let mut prepaint = |window: &mut Window| {
            self.geometry.bounds.set(bounds);
            #[cfg(feature = "portable-shaders")]
            if let Some(brush) = self.text_brush.clone() {
                return window.with_glyph_brush(brush, bounds, |window| {
                    Element::prepaint(&mut self.root, id, inspector, bounds, state, window, cx)
                });
            }
            Element::prepaint(&mut self.root, id, inspector, bounds, state, window, cx)
        };
        #[cfg(feature = "portable-shaders")]
        if captured {
            return window.with_layer_effect_scope(bounds, prepaint);
        }
        prepaint(window)
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
        let effect = self.layer_effect.clone().filter(|_| {
            self.geometry.clip_style.visibility != Visibility::Hidden
                && self.geometry.clip_style.display != Display::None
                && !bounds.is_empty()
        });
        let mut paint = |window: &mut Window| {
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
                    if let Some(effect) = &self.backdrop {
                        effect.paint(
                            window,
                            bounds,
                            style
                                .corner_radii
                                .to_pixels(window.rem_size())
                                .clamp_radii_for_quad_size(bounds.size),
                            opacity,
                        );
                    }
                    for layer in self.shadows.iter().filter(|s| !s.is_inset()) {
                        layer
                            .paint(window, bounds, radius, opacity)
                            .expect("validated outset shadow layer");
                    }
                }
            }
            #[cfg(feature = "portable-shaders")]
            if let Some(brush) = self.text_brush.clone() {
                window.with_glyph_brush(brush, bounds, |window| {
                    Element::paint(
                        &mut self.root,
                        id,
                        inspector,
                        bounds,
                        layout,
                        prepaint,
                        window,
                        cx,
                    )
                });
                return;
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
        };
        #[cfg(feature = "portable-shaders")]
        if let Some(effect) = effect {
            return effect.paint(window, bounds, paint);
        }
        paint(window)
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

#[cfg(all(test, feature = "portable-shaders"))]
mod brush_setter_tests {
    use super::*;
    #[::core::prelude::v1::test]
    fn explicit_material_setters_use_the_last_value() {
        let brush = crate::Brush::solid([1.; 4]).unwrap();
        let surface = painted()
            .bg_brush(brush.clone())
            .bg_paint(white())
            .border_brush(brush.clone())
            .border_paint(white());
        assert!(matches!(
            surface.background,
            Some(SurfacePaint::Paint(Paint::Native(_)))
        ));
        assert!(matches!(
            surface.border,
            Some(SurfacePaint::Paint(Paint::Native(_)))
        ));
        let surface = surface.bg_brush(brush.clone()).border_brush(brush);
        assert!(matches!(surface.background, Some(SurfacePaint::Brush(_))));
        assert!(matches!(surface.border, Some(SurfacePaint::Brush(_))));
    }
}
