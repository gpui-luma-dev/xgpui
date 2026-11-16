//! Localization QA (#355): the checker, the findings each text path produces,
//! and the opt-in report (`just text l10n-report`).
use super::{l10n, paragraph, samples};
use gpui::{
    PlatformTextSystem, TestAppContext, TestDispatcher, VisualTestContext, div, point, prelude::*,
    px, size,
};
use l10n::{Finding, LOCALES, Problem};
use std::sync::Arc;
use xgpui_parley::ParleyTextSystem;

/// A Parley backend with the lab's bundled fonts plus system fonts (for Thai,
/// Khmer, Japanese and Devanagari where the system has them).
fn parley() -> Arc<ParleyTextSystem> {
    let mut fonts = parley::FontContext::new();
    for data in std::iter::once(samples::PLEX).chain(paragraph::FONTS) {
        fonts
            .collection
            .register_fonts(parley::fontique::Blob::new(Arc::new(data.to_vec())), None);
    }
    Arc::new(ParleyTextSystem::new(fonts, "IBM Plex Sans").unwrap())
}

#[cfg(target_os = "macos")]
fn native() -> Arc<dyn PlatformTextSystem> {
    let backend: Arc<dyn PlatformTextSystem> = Arc::new(gpui_apple::AppleTextSystem::new());
    backend
        .add_fonts(
            std::iter::once(samples::PLEX)
                .chain(paragraph::FONTS)
                .map(std::borrow::Cow::Borrowed)
                .collect(),
        )
        .unwrap();
    backend
}

/// The text paths the lab compares.
#[derive(Clone, Copy, Debug)]
enum Path {
    /// GPUI's flat lines, shaped by Parley.
    ParleyFlat,
    /// Parley through `install_text_layout`.
    Parley,
    /// GPUI's flat lines on the platform's text system.
    #[cfg(target_os = "macos")]
    Native,
}

impl Path {
    fn label(self) -> &'static str {
        match self {
            Path::ParleyFlat => "GPUI lines (Parley shaping)",
            Path::Parley => "Parley (install_text_layout)",
            #[cfg(target_os = "macos")]
            Path::Native => "GPUI lines (CoreText)",
        }
    }
}

/// Draws the lab on `path` and returns its findings.
fn findings(path: Path) -> Vec<Finding> {
    let (backend, parley): (Arc<dyn PlatformTextSystem>, _) = match path {
        Path::ParleyFlat | Path::Parley => {
            let parley = parley();
            (parley.clone(), Some(parley))
        }
        #[cfg(target_os = "macos")]
        Path::Native => (native(), None),
    };
    let mut cx = TestAppContext::build_with_text_system(TestDispatcher::new(0), None, backend);
    let window = cx.open_window(size(px(1300.), px(3000.)), |_, cx| {
        let mut lab = l10n::L10nLab::new("test".into(), parley, cx);
        lab.set_plain_on_parley(matches!(path, Path::Parley));
        lab
    });
    let lab = window.root(&mut cx).unwrap();
    let mut visual = VisualTestContext::from_window(window.into(), &cx);
    visual.draw(point(px(0.), px(0.)), size(px(1300.), px(3000.)), |_, _| {
        div().size_full().child(lab.clone())
    });
    visual.update(|window, cx| {
        lab.update(cx, |lab, _| {
            lab.audit(window);
            assert!(lab.audited());
            lab.findings().to_vec()
        })
    })
}

fn has(findings: &[Finding], locale: &str, problem: Problem) -> bool {
    findings
        .iter()
        .any(|finding| finding.locale == locale && finding.problem == problem)
}

#[test]
fn check_breaks_follows_icu_line_breaks_and_clusters() {
    // English: a break after a space is allowed; inside a word it isn't.
    assert!(l10n::check_breaks("one two", "one \ntwo").is_empty());
    assert_eq!(
        l10n::check_breaks("one two", "on\ne two"),
        [(Problem::BadBreak, 2)]
    );
    // Thai: between the words ข้อมูล and ทั้งหมด is allowed; inside the
    // syllable ทั้ง (after ทั้), as GPUI's flat lines break it, isn't.
    let thai = "ข้อมูลทั้งหมด";
    let word = "ข้อมูล".len();
    assert!(l10n::check_breaks(thai, &format!("{}\n{}", &thai[..word], &thai[word..])).is_empty());
    let inside = word + "ทั้".len();
    assert_eq!(
        l10n::check_breaks(thai, &format!("{}\n{}", &thai[..inside], &thai[inside..])),
        [(Problem::BadBreak, inside)]
    );
    // A break between a base letter and its vowel sign splits a cluster.
    let khmer = "នៃ";
    let base = "ន".len();
    assert_eq!(
        l10n::check_breaks(khmer, &format!("{}\n{}", &khmer[..base], &khmer[base..])),
        [(Problem::SplitCluster, base)]
    );
    // Lines that don't rebuild the text (truncation) aren't checked.
    assert!(l10n::check_breaks("one two", "one…").is_empty());
}

#[test]
fn parley_breaks_every_locale_at_allowed_points() {
    let findings = findings(Path::Parley);
    let breaks: Vec<_> = findings
        .iter()
        .filter(|finding| finding.problem != Problem::Overflow)
        .collect();
    assert!(breaks.is_empty(), "{breaks:#?}");
    // Parley doesn't break inside a word, so the German compound overflows.
    assert!(has(&findings, "de", Problem::Overflow), "{findings:#?}");
}

#[test]
fn gpui_lines_break_thai_inside_words() {
    let findings = findings(Path::ParleyFlat);
    // GPUI's line breaker has no dictionary for Thai.
    assert!(has(&findings, "th", Problem::BadBreak), "{findings:#?}");
    // The German compound gets an emergency break instead of overflowing.
    assert!(has(&findings, "de", Problem::BadBreak), "{findings:#?}");
    assert!(!has(&findings, "de", Problem::Overflow), "{findings:#?}");
}

#[cfg(target_os = "macos")]
#[test]
fn native_lines_break_thai_inside_words() {
    let findings = findings(Path::Native);
    assert!(has(&findings, "th", Problem::BadBreak), "{findings:#?}");
}

/// Prints problems per locale for each text path. Run with
/// `just text l10n-report`.
#[test]
#[ignore = "report; run with `just text l10n-report`"]
fn l10n_report() {
    let paths = [
        Path::ParleyFlat,
        Path::Parley,
        #[cfg(target_os = "macos")]
        Path::Native,
    ];
    for path in paths {
        let findings = findings(path);
        println!("\n## {} — {} problems", path.label(), findings.len());
        println!(
            "{:<6} {:>10} {:>14} {:>9}",
            "locale", "bad break", "split cluster", "overflow"
        );
        for locale in LOCALES {
            let count = |problem| {
                findings
                    .iter()
                    .filter(|finding| finding.locale == locale.tag && finding.problem == problem)
                    .count()
            };
            println!(
                "{:<6} {:>10} {:>14} {:>9}",
                locale.tag,
                count(Problem::BadBreak),
                count(Problem::SplitCluster),
                count(Problem::Overflow)
            );
        }
        for finding in &findings {
            println!(
                "  {} · {} · {}: {}",
                finding.locale, finding.problem, finding.element, finding.detail
            );
        }
    }
}
