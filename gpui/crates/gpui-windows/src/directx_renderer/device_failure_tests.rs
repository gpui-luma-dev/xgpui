//! A renderer whose device recovery failed stops drawing for good (#119).
use super::*;

#[::core::prelude::v1::test]
fn failed_renderer_never_draws_again() {
    let mut renderer = Dx11HeadlessRenderer::new().expect("WARP renderer");
    renderer.0.mark_failed();
    // The forced render after a recovery must not revive a failed renderer.
    renderer.0.mark_drawable();
    assert!(renderer.0.skip_draws);
    // A headless renderer has no swap chain, so drawing would fail if it ran.
    let mut scene = Scene::default();
    scene.finish();
    renderer
        .0
        .draw(&scene, WindowBackgroundAppearance::Opaque)
        .expect("a failed renderer skips the frame");
}
