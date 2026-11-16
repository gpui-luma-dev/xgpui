//! Frame cost of plain GPUI text on GPUI's flat lines versus the Parley engine
//! (`xgpui_parley::install_text_layout`, gpui-luma-dev/xgpui#332), both on the
//! Parley backend. Opt-in: `just text layout-cost` (release build).
//!
//! Each frame draws a busy page of plain text elements: short Latin and Hebrew
//! labels, wrapped Latin and Hebrew paragraphs, and truncated cells. Three
//! scenarios:
//! - cold: every frame uses new strings, so nothing is cached;
//! - warm: the same page again, as when a window redraws unchanged;
//! - reflow: the same strings at alternating widths, as when resizing.
use super::{paragraph, samples};
use gpui::{
    AvailableSpace, Pixels, TestAppContext, TestDispatcher, VisualTestContext, div, point,
    prelude::*, px, size,
};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use xgpui_parley::ParleyTextSystem;

const LABELS: usize = 100;
const PARAGRAPHS: usize = 40;
const CELLS: usize = 40;
const FRAMES: usize = 60;
const WARMUP: usize = 5;

const LATIN: &str = "Plain GPUI text wraps across lines when it is longer than the width of the box that holds it, like this sentence";
const HEBREW: &str = "טקסט רגיל של GPUI עובר לשורה הבאה כשהוא ארוך יותר מרוחב התיבה שמכילה אותו, כמו המשפט הזה בדיוק";

fn parley() -> Arc<ParleyTextSystem> {
    let mut fonts = parley::FontContext::new();
    for data in std::iter::once(samples::PLEX).chain(paragraph::FONTS) {
        fonts
            .collection
            .register_fonts(parley::fontique::Blob::new(Arc::new(data.to_vec())), None);
    }
    Arc::new(ParleyTextSystem::new(fonts, "IBM Plex Sans").unwrap())
}

/// One page of plain text elements; `salt` makes every string distinct.
fn page(salt: usize, width: Pixels) -> gpui::Div {
    let label = |ix: usize| {
        if ix.is_multiple_of(2) {
            format!("Settings item {ix} · {salt}")
        } else {
            format!("פריט הגדרות {ix} · {salt}")
        }
    };
    let paragraph = |ix: usize| {
        let base = if ix.is_multiple_of(2) { LATIN } else { HEBREW };
        format!("{base} {ix}.{salt}")
    };
    div()
        .flex()
        .flex_col()
        .font_family("IBM Plex Sans")
        .text_size(px(14.))
        .children((0..LABELS).map(|ix| div().whitespace_nowrap().child(label(ix))))
        .children((0..PARAGRAPHS).map(|ix| div().w(width).child(paragraph(ix))))
        .children((0..CELLS).map(|ix| div().w(px(140.)).truncate().child(paragraph(ix))))
}

fn draw(visual: &mut VisualTestContext, salt: usize, width: Pixels) -> Duration {
    let start = Instant::now();
    visual.draw(
        point(px(0.), px(0.)),
        size(
            AvailableSpace::Definite(px(1200.)),
            AvailableSpace::MinContent,
        ),
        |_, _| page(salt, width),
    );
    start.elapsed()
}

#[derive(Clone, Copy)]
enum Scenario {
    Cold,
    Warm,
    Reflow,
}

/// Median and 90th percentile frame time in milliseconds.
fn measure(provider: bool, scenario: Scenario) -> (f64, f64) {
    let parley = parley();
    let mut cx =
        TestAppContext::build_with_text_system(TestDispatcher::new(0), None, parley.clone());
    if provider {
        cx.update(|cx| xgpui_parley::install_text_layout(parley, cx));
    }
    let visual = cx.add_empty_window();
    let frame = |visual: &mut VisualTestContext, ix: usize| match scenario {
        Scenario::Cold => draw(visual, ix + 1, px(220.)),
        Scenario::Warm => draw(visual, 0, px(220.)),
        Scenario::Reflow => draw(
            visual,
            0,
            if ix.is_multiple_of(2) {
                px(220.)
            } else {
                px(260.)
            },
        ),
    };
    for ix in 0..WARMUP {
        frame(visual, FRAMES + ix);
    }
    let mut times: Vec<f64> = (0..FRAMES)
        .map(|ix| frame(visual, ix).as_secs_f64() * 1000.)
        .collect();
    times.sort_by(f64::total_cmp);
    (times[FRAMES / 2], times[FRAMES * 9 / 10])
}

#[test]
#[ignore = "timing; run with `just text layout-cost`"]
fn plain_text_layout_cost() {
    println!(
        "Plain text frame cost: {} labels, {} wrapped paragraphs, {} truncated cells per frame; {} frames, release={}",
        LABELS,
        PARAGRAPHS,
        CELLS,
        FRAMES,
        !cfg!(debug_assertions)
    );
    println!(
        "{:<8} {:>17} {:>17} {:>9}",
        "scenario", "GPUI lines (ms)", "Parley (ms)", "median ×"
    );
    for (name, scenario) in [
        ("cold", Scenario::Cold),
        ("warm", Scenario::Warm),
        ("reflow", Scenario::Reflow),
    ] {
        let flat = measure(false, scenario);
        let parley = measure(true, scenario);
        println!(
            "{name:<8} {:>7.2} p90 {:>5.2} {:>7.2} p90 {:>5.2} {:>9.2}",
            flat.0,
            flat.1,
            parley.0,
            parley.1,
            parley.0 / flat.0
        );
    }
}
