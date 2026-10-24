use super::*;
use gpui::*;
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
    sync::Arc,
};

// Thread-local accounting avoids allocations from parallel GPU/test workers.
struct CountingAllocator;
thread_local! { static ALLOCATIONS: Cell<Option<usize>> = const { Cell::new(None) }; }
fn count_allocation() {
    let _ = ALLOCATIONS.try_with(|count| {
        if let Some(n) = count.get() {
            count.set(Some(n + 1));
        }
    });
}
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count_allocation();
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        count_allocation();
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        count_allocation();
        unsafe { System.realloc(ptr, layout, size) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}
#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;
fn allocations<T>(f: impl FnOnce() -> T) -> (T, usize) {
    assert!(ALLOCATIONS.with(Cell::get).is_none());
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            ALLOCATIONS.with(|c| c.set(None));
        }
    }
    ALLOCATIONS.with(|c| c.set(Some(0)));
    let reset = Reset;
    let result = f();
    let count = ALLOCATIONS.with(Cell::get).unwrap();
    drop(reset);
    (result, count)
}
#[derive(Debug)]
struct UnusedProducer;
impl CustomContent for UnusedProducer {
    fn prepare(
        &self,
        _: &dyn std::any::Any,
        _: Size<DevicePixels>,
    ) -> anyhow::Result<Box<dyn std::any::Any>> {
        unreachable!("CPU key tests do not prepare paints")
    }
}
fn vertex(x: f32, y: f32) -> PathVertex<ScaledPixels> {
    PathVertex {
        xy_position: point(ScaledPixels(x), ScaledPixels(y)),
        st_position: point(0., 1.),
        content_mask: ContentMask::default(),
    }
}
fn path_draw(count: usize) -> CustomDraw {
    let mut path = Path::new(point(px(0.), px(0.))).scale(1.);
    path.vertices = (0..count)
        .map(|i| vertex((i % 100) as f32, (i / 100) as f32))
        .collect();
    CustomDraw {
        order: 0,
        bounds: Bounds::new(
            point(ScaledPixels(0.), ScaledPixels(0.)),
            size(ScaledPixels(128.), ScaledPixels(128.)),
        ),
        content_mask: ContentMask::default(),
        corner_radii: Corners::all(ScaledPixels(4.)),
        opacity: 1.,
        content: Arc::new(UnusedProducer),
        coverage: Some(CustomCoverage::Path(Arc::new(path))),
    }
}
fn path_mut(draw: &mut CustomDraw) -> &mut Path<ScaledPixels> {
    let Some(CustomCoverage::Path(path)) = &mut draw.coverage else {
        panic!("path expected")
    };
    Arc::make_mut(path)
}
struct CollidingHasher;
impl Hasher for CollidingHasher {
    fn write(&mut self, _: &[u8]) {}
    fn finish(&self) -> u64 {
        42
    }
}
struct ForbiddenHasher;
impl Hasher for ForbiddenHasher {
    fn write(&mut self, _: &[u8]) {
        panic!("oversized path was hashed")
    }
    fn finish(&self) -> u64 {
        panic!("oversized path was hashed")
    }
}

#[::core::prelude::v1::test]
fn coverage_cache_forced_collisions_never_alias_geometry() {
    let a = path_draw(3);
    let mut b = a.clone();
    path_mut(&mut b).vertices[1].st_position.x = 0.5;
    let ka = CoverageKey::with_hasher(&a, CollidingHasher).unwrap();
    let kb = CoverageKey::with_hasher(&b, CollidingHasher).unwrap();
    let mut cache = CoverageCache::default();
    cache.insert(&ka, "a");
    assert!(cache.get(&kb).is_none());
    cache.insert(&kb, "b");
    assert_eq!(cache.get(&ka), Some(&"a"));
    assert_eq!(cache.get(&kb), Some(&"b"));
    assert_eq!(cache.buckets.len(), 1);
    assert_eq!(cache.buckets[&42].len(), 2);
    cache.insert(&ka, "updated");
    assert_eq!(cache.buckets[&42].len(), 2);
    assert_eq!(cache.get(&ka), Some(&"updated"));
}

#[::core::prelude::v1::test]
fn coverage_cache_translation_reuses_exact_normalized_geometry() {
    let original = path_draw(12);
    let mut moved = original.clone();
    moved.bounds.origin = point(ScaledPixels(40.), ScaledPixels(-20.));
    for v in &mut path_mut(&mut moved).vertices {
        v.xy_position.x.0 += 40.;
        v.xy_position.y.0 -= 20.;
    }
    moved.opacity = 0.2;
    moved.order = 999;
    moved.content = Arc::new(UnusedProducer);
    moved.content_mask.bounds = Bounds::new(
        point(ScaledPixels(50.), ScaledPixels(10.)),
        size(ScaledPixels(10.), ScaledPixels(20.)),
    );
    path_mut(&mut moved).color = red().into();
    path_mut(&mut moved).content_mask = moved.content_mask;
    let key = CoverageKey::new(&original).unwrap();
    let moved_key = CoverageKey::new(&moved).unwrap();
    assert_eq!(key.fingerprint, moved_key.fingerprint);
    let mut cache = CoverageCache::default();
    cache.insert(&key, 7);
    assert_eq!(cache.get(&moved_key), Some(&7));
}

