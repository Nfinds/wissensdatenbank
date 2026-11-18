//! Tests zu Vertrag §18.3 bis §18.6: Wortgebrauch „aktiv“, Offen, Befunde wie `wb check`, Ortszeit.
//! Alles mit erfundenen Daten (K12).

use wb_tui::fixture::FixtureSource;
use wb_tui::snapshot;
use wb_tui::{App, Theme};

fn text_of(app: &App, w: u16, h: u16) -> String {
    snapshot::to_text(&snapshot::render_app(app, w, h))
}

// ---- §18.3 „aktiv“ heißt nur `status: active`, Zeiträume stehen da ----

#[test]
fn active_means_status_active_and_periods_are_named() {
    use wb_tui::{DataSource, Status};
    let src = FixtureSource::new();
    let want = src.projects().iter().filter(|p| p.status == Some(Status::Active)).count();
    let mut app = App::new(&src, Theme::mono());
    let t = text_of(&app, 130, 30);
    let head = t.lines().nth(1).unwrap().to_string();
    assert!(head.contains(&format!("{want} aktiv")), "Kopfsatz: {head}");
    assert!(head.contains("in 7 Tagen geändert"), "Zeitraum fehlt: {head}");
    // enge Fassung nennt den Zeitraum ebenfalls
    let narrow = text_of(&app, 80, 24).lines().nth(1).unwrap().to_string();
    assert!(narrow.contains("in 7 T. geändert"), "{narrow}");
    // Filter `f`: Hinweis und Kopfsatz nennen 30 Tage
    assert!(t.contains("f 30 Tage"), "{t}");
    app.press("f");
    let t = text_of(&app, 130, 30);
    assert!(t.lines().nth(1).unwrap().contains("30 Tagen"), "{t}");
}

#[test]
fn singular_in_the_headline() {
    let src = FixtureSource::with_projects(1);
    let app = App::new(&src, Theme::mono());
    let head = text_of(&app, 120, 30).lines().nth(1).unwrap().to_string();
    assert!(head.starts_with(" 1 Projekt ") && !head.contains("1 Projekte"), "{head}");
}

// ---- Echte Kernabfragen: Offen, Befunde, Ortszeit ----

#[cfg(feature = "core")]
mod core_tests {
    use super::*;
    use std::fs;
    use std::path::{Path, PathBuf};
    use wb_core::{CheckOptions, OpenOptions, Vault};
    use wb_tui::core_source::CoreSource;
    use wb_tui::DataSource;

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

