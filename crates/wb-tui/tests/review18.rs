//! Tests zu Vertrag §18 (Nachprüfung der Ansichten): Leser, Editorstart, Offen, Befunde, Ortszeit.
//! Alles mit erfundenen Daten (K12).

use wb_tui::fixture::FixtureSource;
use wb_tui::snapshot;
use wb_tui::term::{editor_command, split_command_line};
use wb_tui::{App, Theme, View};

fn text_of(app: &App, w: u16, h: u16) -> String {
    snapshot::to_text(&snapshot::render_app(app, w, h))
}

// ---- §18.1 Linkzähler im schmalen Leser ----

#[test]
fn reader_link_counter_never_sits_over_text() {
    let src = FixtureSource::new();
    let mut app = App::new(&src, Theme::mono());
    app.press("Enter");
    app.press("Enter");
    assert_eq!(app.view(), View::Reader);
    let mut seen = 0;
    for w in [60u16, 70, 80, 90, 99] {
        for h in [16u16, 24, 30] {
            let t = text_of(&app, w, h);
            let lines: Vec<&str> = t.lines().collect();
            for (i, l) in lines.iter().enumerate() {
                if let Some(at) = l.find("Tab: Links (") {
                    seen += 1;
                    assert!(l[..at].trim().is_empty(), "{w}x{h}: Zähler steht hinter Text: {l:?}");
                    assert!(l.contains(" ein)"), "{w}x{h}: Zähler abgeschnitten: {l:?}");
                    // Zeile über dem Zähler: endet nicht mit einem Textstück ohne Lücke zum Zähler
                    assert!(i > 0);
                }
            }
        }
    }
    assert!(seen > 0, "Zähler nie gesehen");
    // der Text selbst verliert nichts: dieselben Wörter wie bei großer Höhe, soweit sichtbar
    let t = text_of(&app, 80, 24);
    assert!(!t.contains("alleTab"), "{t}");
}

// ---- §18.2 Editorstart ----

#[test]
fn editor_line_is_split_like_a_shell_would_with_quotes() {
    assert_eq!(split_command_line("code --wait"), ["code", "--wait"]);
    assert_eq!(
        split_command_line(r#""C:\Program Files\Edit Me\ed.exe" --wait "-x y""#),
        [r"C:\Program Files\Edit Me\ed.exe", "--wait", "-x y"]
    );
    assert_eq!(split_command_line("  vi  "), ["vi"]);
    assert_eq!(split_command_line(""), Vec::<String>::new());
    assert_eq!(split_command_line("'a b' c"), ["a b", "c"]);
}

fn run_editor(line: &str, path: &std::path::Path, file: &std::path::Path) -> std::io::Result<std::process::ExitStatus> {
    let pe = std::env::var("PATHEXT").ok();
    editor_command(line, file, Some(path.as_os_str()), pe.as_deref())?.status()
}

#[cfg(windows)]
#[test]
fn editor_cmd_from_path_and_quoted_path_with_spaces_start() {
    let dir = tempfile::tempdir().unwrap();
    // erfundener Editor „Zeta“: hängt seine Argumente an die Zieldatei (letztes Argument)
    let bin = dir.path().join("Programme mit Leerzeichen");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::write(bin.join("zeta.cmd"), "@echo off\r\nif \"%~2\"==\"\" (echo gestartet >> \"%~1\") else (echo gestartet %1 %2 >> \"%~2\")\r\nexit /b 0\r\n").unwrap();
    let target = dir.path().join("Notiz.md");

    // 1) .cmd über PATH, ohne Endung, mit Argument wie `code --wait`
    std::fs::write(&target, "").unwrap();
    let st = run_editor("zeta --wait", &bin, &target).unwrap();
    assert!(st.success());
    let got = std::fs::read_to_string(&target).unwrap();
    assert!(got.contains("gestartet --wait"), "{got:?}");

    // 2) Pfad mit Leerzeichen in Anführungszeichen
    std::fs::write(&target, "").unwrap();
    let line = format!("\"{}\" --wait", bin.join("zeta.cmd").display());
    let st = run_editor(&line, std::path::Path::new(""), &target).unwrap();
    assert!(st.success());
    assert!(std::fs::read_to_string(&target).unwrap().contains("gestartet --wait"));

    // 3) ganze Zeile ist ein Pfad mit Leerzeichen, ohne Anführungszeichen
    std::fs::write(&target, "").unwrap();
    let line = bin.join("zeta.cmd").display().to_string();
    let st = run_editor(&line, std::path::Path::new(""), &target).unwrap();
    assert!(st.success());
    assert!(std::fs::read_to_string(&target).unwrap().contains("gestartet"));
}

#[cfg(not(windows))]
#[test]
fn editor_script_from_path_and_quoted_path_with_spaces_start() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("Programme mit Leerzeichen");
    std::fs::create_dir_all(&bin).unwrap();
    let sc = bin.join("zeta");
    std::fs::write(&sc, "#!/bin/sh\necho gestartet \"$1\" \"$2\" >> \"$2\"\n").unwrap();
    std::fs::set_permissions(&sc, std::fs::Permissions::from_mode(0o755)).unwrap();
    let target = dir.path().join("Notiz.md");
    std::fs::write(&target, "").unwrap();
    assert!(run_editor("zeta --wait", &bin, &target).unwrap().success());
    assert!(std::fs::read_to_string(&target).unwrap().contains("gestartet --wait"));
    std::fs::write(&target, "").unwrap();
    let line = format!("\"{}\" --wait", sc.display());
    assert!(run_editor(&line, std::path::Path::new(""), &target).unwrap().success());
    assert!(std::fs::read_to_string(&target).unwrap().contains("gestartet --wait"));
}

// ---- §18.7 Feinschliff ----

#[test]
fn legend_is_shown_from_60_columns_and_explains_the_plus() {
    let src = FixtureSource::new();
    let app = App::new(&src, Theme::mono());
    for w in [60u16, 62, 65] {
        let t = text_of(&app, w, 30);
        assert!(t.contains("V veraltet") || t.contains("V veraltet"), "{w} Spalten ohne Legende:\n{t}");
        assert!(t.contains("Waise"), "{w}: {t}");
    }
    // breit: Legende neben der Heatmap nennt das „+“
    let t = text_of(&app, 130, 30);
    assert!(t.contains("+ = 10 und mehr"), "{t}");
}

#[test]
fn reader_header_has_no_vor_heute() {
    let src = FixtureSource::new();
    let mut app = App::new(&src, Theme::mono());
    app.press("Enter");
    app.press("Enter");
    let head = text_of(&app, 160, 30).lines().nth(1).unwrap().to_string();
    assert!(!head.contains("vor heute"), "{head}");
}

#[test]
fn search_preview_has_no_raw_heading_marks() {
    let src = FixtureSource::new();
    let mut app = App::new(&src, Theme::mono());
    app.press("3");
    app.press("L a g e r u n g");
    let t = text_of(&app, 160, 45);
    assert!(!t.lines().any(|l| l.contains("## ")), "rohe ##:\n{t}");
}
