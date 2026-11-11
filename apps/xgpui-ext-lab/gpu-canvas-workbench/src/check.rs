//! `--check`: replay a deterministic edit script on the engine and compare
//! every working pixel, the published values and the engine's lifecycle
//! against the CPU reference. No window; exits non-zero on failure.
use crate::engine::{Engine, MAX_DABS};
use crate::oracle::{Dab, HEIGHT, Reference, WIDTH, base, covers, f16_to_f32, published};
use anyhow::{Result, bail, ensure};

const WORKING_TOLERANCE: f32 = 1e-4;
const PUBLISHED_TOLERANCE: f32 = 2e-3;

/// Deterministic strokes: saturated P3 and near-black colours, fractional
/// opacity, small and large radii, a stroke crossing the canvas edge, and
/// discs with sub-pixel centres for partial pixel coverage at the rim.
pub fn script() -> Vec<Dab> {
    let mut dabs = Vec::new();
    let colors = [
        [1., 0., 0.],
        [0., 1., 0.],
        [0.004, 0.004, 0.004],
        [0.2, 0.4, 1.],
    ];
    let mut seed = 0x2545_f491_u32;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        (seed % 10_000) as f32 / 10_000.
    };
    for stroke in 0..12 {
        let color = colors[stroke % colors.len()];
        let radius = [3.5, 9.25, 22.0][stroke % 3];
        let opacity = [1.0, 0.35, 0.6][stroke % 3];
        let (mut x, mut y) = (next() * WIDTH as f32, next() * HEIGHT as f32);
        let (dx, dy) = (next() * 16. - 8., next() * 16. - 8.);
        for _ in 0..20 {
            dabs.push(Dab {
                x,
                y,
                radius,
                opacity,
                color,
            });
            x += dx;
            y += dy;
        }
    }
    // Off the left edge, half visible.
    for i in 0..10 {
        dabs.push(Dab {
            x: -4.,
            y: 40. + i as f32 * 30.,
            radius: 12.3,
            opacity: 0.8,
            color: [0., 0., 1.],
        });
    }
    dabs
}

fn report(ok: bool, line: impl std::fmt::Display) -> bool {
    println!("{} {line}", if ok { "PASS" } else { "FAIL" });
    ok
}

fn compare_working(engine: &mut Engine, reference: &Reference) -> Result<(usize, f32)> {
    let working = engine.read_working()?;
    let mut mismatches = 0;
    let mut worst = 0f32;
    for (pixel, expected) in working.iter().zip(&reference.pixels) {
        let error = (0..3)
            .map(|c| (pixel[c] - expected[c]).abs())
            .fold((pixel[3] - 1.).abs(), f32::max);
        worst = worst.max(error);
        if error > WORKING_TOLERANCE {
            mismatches += 1;
        }
    }
    Ok((mismatches, worst))
}

fn compare_published(
    engine: &mut Engine,
    reference: &Reference,
    exposure: f32,
) -> Result<(usize, f32)> {
    let slot = engine.read_published()?;
    let mut mismatches = 0;
    let mut worst = 0f32;
    for (texel, expected) in slot.iter().zip(&reference.pixels) {
        let expected = published(*expected, exposure);
        let error = (0..3)
            .map(|c| (f16_to_f32(texel[c]) - expected[c]).abs())
            .fold((f16_to_f32(texel[3]) - 1.).abs(), f32::max);
        worst = worst.max(error);
        if error > PUBLISHED_TOLERANCE {
            mismatches += 1;
        }
    }
    Ok((mismatches, worst))
}

