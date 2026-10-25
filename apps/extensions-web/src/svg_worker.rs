//! The module worker gets its own Wasm instance and immutable parsed document.
//! No GPUI platform/window/GPU objects are constructed in that instance.
use gpui::{Bounds, DevicePixels, SvgRenderer, SvgSize, point, size};
use std::{cell::RefCell, sync::Arc};
use wasm_bindgen::prelude::*;
use xgpui_svg::{MIB, SvgDocument};

thread_local! {
    static DOCUMENT: RefCell<Option<(Vec<u8>, SvgDocument)>> = const { RefCell::new(None) };
}

#[wasm_bindgen(js_namespace = xgpuiSvgWorker)]
extern "C" {
    #[wasm_bindgen(catch, js_name = request)]
    pub fn request(
        svg: &[u8],
        width: u32,
        height: u32,
        x: f32,
        y: f32,
        view_width: f32,
        view_height: f32,
    ) -> Result<js_sys::Promise, JsValue>;
    #[wasm_bindgen(js_name = invalidate)]
    pub fn invalidate();
    #[wasm_bindgen(js_name = dispose)]
    pub fn dispose();
}

/// Called exclusively by svg-worker.mjs. Returns the GPUI-ready BGRA bytes.
#[wasm_bindgen]
pub fn rasterize_svg_region(
    svg: &[u8],
    width: u32,
    height: u32,
    x: f32,
    y: f32,
    view_width: f32,
    view_height: f32,
) -> Result<Vec<u8>, JsValue> {
    if web_sys::window().is_some() {
        return Err(JsValue::from_str(
            "SVG worker rasterization must run in a Web Worker",
        ));
    }
    if width == 0
        || height == 0
        || width > 8192
        || height > 8192
        || u64::from(width) * u64::from(height) * 4 > 32 * MIB as u64
    {
        return Err(JsValue::from_str(
            "Worker output exceeds 32 MiB / 8192 edge limit",
        ));
    }
    DOCUMENT.with(|slot| {
        let renderer = SvgRenderer::new(Arc::new(()));
        let mut slot = slot.borrow_mut();
        if slot.as_ref().is_none_or(|(bytes, _)| bytes != svg) {
            let document = SvgDocument::parse(&renderer, svg)
                .map_err(|e| JsValue::from_str(&e.to_string()))?;
            *slot = Some((svg.to_vec(), document));
        }
        let document = &slot.as_ref().unwrap().1;
        let raster = renderer
            .render_parsed(
                document,
                SvgSize::Viewport {
                    size: size(DevicePixels(width as i32), DevicePixels(height as i32)),
                    view_box: Bounds::new(point(x, y), size(view_width, view_height)),
                },
            )
            .map_err(|e| JsValue::from_str(&e.to_string()))?;
        Ok(raster.as_bytes(0).unwrap().to_vec())
    })
}
