use gpui::{
    Bounds, ContentMask, CustomDraw, DevicePixels, PlatformHeadlessRenderer, ScaledPixels, Scene,
    content::*, point, px, size,
};
use gpui_windows::{Dx11HeadlessRenderer, content_executor::*, custom_draw::dxgi::DxgiFrame};
use std::{
    os::windows::io::{AsHandle, FromRawHandle, OwnedHandle},
    sync::Arc,
    time::{Duration, Instant},
};
use windows::{
    Win32::{
        Foundation::{HMODULE, WAIT_TIMEOUT},
        Graphics::{
            Direct3D::*,
            Direct3D11::*,
            Dxgi::{
                Common::*, DXGI_SHARED_RESOURCE_READ, DXGI_SHARED_RESOURCE_WRITE, IDXGIKeyedMutex,
                IDXGIResource1,
            },
        },
    },
    core::{Interface, PCWSTR},
};
fn device() -> (ID3D11Device, ID3D11DeviceContext) {
    let (mut device, mut context) = (None, None);
    unsafe {
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_WARP,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            Some(&[D3D_FEATURE_LEVEL_11_0]),
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            Some(&mut context),
        )
        .unwrap();
    }
    (device.unwrap(), context.unwrap())
}
fn readonly_texture(device: &ID3D11Device) -> ID3D11Texture2D {
    let bytes = [32_u8, 64, 224, 255].repeat(16 * 8);
    let desc = D3D11_TEXTURE2D_DESC {
        Width: 16,
        Height: 8,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT_B8G8R8A8_UNORM,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
        ..Default::default()
    };
    let data = D3D11_SUBRESOURCE_DATA {
        pSysMem: bytes.as_ptr().cast(),
        SysMemPitch: 16 * 4,
        SysMemSlicePitch: 0,
    };
    let mut texture = None;
    unsafe {
        device
            .CreateTexture2D(&desc, Some(&data), Some(&mut texture))
            .unwrap();
    }
    texture.unwrap()
}
fn graph(executor: &Dx11ContentExecutor, content: Content) -> Arc<ContentGraph> {
    Arc::new(
        ContentGraph::describe(
            &[ContentRoot {
                content,
                request: ContentRequest::new(
                    size(px(64.), px(32.)),
                    ImageExtent::new(64, 32).unwrap(),
                    1.,
                )
                .unwrap(),
            }],
            executor.capabilities().clone(),
            GraphLimits::default(),
        )
        .unwrap(),
    )
}
fn raw_acquire(mutex: &IDXGIKeyedMutex, key: u64) -> i32 {
    unsafe { (Interface::vtable(mutex).AcquireSync)(mutex.as_raw(), key, 0) }.0
}
fn released(mutex: &IDXGIKeyedMutex) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match raw_acquire(mutex, 0) {
            0 => {
                unsafe {
                    mutex.ReleaseSync(0).unwrap();
                }
                break;
            }
            code if code == WAIT_TIMEOUT.0 as i32 => {
                assert!(Instant::now() < deadline, "producer key did not retire");
                std::thread::sleep(Duration::from_millis(1));
            }
            code => panic!("producer acquisition failed: {code}"),
        }
    }
}
fn shared(consumer: &ID3D11Device) -> (DxgiFrame, IDXGIKeyedMutex) {
    let (producer, context) = device();
    let desc = D3D11_TEXTURE2D_DESC {
        Width: 16,
        Height: 8,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT_B8G8R8A8_UNORM,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: (D3D11_BIND_SHADER_RESOURCE.0 | D3D11_BIND_RENDER_TARGET.0) as u32,
        MiscFlags: (D3D11_RESOURCE_MISC_SHARED_NTHANDLE.0 | D3D11_RESOURCE_MISC_SHARED_KEYEDMUTEX.0)
            as u32,
        ..Default::default()
    };
    let mut texture = None;
    unsafe {
        producer
            .CreateTexture2D(&desc, None, Some(&mut texture))
            .unwrap();
    }
    let texture = texture.unwrap();
    let mutex: IDXGIKeyedMutex = texture.cast().unwrap();
    assert_eq!(raw_acquire(&mutex, 0), 0);
    let mut rtv = None;
    unsafe {
        producer
            .CreateRenderTargetView(&texture, None, Some(&mut rtv))
            .unwrap();
        context.ClearRenderTargetView(rtv.as_ref().unwrap(), &[1., 0., 0., 1.]);
        context.Flush();
        mutex.ReleaseSync(1).unwrap();
    }
    let resource: IDXGIResource1 = texture.cast().unwrap();
    let handle = unsafe {
        resource
            .CreateSharedHandle(
                None,
                DXGI_SHARED_RESOURCE_READ.0 | DXGI_SHARED_RESOURCE_WRITE.0,
                PCWSTR::null(),
            )
            .unwrap()
    };
    let handle = unsafe { OwnedHandle::from_raw_handle(handle.0) };
    // Wrong-key acquisition must return busy immediately, never a positive success HRESULT.
    assert!(
        unsafe { DxgiFrame::try_from_shared_bgra(consumer, handle.as_handle(), 77, 0) }
            .unwrap()
            .is_none()
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    let frame = loop {
        if let Some(frame) =
            unsafe { DxgiFrame::try_from_shared_bgra(consumer, handle.as_handle(), 1, 0) }.unwrap()
        {
            break frame;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    };
    assert!(
        unsafe { DxgiFrame::try_from_shared_bgra(consumer, handle.as_handle(), 1, 0) }
            .unwrap()
            .is_none()
    );
    drop(handle);
    (frame, mutex)
}
#[derive(Debug)]
struct CopySource(Content);
impl ContentSource for CopySource {
    fn describe(
        &self,
        cx: &mut ContentBuilder,
        request: &ContentRequest,
    ) -> Result<ImageRef, ContentError> {
        let image = cx.source(&self.0, request)?;
        cx.copy_image(image)
    }
}
#[test]
fn dxgi_readonly_import_has_native_pixels_without_an_output_allocation() {
    let (device, context) = device();
    let texture = readonly_texture(&device);
    let frame = unsafe { DxgiFrame::from_bgra(texture) }.unwrap();
    let executor =
        Dx11ContentExecutor::on_device(device, context, Dx11ContentOptions::default()).unwrap();
    let image = frame.external_image().unwrap();
    let job = executor
        .submit(
            graph(&executor, Content::external(image)),
            Dx11ContentLimits::default(),
        )
        .unwrap();
    drop(frame);
    let result = job.wait_for_readback(Duration::from_secs(10)).unwrap();
    assert_eq!(result.stats.external_imports, 1);
    assert_eq!(result.stats.image_allocations, 0);
    assert_eq!(
        result.images[0].descriptor.extent(),
        ImageExtent::new(16, 8).unwrap()
    );
    assert!(
        result.images[0].levels[0]
            .chunks_exact(4)
            .all(|p| p == [32, 64, 224, 255])
    );
}
#[test]
fn dxgi_foreign_device_and_descriptor_mismatch_are_rejected() {
    let (device, context) = device();
    let (foreign, _) = self::device();
    let frame = unsafe { DxgiFrame::from_bgra(readonly_texture(&foreign)) }.unwrap();
    let executor =
        Dx11ContentExecutor::on_device(device.clone(), context, Dx11ContentOptions::default())
            .unwrap();
    let error = executor
        .submit(
            graph(
                &executor,
                Content::external(frame.external_image().unwrap()),
            ),
            Dx11ContentLimits::default(),
        )
        .unwrap()
        .wait_for_readback(Duration::from_secs(10))
        .unwrap_err();
    assert_eq!(error.kind, ContentErrorKind::UnsupportedCapability);
    let frame = unsafe { DxgiFrame::from_bgra(readonly_texture(&device)) }.unwrap();
    let mut descriptor = frame.external_image().unwrap().descriptor().clone();
    descriptor = ImageDescriptor::new(
        ImageExtent::new(8, 8).unwrap(),
        descriptor.format(),
        descriptor.interpretation(),
        1,
    )
    .unwrap();
    let image = ExternalImage::ready(descriptor, Arc::new(frame), || {});
    assert_eq!(
        executor
            .submit(
                graph(&executor, Content::external(image)),
                Dx11ContentLimits::default()
            )
            .unwrap()
            .wait_for_readback(Duration::from_secs(10))
            .unwrap_err()
            .kind,
        ContentErrorKind::InvalidDescriptor
    );
    let (_, wrong_context) = self::device();
    assert!(
        Dx11ContentExecutor::on_device(device, wrong_context, Dx11ContentOptions::default())
            .is_err()
    );
}
fn scene(frame: DxgiFrame) -> Scene {
    let mut scene = Scene::default();
    let bounds = Bounds::new(
        point(ScaledPixels(4.), ScaledPixels(4.)),
        size(ScaledPixels(32.), ScaledPixels(16.)),
    );
    let content = ContentDraw::new(Content::external(frame.external_image().unwrap()))
        .color_output(ContentColorOutput::SrgbPreview)
        .color_transform(
            ImageColorTransform::encoded_srgb([
                [0., 0., 1., 0.],
                [0., 1., 0., 0.],
                [1., 0., 0., 0.],
            ])
            .unwrap(),
        )
        .into_scene_content();
    scene.insert_primitive(CustomDraw {
        order: 0,
        bounds,
        content_mask: ContentMask { bounds },
        corner_radii: Default::default(),
        opacity: 0.5,
        content,
        coverage: None,
    });
    scene.finish();
    scene
}
fn ready(renderer: &mut Dx11HeadlessRenderer, scene: &Scene) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match renderer.prepare_content_snapshot(scene) {
            Ok(()) => break,
            Err(e)
                if e.downcast_ref::<ContentError>()
                    .is_some_and(|e| e.kind == ContentErrorKind::Pending) => {}
            Err(e) => panic!("{e}"),
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
}
#[test]
fn dxgi_presentation_reuses_readonly_import_and_applies_adjustments() {
    let mut renderer = Dx11HeadlessRenderer::new().unwrap();
    let device = renderer.native_device().unwrap();
    let frame = unsafe { DxgiFrame::from_bgra(readonly_texture(&device)) }.unwrap();
    let scene = scene(frame);
    ready(&mut renderer, &scene);
    let image = renderer
        .render_scene_to_image(&scene, size(DevicePixels(40), DevicePixels(24)))
        .unwrap();
    let p = image.get_pixel(6, 6).0;
    assert!(
        p[0].abs_diff(16) <= 1 && p[1].abs_diff(32) <= 1 && p[2].abs_diff(112) <= 1,
        "{p:?}"
    );
    let stats = renderer.content_presentation_stats();
    assert_eq!(stats.execution.external_imports, 1);
    assert_eq!(stats.execution.image_allocations, 0);
    assert_eq!(stats.execution.readbacks, 0);
    ready(&mut renderer, &scene);
    let replay = renderer
        .render_scene_to_image(&scene, size(DevicePixels(40), DevicePixels(24)))
        .unwrap();
    assert_eq!(image, replay);
    assert_eq!(
        renderer.content_presentation_stats().requests,
        stats.requests
    );
}
#[test]
fn dxgi_keyed_lease_survives_paused_work_and_cancellation() {
    let (device, context) = device();
    let (frame, mutex) = shared(&device);
    let executor =
        Dx11ContentExecutor::on_device(device, context, Dx11ContentOptions::default()).unwrap();
    let content = Content::new(CopySource(Content::external(
        frame.external_image().unwrap(),
    )));
    drop(frame);
    struct Release(Arc<Dx11ContentTestGate>);
    impl Drop for Release {
        fn drop(&mut self) {
            self.0.release();
        }
    }
    let gate = Release(Arc::new(Dx11ContentTestGate::default()));
    let job = executor
        .submit(
            graph(&executor, content),
            Dx11ContentLimits {
                pause_before_node: Some((1, gate.0.clone())),
                ..Default::default()
            },
        )
        .unwrap();
    assert!(gate.0.wait_until_entered(Duration::from_secs(5)));
    assert_eq!(raw_acquire(&mutex, 0), WAIT_TIMEOUT.0 as i32);
    job.cancel();
    gate.0.release();
    assert_eq!(
        job.wait_for_readback(Duration::from_secs(10))
            .unwrap_err()
            .kind,
        ContentErrorKind::Cancelled
    );
    released(&mutex);
}
#[test]
fn dxgi_keyed_prefix_failure_releases_only_after_native_retirement() {
    let (device, context) = device();
    let (frame, mutex) = shared(&device);
    let executor =
        Dx11ContentExecutor::on_device(device, context, Dx11ContentOptions::default()).unwrap();
    let source = Content::new(CopySource(Content::external(
        frame.external_image().unwrap(),
    )));
    drop(frame);
    let job = executor
        .submit(
            graph(&executor, Content::new(CopySource(source))),
            Dx11ContentLimits {
                fail_before_node: Some(2),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(
        job.wait_for_readback(Duration::from_secs(10))
            .unwrap_err()
            .kind,
        ContentErrorKind::ExecutionFailure
    );
    released(&mutex);
}
#[test]
fn dxgi_keyed_sampling_survives_renderer_teardown() {
    let mut renderer = Dx11HeadlessRenderer::new().unwrap();
    let (frame, mutex) = shared(&renderer.native_device().unwrap());
    let scene = scene(frame);
    ready(&mut renderer, &scene);
    renderer
        .render_scene(&scene, size(DevicePixels(40), DevicePixels(24)))
        .unwrap();
    assert_eq!(
        raw_acquire(&mutex, 0),
        WAIT_TIMEOUT.0 as i32,
        "current snapshot must retain producer ownership"
    );
    drop(scene);
    drop(renderer);
    released(&mutex);
}
