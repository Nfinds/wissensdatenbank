//! Tests zu Vertrag §16 (Prüfung der Ansichten): Befundspalten, Neuladen, kleine Fenster, `wb stats`,
//! Tasten, ASCII, leere Daten. Alles mit erfundenen Daten (K12).

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use unicode_width::UnicodeWidthStr;
use wb_tui::fixture::FixtureSource;
use wb_tui::snapshot::{self, SCENARIOS};
use wb_tui::{stats, App, Theme, View};

fn text_of(app: &App, w: u16, h: u16) -> String {
    snapshot::to_text(&snapshot::render_app(app, w, h))
}

fn key(code: KeyCode, mods: KeyModifiers) -> KeyEvent {
    KeyEvent::new(code, mods)
}

// ---- §16.1 / H1 ----

#[test]
fn findings_head_stays_from_80_columns() {
    let src = FixtureSource::new();
    let app = App::new(&src, Theme::mono());
    for w in [80u16, 100, 120] {
        let t = text_of(&app, w, 30);
        let head = t.lines().find(|l| l.contains("Projekt") && l.contains("12 Wochen") || l.contains("Projekt") && l.contains("Alter")).unwrap_or_else(|| panic!("Kopfzeile fehlt bei {w}:\n{t}"));
        assert!(head.contains("V G T W"), "Befundspalten fehlen im Kopf bei {w}: {head:?}");
        let s = stats::render(&src, w, false);
        let head = s.lines().find(|l| l.contains("Projekt") && l.contains("V G T W"));
        assert!(head.is_some(), "stats {w}: Befundspalten fehlen:\n{s}");
    }
}

// ---- §16.3 / M1, N1 ----

#[test]
fn keys_never_panic_at_any_size() {
    let src = FixtureSource::new();
    let keys = "j k Down Up PgDn PgUp Home End Enter Tab BackTab Esc s S f g d p o e [ ] Space l / x r F1 F2 F3 F4 Enter Enter Tab Down Enter";
    for name in SCENARIOS {
        for (w, h) in [(1u16, 1u16), (2, 2), (3, 40), (40, 3), (10, 5), (30, 9), (59, 15), (59, 40), (60, 15), (60, 16), (61, 17)] {
            let mut app = snapshot::scenario(&src, name, Theme::mono()).unwrap();
            let _ = snapshot::render_app(&app, w, h);
            for k in keys.split_whitespace() {
                app.press(k);
                let _ = snapshot::render_app(&app, w, h);
            }
            app.press("F5 Down PgDn End Esc");
            let _ = snapshot::render_app(&app, w, h);
        }
    }
}

#[test]
fn keys_do_nothing_below_the_minimum_size() {
    let src = FixtureSource::new();
    let mut app = App::new(&src, Theme::mono());
    let _ = snapshot::render_app(&app, 40, 10);
    app.press("2 3 4 /");
    assert_eq!(app.view(), View::Overview, "unter der Mindestgröße wirken nur Beenden-Tasten");
    app.press("q");
    assert!(app.quit);
}

// ---- §16.4 / M2, M3 ----

#[test]
fn stats_width_is_bounded_and_complete_for_any_project_count() {
    for n in [0usize, 1, 2, 5, 12, 13, 24] {
        let src = FixtureSource::with_projects(n);
        let total = wb_tui::DataSource::projects(&src).len();
        for w in [0u16, 1, 39, 40, 41, 60, 80, 160, 240, 241, 500] {
            let t = stats::render(&src, w, false);
            let bound = w.clamp(stats::STATS_MIN_WIDTH, stats::STATS_MAX_WIDTH) as usize;
            for l in t.lines() {
                assert!(UnicodeWidthStr::width(l) <= bound, "n={n} w={w}: Zeile über {bound}: {l:?}");
            }
            assert!(t.contains("Aktivität") && t.contains("Mo") && t.contains("So"), "n={n} w={w}: Heatmap fehlt:\n{t}");
            assert!(t.contains("veraltet") && t.contains("Waise"), "n={n} w={w}: Legende fehlt:\n{t}");
            if total > stats::STATS_ROWS {
                assert!(t.contains(&format!("{} von {total}", stats::STATS_ROWS)), "n={n} w={w}: „n von m“ fehlt:\n{t}");
            } else {
                assert!(!t.contains(" von ") || t.contains("Projekten sind"), "n={n} w={w}: unnötiger Kürzungshinweis:\n{t}");
            }
        }
        // kleinere Werte verhalten sich wie 40, größere wie 240
        assert_eq!(stats::render(&src, 3, false), stats::render(&src, 40, false));
        assert_eq!(stats::render(&src, 999, false), stats::render(&src, 240, false));
    }
}

