# xgpui-media — experimental

Opt-in bounded native image loading and dedicated mipmapped SDR textures.
Native Metal and D3D11; optional wgpu. Independent of `xgpui-shader`.

The maintained [media image guide](../../../../docs/extensions/media-images.md)
covers usage, payload budgets, cancellation, color/resource contracts, backend
coverage, validation and remaining asynchronous upload work.

## GPUI public API changes: additions and compatibility

[GPUI API additions](../../../../docs/extensions/gpui-public-api.md#added-source-sized-media-textures).
See the [media guide](../../../../docs/extensions/media-images.md#gpui-public-api-changes-additions-and-compatibility).

`image_view(image).filter(ImageFilter::new().brightness(1.1).saturation(1.3))`
applies encoded-sRGB color adjustments in the sampling pass, preserving alpha
and sharing the retained upload. See the web guide for composition semantics.

`video(VideoStream)` provides bounded latest-frame presentation; `video_frame`
paints immutable snapshots. Native macOS `CoreVideoFrame` imports BGRA IOSurfaces
and opaque two-plane NV12 (`from_nv12`, explicit `Nv12Matrix::Bt601` / `Bt709`;
full/video range from the CoreVideo pixel format)
without a texture upload. The channel supplies no decoder or playback clock. Windows `DxgiFrame` provides dedicated immutable same-device BGRA surface import;
keyed NT-handle imports add nonblocking read leases and producer-key handoff.
Linux `DmaBufFrame` (`wgpu` feature) imports dedicated immutable linear BGRA
Vulkan snapshots with producer synchronization already complete. Windows/Linux
runtime verification, DMA-BUF explicit fences and pool reuse remain pending. See
the web guide for the unsafe native ownership contract and video lab commands.


System file playback is separate follow-up work: [#40](https://github.com/gpui-luma-dev/xgpui/issues/40).