#[::core::prelude::v1::test]
fn coverage_cache_vertex_st_count_bounds_and_radii_invalidate() {
    let draw = path_draw(3);
    let mut changes = vec![draw.clone(); 6];
    path_mut(&mut changes[0]).vertices[0].xy_position.x.0 += 0.25;
    path_mut(&mut changes[1]).vertices[0].st_position.y += 0.25;
    path_mut(&mut changes[2]).vertices.push(vertex(0., 0.));
    changes[3].bounds.size.width.0 += 1.;
    changes[4].bounds.size.height.0 += 1.;
    changes[5].corner_radii.bottom_left.0 += 1.;
    let mut cache = CoverageCache::default();
    // Force every change into the same bucket: exact comparison must reject it.
    cache.insert(
        &CoverageKey::with_hasher(&draw, CollidingHasher).unwrap(),
        7,
    );
    for changed in changes {
        assert!(
            cache
                .get(&CoverageKey::with_hasher(&changed, CollidingHasher).unwrap())
                .is_none()
        );
    }
}

#[::core::prelude::v1::test]
fn coverage_cache_border_style_widths_and_kind_invalidate() {
    let mut draw = path_draw(3);
    draw.coverage = Some(CustomCoverage::Border {
        widths: Edges::all(ScaledPixels(2.)),
        style: BorderStyle::Solid,
    });
    let mut cache = CoverageCache::default();
    cache.insert(
        &CoverageKey::with_hasher(&draw, CollidingHasher).unwrap(),
        7,
    );
    for edge in 0..4 {
        let mut changed = draw.clone();
        let Some(CustomCoverage::Border { widths, .. }) = &mut changed.coverage else {
            unreachable!()
        };
        match edge {
            0 => widths.top.0 += 1.,
            1 => widths.right.0 += 1.,
            2 => widths.bottom.0 += 1.,
            _ => widths.left.0 += 1.,
        }
        assert!(
            cache
                .get(&CoverageKey::with_hasher(&changed, CollidingHasher).unwrap())
                .is_none()
        );
    }
    let mut changed = draw.clone();
    let Some(CustomCoverage::Border { style, .. }) = &mut changed.coverage else {
        unreachable!()
    };
    *style = BorderStyle::Dashed;
    assert!(
        cache
            .get(&CoverageKey::with_hasher(&changed, CollidingHasher).unwrap())
            .is_none()
    );
    assert!(
        cache
            .get(&CoverageKey::with_hasher(&path_draw(1), CollidingHasher).unwrap())
            .is_none()
    );
}

#[::core::prelude::v1::test]
fn coverage_cache_hit_and_miss_lookup_allocate_nothing() {
    let original = path_draw(10_000);
    let key = CoverageKey::new(&original).unwrap();
    let mut cache = CoverageCache::default();
    cache.insert(&key, 7);
    let mut missing = original.clone();
    path_mut(&mut missing).vertices[0].st_position.x = 0.75;
    let (result, count) = allocations(|| {
        for _ in 0..16 {
            let hit = CoverageKey::new(std::hint::black_box(&original)).unwrap();
            assert_eq!(cache.get(&hit), Some(&7));
            let miss = CoverageKey::new(std::hint::black_box(&missing)).unwrap();
            assert!(cache.get(&miss).is_none());
        }
        cache.get(&key).copied()
    });
    assert_eq!(result, Some(7));
    assert_eq!(count, 0, "lookup allocated a temporary geometry key");
    assert_eq!(key.byte_len(), (8 + 4 * 10_000) * 4);
    let entry = &cache.buckets[&key.fingerprint][0];
    assert_eq!(entry.words.len() * 4, key.byte_len());
    assert_eq!(entry.words.capacity(), entry.words.len());
    cache.clear();
    assert!(cache.get(&key).is_none());
}

#[::core::prelude::v1::test]
fn coverage_cache_path_limit_rejected_before_hashing() {
    let draw = path_draw(MAX_PATH_VERTICES + 1);
    assert!(CoverageKey::with_hasher(&draw, ForbiddenHasher).is_err());
    let boundary = path_draw(MAX_PATH_VERTICES);
    assert!(CoverageKey::new(&boundary).is_ok());
}