// ---- §16.5 / M4, M5, N2–N4 ----

#[test]
fn ctrl_c_quits_in_every_state() {
    let src = FixtureSource::new();
    for name in SCENARIOS {
        // normal
        let mut app = snapshot::scenario(&src, name, Theme::mono()).unwrap();
        app.handle_key(key(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert!(app.quit, "{name}: Strg-C beendet");
        // bei offener Hilfe (F5 gilt überall)
        let mut app = snapshot::scenario(&src, name, Theme::mono()).unwrap();
        let _ = snapshot::render_app(&app, 100, 30);
        app.press("F5");
        assert!(text_of(&app, 100, 30).contains("Tasten"), "{name}: Hilfe offen");
        app.handle_key(key(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert!(app.quit, "{name}: Strg-C beendet auch bei offener Hilfe");
        // zu kleines Fenster
        let mut app = snapshot::scenario(&src, name, Theme::mono()).unwrap();
        let _ = snapshot::render_app(&app, 30, 8);
        app.handle_key(key(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert!(app.quit, "{name}: Strg-C beendet im zu kleinen Fenster");
    }
}

#[test]
fn altgr_characters_are_typable_in_search() {
    let src = FixtureSource::new();
    let mut app = App::new(&src, Theme::mono());
    app.press("3");
    assert_eq!(app.view(), View::Search);
    // AltGr kommt unter Windows als Strg+Alt an
    let altgr = KeyModifiers::CONTROL | KeyModifiers::ALT;
    for c in ['@', '\\', '{', '}', '[', ']', '~', '|', '€'] {
        app.handle_key(key(KeyCode::Char(c), altgr));
    }
    let t = text_of(&app, 100, 30);
    assert!(t.contains("@\\{}[]~|€"), "AltGr-Zeichen fehlen in der Eingabezeile:\n{t}");
    // echtes Strg-U löscht weiterhin
    app.handle_key(key(KeyCode::Char('u'), KeyModifiers::CONTROL));
    assert!(!text_of(&app, 100, 30).contains("@\\{}"));
}

#[test]
fn footer_and_help_name_only_working_keys() {
    let src = FixtureSource::new();
    // Suche: `?` und `q` sind Zeichen; Hilfe und Ende stehen als F5 und Strg-C im Fuß
    let mut app = snapshot::scenario(&src, "search", Theme::mono()).unwrap();
    let t = text_of(&app, 100, 30);
    let foot = t.lines().last().unwrap().to_string();
    assert!(foot.contains("F5") && foot.contains("Strg-C"), "Suche: Fuß {foot:?}");
    assert!(!foot.contains("? Hilfe") && !foot.contains("q Ende"), "Suche: Fuß nennt tote Tasten {foot:?}");
    app.press("F5");
    let h = text_of(&app, 100, 30);
    assert!(h.contains("Strg-C") && !h.contains("Sortierung"), "Suche: Hilfe:\n{h}");
    app.press("Esc");
    // Projektkarte: kein `S`
    let mut app = snapshot::scenario(&src, "project", Theme::mono()).unwrap();
    app.press("?");
    let h = text_of(&app, 100, 30);
    assert!(h.contains("Sortierung wechseln") && !h.contains("umkehren"), "Projekt: Hilfe nennt S:\n{h}");
    // Graph: Enter nur mit gewähltem Knoten
    let mut app = snapshot::scenario(&src, "graph-notes", Theme::mono()).unwrap();
    let foot = text_of(&app, 120, 30).lines().last().unwrap().to_string();
    assert!(!foot.contains("Enter"), "Graph ohne Auswahl: Fuß nennt Enter {foot:?}");
    app.press("Enter");
    assert_eq!(app.view(), View::Graph, "Enter am Fokus tut nichts");
    app.press("Right");
    let foot = text_of(&app, 120, 30).lines().last().unwrap().to_string();
    assert!(foot.contains("Enter"), "Graph mit Auswahl: Fuß ohne Enter {foot:?}");
}

// ---- §16.9 / N5, N6 ----

#[test]
fn help_is_fully_reachable_at_60x16() {
    let src = FixtureSource::new();
    for name in SCENARIOS {
        let mut app = snapshot::scenario(&src, name, Theme::mono()).unwrap();
        let _ = snapshot::render_app(&app, 60, 16);
        app.press("F5");
        let mut seen = String::new();
        seen.push_str(&text_of(&app, 60, 16));
        for _ in 0..6 {
            app.press("PgDn");
            seen.push_str(&text_of(&app, 60, 16));
        }
        assert!(seen.contains("beenden"), "{name}: letzte Hilfezeile nie sichtbar:\n{seen}");
        assert!(seen.contains("Tasten"));
        for l in seen.lines() {
            assert!(UnicodeWidthStr::width(l) <= 60);
        }
        app.press("Esc");
        assert!(!text_of(&app, 60, 16).contains("Tasten"), "{name}: Esc schließt die Hilfe");
    }
}

#[test]
fn empty_data_shows_sensible_texts() {
    let src = FixtureSource::with_projects(0);
    let mut app = App::new(&src, Theme::mono());
    for k in ["F1", "F2", "F3", "F4"] {
        app.press(k);
        let t = text_of(&app, 100, 30);
        assert!(!t.contains("1/0"), "Ansicht {k}: Position 1/0:\n{t}");
        assert!(!t.contains("  · 0 Notizen") && !t.contains(" hat 0 Nachbarn"), "Ansicht {k}: Kernaussage mit leerem Namen:\n{t}");
    }
    app.press("F1");
    assert!(text_of(&app, 100, 30).contains("Noch keine Projekte"));
    let s = stats::render(&src, 80, false);
    assert!(s.contains("Noch keine Projekte"), "{s}");
}

// ---- §16.7 / N7, N8 ----

#[test]
fn ascii_mode_is_pure_ascii_everywhere() {
    let src = FixtureSource::new();
    let theme = Theme::mono().with_ascii(true);
    for name in SCENARIOS {
        for (w, h) in [(80u16, 24u16), (100, 30), (160, 45)] {
            let mut app = snapshot::scenario(&src, name, theme).unwrap();
            let t = text_of(&app, w, h);
            if let Some((i, c)) = t.char_indices().find(|(_, c)| !c.is_ascii() && *c != '\n') {
                panic!("{name} {w}x{h}: Zeichen {c:?} bei {i}:\n{t}");
            }
            app.press("F5");
            let t = text_of(&app, w, h);
            assert!(t.is_ascii(), "{name} {w}x{h}: Hilfe nicht ASCII");
        }
    }
    for w in [40u16, 80, 120] {
        let s = stats::render_with(&src, w, theme, false);
        assert!(s.is_ascii(), "stats {w} nicht ASCII:\n{s}");
        let s = stats::render_for(&src, w, true, false);
        assert!(s.is_ascii(), "render_for ascii {w}");
    }
}

#[test]
fn states_differing_only_by_colour_differ_in_characters() {
    use wb_tui::Freshness;
    for ascii in [false, true] {
        let g = Theme::mono().with_ascii(ascii).glyphs();
        let set: std::collections::HashSet<char> =
            [Freshness::Fresh, Freshness::Recent, Freshness::Aging, Freshness::Stale, Freshness::Unknown].iter().map(|&f| g.freshness(f)).collect();
        assert_eq!(set.len(), 5, "ascii={ascii}: Frischestufen unterscheiden sich nicht im Zeichen");
    }
}

#[test]
fn no_color_env_needs_a_non_empty_value() {
    // einziger Test dieser Datei, der die Umgebung ändert
    std::env::remove_var("NO_COLOR");
    assert!(Theme::from_env().color);
    std::env::set_var("NO_COLOR", "");
    assert!(Theme::from_env().color, "leeres NO_COLOR gilt nicht");
    std::env::set_var("NO_COLOR", "1");
    assert!(!Theme::from_env().color);
    let src = FixtureSource::new();
    assert!(!stats::render_for(&src, 80, false, true).contains('\x1b'), "NO_COLOR: kein ANSI in wb stats");
    std::env::remove_var("NO_COLOR");
    assert!(stats::render_for(&src, 80, false, true).contains('\x1b'), "ohne NO_COLOR am Terminal: ANSI");
    assert!(!stats::render_for(&src, 80, false, false).contains('\x1b'), "kein Terminal: kein ANSI");
}

// ---- §16.2 / H2 und §16.6 / M6 (mit dem Kern) ----

#[cfg(feature = "core")]
mod with_core {
    use super::*;
    use std::fs;
    use std::path::{Path, PathBuf};
    use wb_core::{OpenOptions, Vault};
    use wb_tui::core_source::CoreSource;

    struct Fx {
        _tmp: tempfile::TempDir,
        vault: PathBuf,
        index: PathBuf,
    }

    fn fx() -> Fx {
        let tmp = tempfile::tempdir().unwrap();
        let vault = tmp.path().join("vault");
        let index = tmp.path().join("index");
        fs::create_dir_all(&vault).unwrap();
        Fx { _tmp: tmp, vault, index }
    }

    fn note(root: &Path, rel: &str, kind: &str, title: &str, body: &str) {
        let p = root.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, format!("---\ntype: {kind}\nchecked_on: 2026-11-01\n---\n# {title}\n\n{body}\n")).unwrap();
    }

    #[test]
    fn r_picks_up_changes_to_the_vault_even_read_only() {
        let f = fx();
        note(&f.vault, "Projects/Demo Alpha/00 Overview.md", "overview", "Demo Alpha", "Ein erfundenes Projekt.");
        note(&f.vault, "Projects/Demo Alpha/Thema Eins.md", "topic", "Thema Eins", "Alter Text zur Lagerung.");
        let v = Vault::open(OpenOptions::new(&f.vault).index_dir(&f.index).read_only(true).create_index(true)).unwrap();
        v.sync().unwrap();
        let src = CoreSource::new(&v);
        let mut app = App::new(&src, Theme::mono());
        let before = text_of(&app, 100, 30);
        assert!(before.contains("Demo Alpha") && !before.contains("Demo Beta"));

        // neues Projekt und geänderte Notiz ohne Zutun der Oberfläche
        note(&f.vault, "Projects/Demo Beta/00 Overview.md", "overview", "Demo Beta", "Noch ein erfundenes Projekt.");
        note(&f.vault, "Projects/Demo Beta/Thema Zwei.md", "topic", "Thema Zwei", "Etwas.");
        note(&f.vault, "Projects/Demo Alpha/Thema Eins.md", "topic", "Thema Eins", "Neuer Text zur Beleuchtung, deutlich länger als vorher.");
        assert!(!text_of(&app, 100, 30).contains("Demo Beta"), "ohne `r` bleibt alles beim Alten");

        // `r` gleicht den Index an und verwirft die Zwischenspeicher
        app.press("r");
        let after = text_of(&app, 100, 30);
        assert!(after.contains("Demo Beta"), "neues Projekt nach `r` sichtbar:\n{after}");

        // geänderter Text im Leser
        app.press("Enter");
        let alpha = text_of(&app, 100, 30);
        assert!(alpha.contains("Thema Eins") || alpha.contains("Demo Alpha"));
        app.press("2");
        let proj = text_of(&app, 100, 30);
        assert!(proj.contains("Thema Eins"), "{proj}");
        let path = wb_tui::DataSource::notes(&src, "Demo Alpha")
            .into_iter()
            .find(|n| n.title == "Thema Eins")
            .unwrap_or_else(|| panic!("Thema Eins fehlt"))
            .path;
        app.open_reader(&path);
        let rd = text_of(&app, 100, 30);
        assert!(rd.contains("Neuer Text"), "Leser zeigt den geänderten Text:
{rd}");
        note(&f.vault, "Projects/Demo Alpha/Thema Eins.md", "topic", "Thema Eins", "Dritte Fassung mit anderem Wortlaut und mehr Zeichen als zuvor.");
        app.press("r");
        assert!(text_of(&app, 100, 30).contains("Dritte Fassung"), "Leser nach `r` aktuell");
    }

    #[test]
    fn no_key_is_slow_on_a_larger_vault() {
        let f = fx();
        for p in 0..8 {
            note(&f.vault, &format!("Projects/Demo {p}/00 Overview.md"), "overview", &format!("Demo {p}"), "Erfunden.");
            for n in 0..40 {
                note(
                    &f.vault,
                    &format!("Projects/Demo {p}/Thema {n:02}.md"),
                    "topic",
                    &format!("Thema {n:02}"),
                    &format!("Absatz {n} zu Lagerung und Beleuchtung. Siehe [[Thema {:02}]].", (n + 1) % 40),
                );
            }
        }
        let v = Vault::open(OpenOptions::new(&f.vault).index_dir(&f.index)).unwrap();
        v.sync().unwrap();
        let src = CoreSource::new(&v);
        let mut app = App::new(&src, Theme::mono());
        let _ = snapshot::render_app(&app, 120, 40);
        // Obergrenze großzügig, weil Tests ohne Optimierung laufen; §16.6 verlangt 100 ms auf echten Daten
        let limit = std::time::Duration::from_millis(300);
        for k in ["3", "l", "a", "g", "e", "Backspace", "Down", "Down", "Enter", "Esc", "Esc", "1", "j", "Enter", "j", "2", "4"] {
            let t = std::time::Instant::now();
            app.press(k);
            let _ = snapshot::render_app(&app, 120, 40);
            let took = t.elapsed();
            assert!(took < limit, "Taste {k:?} brauchte {took:?}");
        }
    }
}
