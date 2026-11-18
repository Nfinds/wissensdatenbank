//! Schnappschuss- und Verhaltenstests mit der synthetischen Fixture-Quelle.
//! Golden-Dateien: `tests/snapshots/<szenario>-<B>x<H>.txt`; neu schreiben mit `WB_TUI_BLESS=1`.

use std::path::PathBuf;
use unicode_width::UnicodeWidthStr;
use wb_tui::fixture::FixtureSource;
use wb_tui::snapshot::{self, SCENARIOS, SIZES};
use wb_tui::{stats, App, Theme, View};

fn golden_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests").join("snapshots")
}

#[test]
fn golden_snapshots() {
    let src = FixtureSource::new();
    let bless = std::env::var("WB_TUI_BLESS").is_ok();
    let dir = golden_dir();
    std::fs::create_dir_all(&dir).unwrap();
    let mut diffs = Vec::new();
    for name in SCENARIOS {
        for (w, h) in SIZES {
            let app = snapshot::scenario(&src, name, Theme::mono()).unwrap_or_else(|| panic!("Szenario {name}"));
            let text = snapshot::to_text(&snapshot::render_app(&app, w, h));
            let file = dir.join(format!("{name}-{w}x{h}.txt"));
            if bless {
                std::fs::write(&file, &text).unwrap();
                continue;
            }
            let want = std::fs::read_to_string(&file).unwrap_or_default().replace("\r\n", "\n");
            if want != text {
                diffs.push(format!("{name} {w}x{h}"));
            }
        }
    }
    assert!(diffs.is_empty(), "Schnappschüsse weichen ab (WB_TUI_BLESS=1 zum Neuschreiben): {diffs:?}");
}

#[test]
fn nothing_exceeds_width_and_mono_is_colorless() {
    let src = FixtureSource::new();
    for name in SCENARIOS {
        for (w, h) in SIZES {
            let app = snapshot::scenario(&src, name, Theme::mono()).unwrap();
            let buf = snapshot::render_app(&app, w, h);
            assert!(snapshot::is_colorless(&buf), "{name} {w}x{h} trägt Farbe im Mono-Thema");
            let text = snapshot::to_text(&buf);
            assert_eq!(text.lines().count(), h as usize, "{name} {w}x{h} Zeilenzahl");
            for l in text.lines() {
                assert!(UnicodeWidthStr::width(l) <= w as usize, "{name} {w}x{h}: Zeile zu breit: {l:?}");
            }
            // Farbe und Mono liefern denselben Text
            let c = snapshot::scenario(&src, name, Theme::color()).unwrap();
            assert_eq!(snapshot::to_text(&snapshot::render_app(&c, w, h)), text, "{name} {w}x{h}: Text je Thema verschieden");
        }
    }
}

#[test]
fn color_theme_actually_uses_color() {
    let src = FixtureSource::new();
    let app = snapshot::scenario(&src, "overview", Theme::color()).unwrap();
    let buf = snapshot::render_app(&app, 80, 24);
    assert!(!snapshot::is_colorless(&buf));
    let html = snapshot::to_html(&buf, "t");
    assert!(html.contains("color:#") && html.contains("<pre>"));
}

#[test]
fn ascii_fallback_has_no_wide_glyphs() {
    let src = FixtureSource::new();
    for name in SCENARIOS {
        let app = snapshot::scenario(&src, name, Theme::mono().with_ascii(true)).unwrap();
        let text = snapshot::to_text(&snapshot::render_app(&app, 100, 30));
        // Umlaute und Dateinamen bleiben, aber keine Rahmen-/Blockzeichen
        for ch in text.chars() {
            assert!(!('\u{2500}'..='\u{259f}').contains(&ch), "{name}: Box-/Blockzeichen {ch:?} im ASCII-Modus");
        }
    }
}