    fn write(root: &Path, rel: &str, kind: &str, body: &str) {
        let p = root.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, format!("---\ntype: {kind}\nchecked_on: 2026-11-01\n---\n{body}\n")).unwrap();
    }

    fn open(f: &Fx) -> Vault {
        let v = Vault::open(OpenOptions::new(&f.vault).index_dir(&f.index).read_only(true).create_index(true)).unwrap();
        v.sync().unwrap();
        v
    }

    fn today() -> wb_tui::Date {
        wb_tui::Date::parse("2026-11-10").unwrap()
    }

    // ---- §18.4 ----

    #[test]
    fn open_detail_shows_first_line_without_bullets_and_the_column_number() {
        let f = fx();
        write(
            &f.vault,
            "Projects/Demo Alpha/00 Overview.md",
            "overview",
            "# Demo Alpha\n\n## Stand\n\nLäuft.\n\n## Offen\n\nNoch nichts Festes, erst die Messung abwarten.\n",
        );
        write(&f.vault, "Projects/Demo Alpha/Thema Eins.md", "topic", "# Thema Eins\n\n## Offen\n\n- Punkt A\n- Punkt B\n- Punkt C\n");
        let v = open(&f);
        let src = CoreSource::with_today(&v, today());
        let col = src.projects()[0].open_points;
        assert_eq!(col, 3);
        let app = App::new(&src, Theme::mono());
        let t = text_of(&app, 160, 45);
        let detail = t.lines().find(|l| l.contains("Offen   ")).unwrap_or_else(|| panic!("kein Offen-Feld:\n{t}"));
        assert!(detail.contains(&format!("Offen   {col} · Noch nichts Festes")), "{detail}");
        // dieselbe Zahl in der Spalte
        let row = t.lines().find(|l| l.contains("Demo Alpha") && !l.contains('▸')).unwrap();
        assert!(row.split_whitespace().any(|w| w == col.to_string()), "Spalte: {row}");
    }

    // ---- §18.5 ----

    #[test]
    fn findings_count_like_wb_check_from_warning_level() {
        let f = fx();
        let mut long = String::from("# Demo Alpha\n\n## Stand\n\n");
        for i in 0..110 {
            long.push_str(&format!("Zeile {i}.\n"));
        }
        write(&f.vault, "Projects/Demo Alpha/00 Overview.md", "overview", &long);
        // zwei Notizen gleichen Namens in verschiedenen Projekten + mehrdeutiger Link
        write(&f.vault, "Projects/Demo Alpha/Gemeinsam.md", "topic", "# Gemeinsam\n\nSiehe [[Doppelt]].\n");
        write(&f.vault, "Projects/Demo Alpha/Doppelt.md", "topic", "# Doppelt\n\nA.\n");
        write(&f.vault, "Projects/Demo Beta/00 Overview.md", "overview", "# Demo Beta\n\nB.\n");
        write(&f.vault, "Projects/Demo Beta/Doppelt.md", "topic", "# Doppelt\n\nB.\n");
        let v = open(&f);
        let src = CoreSource::with_today(&v, today());
        let all = v.check(&CheckOptions { prefix: None, today: Some("2026-11-10".into()), stale_days: Some(60) }).unwrap();
        let warn_or_worse = |p: &str| {
            all.iter().filter(|x| x.path.as_str().starts_with(&format!("Projects/{p}/")) && x.severity != wb_core::Severity::Info).count() as u32
        };
        let alpha = src.projects().into_iter().find(|p| p.name == "Demo Alpha").unwrap();
        // zu lange Übersicht zählt unter G, der mehrdeutige Link ist eine Art ohne eigene Spalte, die Waisen unter W
        assert_eq!((alpha.findings.oversize, alpha.findings.other, alpha.findings.orphans), (1, 1, 2), "{:?}", alpha.findings);
        // jeder Befund ab Warnung, den `wb check` meldet, steckt in der Summe
        assert_eq!(warn_or_worse("Demo Alpha"), 2);
        assert!(alpha.findings.total() >= warn_or_worse("Demo Alpha"));
        let beta = src.projects().into_iter().find(|p| p.name == "Demo Beta").unwrap();
        assert_eq!((beta.findings.total(), beta.findings.other), (1, 0), "nur die Waise: {:?}", beta.findings);

        // Kopfsatz nennt die Projekte mit Befund, Detail und Projektkarte die Zahl der anderen Arten
        let mut app = App::new(&src, Theme::mono());
        let t = text_of(&app, 160, 45);
        let bad = src.projects().iter().filter(|p| p.findings.total() > 0).count();
        assert!(t.lines().nth(1).unwrap().contains(&format!("Befunde in {bad}")), "{}", t.lines().nth(1).unwrap());
        if alpha.findings.other > 0 {
            let sel = t.lines().find(|l| l.contains('▸')).unwrap_or_else(|| panic!("keine Auswahl:\n{t}"));
            assert!(sel.contains("Befunde") && sel.contains("andere"), "{sel}");
            app.press("Enter");
            let p = text_of(&app, 160, 45);
            assert!(p.lines().nth(1).unwrap().contains("andere"), "Projektkarte: {}", p.lines().nth(1).unwrap());
        }
    }

    // ---- §18.6 ----

    #[test]
    fn modified_time_is_converted_with_the_rule_of_its_own_date() {
        // Reine Regel (unabhängig von der Zone des Rechners): 2026-08-01 00:30 MESZ bleibt der 1. August
        use wb_tui::localtime::TzRule;
        let utc = wb_tui::Date::parse("2026-07-31").unwrap().0 as i64 * 86_400 + 22 * 3600 + 30 * 60;
        assert_eq!(TzRule::CENTRAL_EUROPE.day_of(utc), wb_tui::Date::parse("2026-08-01").unwrap());
    }
}