pub fn run(slots: usize) -> Result<()> {
    let mut engine = Engine::new(slots)?;
    let mut ok = true;
    let mut reference = Reference::new();

    engine.frame()?;
    ok &= report(
        engine.surface().stats().presents == 1 && engine.surface().content().is_some(),
        "generated image presented before any edit",
    );
    let (bad, worst) = compare_working(&mut engine, &reference)?;
    ok &= report(
        bad == 0,
        format!(
            "generated image matches the CPU reference ({bad} mismatches, max error {worst:.2e})"
        ),
    );
    let (bad, worst) = compare_published(&mut engine, &reference, 0.)?;
    ok &= report(
        bad == 0,
        format!("published values match ({bad} mismatches, max error {worst:.2e})"),
    );

    // Replay strokes in frames of 7 dabs.
    let script = script();
    for chunk in script.chunks(7) {
        for dab in chunk {
            engine.dab(*dab);
            reference.apply(dab);
        }
        engine.frame()?;
    }
    let (bad, worst) = compare_working(&mut engine, &reference)?;
    let touched = (0..HEIGHT)
        .flat_map(|y| (0..WIDTH).map(move |x| (x, y)))
        .filter(|&(x, y)| script.iter().any(|dab| covers(dab, x, y)))
        .count();
    let working = engine.read_working()?;
    let untouched_changed = (0..HEIGHT)
        .flat_map(|y| (0..WIDTH).map(move |x| (x, y)))
        .filter(|&(x, y)| !script.iter().any(|dab| covers(dab, x, y)))
        .filter(|&(x, y)| {
            let pixel = working[(y * WIDTH + x) as usize];
            base(x, y)
                .iter()
                .zip(pixel)
                .any(|(e, a)| (e - a).abs() > WORKING_TOLERANCE)
        })
        .count();
    ok &= report(
        bad == 0 && untouched_changed == 0,
        format!(
            "{} dabs replayed: {touched} touched pixels match, {} untouched pixels unchanged ({bad} mismatches, {untouched_changed} untouched changed, max error {worst:.2e})",
            script.len(),
            (WIDTH * HEIGHT) as usize - touched
        ),
    );
    let stats = engine.stats();
    ok &= report(
        stats.dabs_applied == script.len() as u64
            && stats.tiles_touched
                < stats.paint_passes * (WIDTH.div_ceil(64) * HEIGHT.div_ceil(64)) as u64,
        format!(
            "edits touched only their region: {} paint passes, {} tiles touched (full image is {} tiles per pass)",
            stats.paint_passes,
            stats.tiles_touched,
            (WIDTH.div_ceil(64) * HEIGHT.div_ceil(64))
        ),
    );

    // Presentation-only frames: no GPU work, no new snapshot, no re-applied dab.
    let before = engine.stats();
    for _ in 0..5 {
        engine.frame()?;
    }
    let after = engine.stats();
    ok &= report(
        after.publishes == before.publishes
            && after.submissions == before.submissions
            && after.dabs_applied == before.dabs_applied
            && after.revision == before.revision,
        format!(
            "5 presentation-only frames: revision {} unchanged, no submissions, no dab applied twice",
            after.revision
        ),
    );

    // Exposure changes the published snapshot, not the working image.
    engine.set_exposure(1.0);
    engine.frame()?;
    let (bad_working, _) = compare_working(&mut engine, &reference)?;
    let (bad, worst) = compare_published(&mut engine, &reference, 1.0)?;
    ok &= report(
        bad == 0 && bad_working == 0,
        format!(
            "+1 stop exposure republished the whole image ({bad} mismatches, max error {worst:.2e}); working image unchanged"
        ),
    );
    engine.set_exposure(0.);
    engine.frame()?;

    // Inspection keeps its revision while edits continue.
    let (ix, iy) = (200, 170);
    let expected: Vec<[f32; 3]> = (0..4)
        .flat_map(|r| (0..4).map(move |c| (ix + c, iy + r)))
        .map(|(x, y)| reference.at(x, y))
        .collect();
    ensure!(engine.inspect(ix, iy), "inspection rejected");
    let requested = engine.stats().revision;
    for i in 0..30 {
        let dab = Dab {
            x: ix as f32 + 2.,
            y: iy as f32 + 2.,
            radius: 6.,
            opacity: 0.5,
            color: [i as f32 / 30., 0., 1.],
        };
        engine.dab(dab);
        reference.apply(&dab);
        engine.frame()?;
    }
    let mut inspection = None;
    for _ in 0..100 {
        engine.wait_idle();
        if let Some(found) = engine.poll_inspections().pop() {
            inspection = Some(found);
            break;
        }
    }
    let Some(inspection) = inspection else {
        bail!("inspection never completed");
    };
    let values_match = inspection
        .pixels
        .iter()
        .zip(&expected)
        .all(|(a, e)| (0..3).all(|c| (a[c] - e[c]).abs() <= WORKING_TOLERANCE));
    ok &= report(
        inspection.revision == requested && values_match,
        format!(
            "inspection at ({ix},{iy}) reports revision {} (requested at {requested}, now {}) with that revision's values",
            inspection.revision,
            engine.stats().revision
        ),
    );

    // Bounded inspections: staging buffers are reused, never grown.
    let mut accepted = 0;
    for i in 0..10 {
        accepted += engine.inspect(i * 10, 0) as usize;
    }
    ok &= report(
        accepted == 4,
        format!("inspection requests bounded: {accepted} of 10 accepted while busy"),
    );
    for _ in 0..100 {
        engine.wait_idle();
        engine.poll_inspections();
        if engine.inspections_in_flight() == 0 {
            break;
        }
    }

    // Steady state: no new GPU objects, one snapshot per edited frame.
    let resources = engine.stats().resources_created;
    let publishes = engine.stats().publishes;
    for i in 0..200 {
        let dab = Dab {
            x: (i * 3 % WIDTH) as f32,
            y: (i * 7 % HEIGHT) as f32,
            radius: 5.,
            opacity: 0.7,
            color: [0.9, 0.5, 0.1],
        };
        engine.dab(dab);
        reference.apply(&dab);
        engine.frame()?;
    }
    let stats = engine.stats();
    ok &= report(
        stats.resources_created == resources && stats.publishes == publishes + 200,
        format!(
            "200 edited frames: {} snapshots, {} GPU objects before and after",
            stats.publishes - publishes,
            stats.resources_created
        ),
    );

    // More dabs than one pass holds are all applied, in order.
    for i in 0..(MAX_DABS * 2 + 37) {
        let dab = Dab {
            x: 320. + (i % 50) as f32 * 2.,
            y: 200. + (i / 50) as f32 * 2.,
            radius: 4.5,
            opacity: 0.3,
            color: [0., (i % 7) as f32 / 7., 0.5],
        };
        engine.dab(dab);
        reference.apply(&dab);
    }
    engine.frame()?;
    let (bad, worst) = compare_working(&mut engine, &reference)?;
    ok &= report(
        bad == 0 && engine.pending_dabs() == 0,
        format!(
            "{} queued dabs applied in order across passes ({bad} mismatches, max error {worst:.2e})",
            MAX_DABS * 2 + 37
        ),
    );

    // Pressure: retained snapshots pin every slot. Edits keep landing in the
    // working image, publishing is deferred without blocking, and releasing
    // the snapshots publishes the newest revision.
    let mut held = Vec::new();
    let slots = engine.surface().desc().slots;
    let mut pinned_dab = 0;
    while held.len() < slots {
        if let Some(content) = engine.surface().content() {
            held.push(content);
        }
        let dab = Dab {
            x: 100. + pinned_dab as f32 * 9.,
            y: 60.,
            radius: 4.,
            opacity: 1.,
            color: [1., 0., 1.],
        };
        pinned_dab += 1;
        engine.dab(dab);
        reference.apply(&dab);
        engine.frame()?;
        ensure!(pinned_dab < 50, "could not pin every slot");
    }
    let blocked_before = engine.stats().publish_blocked;
    let started = std::time::Instant::now();
    for i in 0..10 {
        let dab = Dab {
            x: 500.,
            y: 40. + i as f32 * 4.,
            radius: 3.,
            opacity: 1.,
            color: [0., 1., 1.],
        };
        engine.dab(dab);
        reference.apply(&dab);
        engine.frame()?;
    }
    let blocked = engine.stats().publish_blocked - blocked_before;
    let stalled = started.elapsed();
    let (bad_held, _) = compare_working(&mut engine, &reference)?;
    let unpublished = engine.stats().revision - engine.stats().published_revision;
    drop(held);
    engine.frame()?;
    let caught_up = engine.stats().revision == engine.stats().published_revision;
    ok &= report(
        blocked == 10 && unpublished > 0 && caught_up && bad_held == 0 && stalled.as_millis() < 500,
        format!(
            "{slots} held snapshots pinned the ring: 10 edits applied, publish deferred {blocked}× without blocking ({stalled:?}); released, the newest revision published"
        ),
    );

    // Generation replacement: the device is lost with an inspection in
    // flight and edits still arriving. The rebuilt engine replays the CPU
    // edit log on a new device and surface; the old inspection is never
    // delivered; new inspections report the new generation.
    ensure!(engine.inspect(300, 200), "inspection rejected");
    let old_surface = engine.surface().clone();
    engine.lose_device();
    let lost_frame_rejected = engine.frame().is_err() && !engine.inspect(10, 10);
    let during = Dab {
        x: 300.,
        y: 200.,
        radius: 7.,
        opacity: 1.,
        color: [0.25, 0.5, 0.75],
    };
    engine.dab(during);
    reference.apply(&during);
    let old_still_shows = old_surface.content().is_some() && !old_surface.is_valid();
    let logged = engine.logged_dabs();
    engine.rebuild()?;
    let stale = engine.poll_inspections();
    let (bad, worst) = compare_working(&mut engine, &reference)?;
    let stats = engine.stats();
    ensure!(
        engine.inspect(300, 200),
        "inspection rejected after rebuild"
    );
    let mut fresh = None;
    for _ in 0..100 {
        engine.wait_idle();
        if let Some(found) = engine.poll_inspections().pop() {
            fresh = Some(found);
            break;
        }
    }
    let fresh_ok = fresh.as_ref().is_some_and(|found| {
        found.generation == 1
            && (0..3).all(|c| {
                (found.pixels[0][c] - reference.at(300, 200)[c]).abs() <= WORKING_TOLERANCE
            })
    });
    ok &= report(
        lost_frame_rejected
            && old_still_shows
            && stale.is_empty()
            && bad == 0
            && stats.recoveries == 1
            && stats.published_revision == stats.revision
            && engine.surface().content().is_some()
            && fresh_ok,
        format!(
            "device lost and replaced: {logged} logged dabs replayed in {:.1} ms on generation {} ({bad} mismatches, max error {worst:.2e}); old inspection dropped, new one reports generation 1; last frame kept on the old surface meanwhile",
            stats.replay_ms,
            engine.generation(),
        ),
    );

    // Reset regenerates the image and discards queued edits.
    engine.dab(Dab {
        x: 10.,
        y: 10.,
        radius: 8.,
        opacity: 1.,
        color: [1., 1., 1.],
    });
    engine.reset();
    engine.frame()?;
    let (bad, _) = compare_working(&mut engine, &Reference::new())?;
    ok &= report(
        bad == 0,
        format!("reset regenerated the image and dropped queued dabs ({bad} mismatches)"),
    );

    let stats = engine.stats();
    let latency = percentile(&engine.latencies_ms, 0.5);
    println!(
        "engine: {} revisions, {} snapshots ({:.1} MiB written by GPU publish), {} submissions, {:.1} KiB uploaded, {} B read back by inspection, {} GPU objects, edit-to-publish p50 {latency:.3} ms",
        stats.revision,
        stats.publishes,
        stats.gpu_bytes_published as f64 / (1024. * 1024.),
        stats.submissions,
        stats.cpu_bytes_uploaded as f64 / 1024.,
        stats.cpu_bytes_read,
        stats.resources_created
    );
    ensure!(ok, "workbench check failed");
    println!("workbench check: all passed");
    Ok(())
}

pub fn percentile(values: &[f32], q: f32) -> f32 {
    if values.is_empty() {
        return 0.;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(f32::total_cmp);
    sorted[((sorted.len() - 1) as f32 * q) as usize]
}