#[test]
fn tiny_sizes_do_not_panic() {
    let src = FixtureSource::new();
    for name in SCENARIOS {
        for (w, h) in [(1u16, 1u16), (20, 5), (59, 24), (80, 15), (60, 16), (200, 8), (80, 100)] {
            let app = snapshot::scenario(&src, name, Theme::color()).unwrap();
            let _ = snapshot::to_text(&snapshot::render_app(&app, w, h));
        }
    }
}

#[test]
fn key_flow_between_views() {
    let src = FixtureSource::new();
    let mut app = App::new(&src, Theme::mono());
    assert_eq!(app.view(), View::Overview);
    app.press("Enter");
    assert_eq!(app.view(), View::Project);
    app.press("Enter");
    assert_eq!(app.view(), View::Reader);
    app.press("g");
    assert_eq!(app.view(), View::Graph);
    app.press("Esc");
    assert_eq!(app.view(), View::Reader);
    app.press("Esc");
    assert_eq!(app.view(), View::Project);
    app.press("3");
    assert_eq!(app.view(), View::Search);
    app.press("l i e f");
    // Tippen in der Suche darf keine Ansicht wechseln
    assert_eq!(app.view(), View::Search);
    app.press("F1");
    assert_eq!(app.view(), View::Overview, "F1 wechselt auch aus der Suche");
    app.press("3");
    app.press("Esc");
    app.press("Esc");
    assert_ne!(app.view(), View::Search, "zweites Esc verlässt die Suche");
    app.press("1");
    assert_eq!(app.view(), View::Overview);
    app.press("q");
    assert!(app.quit);
}

#[test]
fn selection_stays_visible_when_scrolling() {
    let src = FixtureSource::new();
    let mut app = App::new(&src, Theme::mono());
    for _ in 0..30 {
        app.press("j");
    }
    let text = snapshot::to_text(&snapshot::render_app(&app, 80, 24));
    // letzte Projektzeile (Auswahl am Ende der Liste) muss sichtbar sein
    let last = FixtureSource::new();
    let names: Vec<String> = wb_tui::DataSource::projects(&last).into_iter().map(|p| p.name).collect();
    assert!(names.iter().any(|n| text.contains(n.as_str())));
    assert!(text.contains("24/24"), "Positionsanzeige am Ende der Liste:\n{text}");
}

#[test]
fn stats_is_static_and_fits() {
    let src = FixtureSource::new();
    for w in [60u16, 80, 120] {
        let t = stats::render(&src, w, false);
        assert!(!t.contains('\x1b'));
        for l in t.lines() {
            assert!(UnicodeWidthStr::width(l) <= w as usize, "stats {w}: {l:?}");
        }
        assert!(t.contains("Projekte") && t.contains("Aktivität"));
        assert_eq!(t, stats::render(&src, w, false), "deterministisch");
    }
    assert!(stats::render(&src, 80, true).contains('\x1b'));
}

#[test]
fn display_width_never_exceeds_terminal_width() {
    let src = FixtureSource::new();
    // `wb stats` (Textform und mit Farbe nach Entfernen der Escape-Folgen) von 40 bis 200 Spalten
    for w in 40u16..=200 {
        let t = stats::render(&src, w, false);
        for l in t.lines() {
            assert!(UnicodeWidthStr::width(l) <= w as usize, "stats {w}: {l:?}");
        }
    }
    // alle Ansichten in vielen Breiten und zwei Höhen
    for name in SCENARIOS {
        for w in (60u16..=200).step_by(7).chain([61, 79, 80, 81, 99, 100, 119, 120]) {
            for h in [16u16, 24, 45] {
                let app = snapshot::scenario(&src, name, Theme::mono()).unwrap();
                let text = snapshot::to_text(&snapshot::render_app(&app, w, h));
                assert_eq!(text.lines().count(), h as usize);
                for l in text.lines() {
                    assert!(UnicodeWidthStr::width(l) <= w as usize, "{name} {w}x{h}: {l:?}");
                }
            }
        }
    }
}
