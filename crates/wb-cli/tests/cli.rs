//! Ende-zu-Ende-Tests der Binärdatei `wb` mit synthetischem Vault (zur Testzeit erzeugt).

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

/// Vault und Index über die Umgebung, damit `more:`-Zeilen keine Testpfade tragen.
fn wb(vault: &Path, index: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_wb"))
        .args(args)
        .env("WB_VAULT", vault)
        .env("WB_INDEX", index)
        .env_remove("WB_READONLY")
        .env_remove("WB_NO_SYNC")
        .output()
        .expect("wb starten")
}

fn fixture() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let vault = tmp.path().join("vault");
    let index = tmp.path().join("index");
    let w = |rel: &str, s: &str| {
        let p = vault.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, s).unwrap();
    };
    w(
        "Projects/Demo/00 Overview.md",
        "---\ntype: overview\nstatus: active\npaths: [\"C:/Arbeit/Demo\"]\n---\n# Demo\n\n## Stand\n\nDer Prüfstand läuft.\n\n## Offene Punkte\n\n- Messung wiederholen\n\n## Themen\n\n- [[Messung|Messreihe am Prüfstand]]\n",
    );
    w("Projects/Demo/Messung.md", "---\ntype: topic\nchecked_on: 2026-11-01\n---\n# Messung\n\n## Stand\n\nDie Größe der Messreihe wächst. Ein zweiter Satz folgt.\n\n## Belege\n\n- Lauf 1 ok.\n");
    w("Projects/Demo/A/Gleich.md", "# Gleich A
");
    w("Projects/Demo/B/Gleich.md", "# Gleich B
");
    for i in 0..300 {
        w(&format!("Projects/Demo/Viele/Notiz {i:03}.md"), &format!("---\ntype: topic\n---\n# Notiz {i}\n\nFüllstoff {i} Okapi.\n"));
    }
    (tmp, vault, index)
}

fn stdout(o: &Output) -> String {
    String::from_utf8(o.stdout.clone()).expect("stdout ist UTF-8")
}

#[test]
fn two_processes_sync_concurrently() {
    let (_t, vault, index) = fixture();
    let spawn = || {
        Command::new(env!("CARGO_BIN_EXE_wb"))
            .arg("--vault")
            .arg(&vault)
            .arg("--index")
            .arg(&index)
            .args(["sync"])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap()
    };
    let (a, b) = (spawn(), spawn());
    let (a, b) = (a.wait_with_output().unwrap(), b.wait_with_output().unwrap());
    assert!(a.status.success(), "a: {}", String::from_utf8_lossy(&a.stderr));
    assert!(b.status.success(), "b: {}", String::from_utf8_lossy(&b.stderr));
    let o = wb(&vault, &index, &["search", "Okapi", "--limit", "1"]);
    assert!(stdout(&o).contains("hits=1-1/300"), "{}", stdout(&o));
    assert_eq!(stdout(&wb(&vault, &index, &["sync"])).trim(), "sync scanned=304 added=0 changed=0 removed=0 touched=0 full=false errors=0");
}

#[test]
fn output_is_byte_identical_and_ascii_json() {
    let (_t, vault, index) = fixture();
    for args in [
        &["search", "Prüfstand", "Größe"][..],
        &["read", "Messung#Stand"],
        &["read", "Messung", "--outline"],
        &["context", "demo"],
        &["context", "--cwd", "C:/Arbeit/Demo/src"],
        &["check"],
        &["stats"],
        &["stats", "--json"],
        &["search", "Messreihe", "--json"],
    ] {
        let a = wb(&vault, &index, args);
        let b = wb(&vault, &index, args);
        assert!(a.status.success(), "{args:?}: {}", String::from_utf8_lossy(&a.stderr));
        assert_eq!(a.stdout, b.stdout, "{args:?}");
        assert!(!a.stdout.contains(&b'\r'), "nur LF: {args:?}");
        assert!(!a.stdout.contains(&0x1b), "keine ANSI-Farben: {args:?}");
    }
    let j = wb(&vault, &index, &["search", "Größe", "--json"]);
    assert!(j.stdout.is_ascii(), "JSON ist reines ASCII");
    let v: serde_json::Value = serde_json::from_slice(&j.stdout).unwrap();
    assert_eq!(v["query"], "Größe");
    let c = stdout(&wb(&vault, &index, &["context", "demo", "--budget", "50000"]));
    assert!(c.contains("Messreihe am Prüfstand"), "{c}");
    assert!(c.starts_with("project=Demo "), "{c}");
}

#[test]
fn exit_codes_and_errors() {
    let (_t, vault, index) = fixture();
    let o = wb(&vault, &index, &["search", "gibtesnichtxyz"]);
    assert_eq!(o.status.code(), Some(1));
    let o = wb(&vault, &index, &["read", "Gleich"]);
    assert_eq!(o.status.code(), Some(2), "mehrdeutig");
    assert!(String::from_utf8_lossy(&o.stderr).starts_with("error[ambiguous]"));
    let o = wb(&vault, &index, &["read", "Fehlt ganz"]);
    assert_eq!(o.status.code(), Some(1));
    // Nur-Lese mit Index im Vault: verweigert, nichts angelegt
    let o = Command::new(env!("CARGO_BIN_EXE_wb")).arg("--vault").arg(&vault).args(["--read-only", "search", "x"]).output().unwrap();
    assert_eq!(o.status.code(), Some(4), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(String::from_utf8_lossy(&o.stderr).contains("\nhint: --index"), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(!vault.join(".wb").exists());
}

#[test]
fn context_without_target_lists_projects_and_search_marks_superseded() {
    let (t, vault, index) = fixture();
    fs::write(vault.join("Projects/Demo/Alt.md"), "---\ntype: topic\nstatus: superseded\n---\n# Alt\n\nOkapi früher.\n").unwrap();
    let run = |dir: &Path, args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_wb"))
            .current_dir(dir)
            .args(args)
            .env("WB_VAULT", &vault)
            .env("WB_INDEX", &index)
            .output()
            .unwrap()
    };
    let o = run(t.path(), &["context"]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let s = stdout(&o);
    assert!(s.starts_with("projects=1 "), "{s}");
    assert!(s.contains("\n  Demo 305 ~"), "{s}");
    assert!(s.ends_with("note: `wb context <projekt>` öffnet ein Projekt\n"), "{s}");
    // ausdrücklich angegebener Ordner ohne Treffer bleibt ein Fehler
    assert_eq!(run(t.path(), &["context", "--cwd", "C:/nirgends"]).status.code(), Some(1));
    // Projekt: überholte nur als Zahl
    let s = stdout(&run(t.path(), &["context", "Demo", "--budget", "50000"]));
    assert!(s.contains("  superseded: 1 (wb search -p Demo -s superseded <begriffe>)"), "{s}");
    assert!(!s.contains("  Alt "), "{s}");
    // Suche: Markierung und Filter
    let s = stdout(&run(t.path(), &["search", "Okapi", "früher", "--limit", "1"]));
    assert!(s.lines().nth(1).is_some_and(|l| l.contains(" superseded")), "{s}");
    let o = run(t.path(), &["search", "Okapi", "-s", "superseded"]);
    assert!(stdout(&o).contains("hits=1-1/1"), "{}", stdout(&o));
    assert_eq!(run(t.path(), &["search", "Okapi", "-s", "veraltet"]).status.code(), Some(2));
}

fn snapshot(root: &Path) -> std::collections::BTreeMap<String, Vec<u8>> {
    let mut out = std::collections::BTreeMap::new();
    fn walk(root: &Path, dir: &Path, out: &mut std::collections::BTreeMap<String, Vec<u8>>) {
        for e in fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            let rel = p.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/");
            if p.is_dir() {
                out.insert(rel + "/", Vec::new());
                walk(root, &p, out);
            } else if rel == ".wb-lock" {
                // Sperrdatei: immer leer, und während ein Test sie hält, nicht lesbar.
                out.insert(rel, Vec::new());
            } else {
                out.insert(rel, fs::read(&p).unwrap());
            }
        }
    }
    walk(root, root, &mut out);
    out
}

#[test]
fn write_commands_new_log_mv() {
    let (_t, vault, index) = fixture();
    let o = wb(&vault, &index, &["new", "Größe neu", "-p", "demo", "--date", "2026-11-09"]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(
        stdout(&o),
        "created Projects/Demo/Größe neu.md\nlink: [[Projects/Demo/Größe neu]]\nlinked: Projects/Demo/00 Overview.md L19\n"
    );
    assert_eq!(wb(&vault, &index, &["new", "größe NEU", "-p", "Demo"]).status.code(), Some(2), "Variante eines vorhandenen Namens");
    // §23.1: Beschreibung als Linktext; mit --no-link bleibt die Übersicht, der Eintrag steht in `next:`
    let o = wb(&vault, &index, &["new", "Zweite", "-p", "demo", "--summary", "Wozu die Notiz da ist"]);
    assert_eq!(stdout(&o), "created Projects/Demo/Zweite.md\nlink: [[Projects/Demo/Zweite]]\nlinked: Projects/Demo/00 Overview.md L20\n", "{}", String::from_utf8_lossy(&o.stderr));
    let o = wb(&vault, &index, &["new", "Dritte", "-p", "demo", "--summary", "Kurz", "--no-link"]);
    assert_eq!(
        stdout(&o),
        "created Projects/Demo/Dritte.md\nlink: [[Projects/Demo/Dritte]]\nnext: in Projects/Demo/00 Overview.md unter Themen verlinken: - [[Projects/Demo/Dritte|Kurz]]\n"
    );
    assert!(fs::read_to_string(vault.join("Projects/Demo/00 Overview.md")).unwrap().ends_with(
        "## Themen\n\n- [[Messung|Messreihe am Prüfstand]]\n- [[Projects/Demo/Größe neu]]\n- [[Projects/Demo/Zweite|Wozu die Notiz da ist]]\n"
    ));
    let c = stdout(&wb(&vault, &index, &["context", "demo", "--budget", "50000"]));
    assert!(c.contains("Wozu die Notiz da ist"), "{c}");
    let o = wb(&vault, &index, &["new", "Eine Quelle", "-t", "source", "--summary", "x"]);
    assert_eq!(o.status.code(), Some(2), "--summary nur für Themen");
    let o = wb(&vault, &index, &["new", "Vierte", "-p", "demo", "--json"]);
    let j: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!((j["linked_line"].as_u64(), j["entry"].as_str()), (Some(21), Some("- [[Projects/Demo/Vierte]]")), "{j}");
    let o = wb(&vault, &index, &["log", "-n", "Messung", "--date", "2026-11-09", "--time", "08:15", "Lauf", "zwei"]);
    assert_eq!(stdout(&o), "logged Journal/2026-11.md L1\n  - 2026-11-09 08:15 [[Projects/Demo/Messung]] Lauf zwei\n", "{}", String::from_utf8_lossy(&o.stderr));
    let o = wb(&vault, &index, &["mv", "Messung", "Messung alt"]);
    assert_eq!(
        stdout(&o),
        "moved Projects/Demo/Messung.md -> Projects/Demo/Messung alt.md links=2 notes=2\n  Journal/2026-11.md 1\n  Projects/Demo/00 Overview.md 1\nnext: wb read 'Projects/Demo/Messung alt'\n",
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );
    assert!(fs::read_to_string(vault.join("Projects/Demo/00 Overview.md")).unwrap().contains("- [[Projects/Demo/Messung alt|Messreihe am Prüfstand]]\n"));
    let o = wb(&vault, &index, &["check", "--json"]);
    assert!(!stdout(&o).contains("link-dead"), "{}", stdout(&o));
    let o = wb(&vault, &index, &["mv", "Messung alt", "Messung", "--json"]);
    assert!(stdout(&o).starts_with("{
  \"from\": \"Projects/Demo/Messung alt.md\",
  \"to\": \"Projects/Demo/Messung.md\","), "{}", stdout(&o));
    assert!(stdout(&o).is_ascii());
}

#[test]
fn two_processes_log_concurrently_lose_no_line() {
    let (_t, vault, index) = fixture();
    assert!(wb(&vault, &index, &["sync"]).status.success());
    let spawn = |who: usize| {
        let (vault, index) = (vault.clone(), index.clone());
        std::thread::spawn(move || {
            for i in 0..40 {
                let o = wb(&vault, &index, &["log", "--date", "2026-12-02", &format!("Prozess {who} Zeile {i:02}")]);
                assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
            }
        })
    };
    let (a, b) = (spawn(1), spawn(2));
    a.join().unwrap();
    b.join().unwrap();
    let text = fs::read_to_string(vault.join("Journal/2026-12.md")).unwrap();
    let lines: std::collections::BTreeSet<&str> = text.lines().collect();
    assert_eq!(text.lines().count(), 80);
    assert_eq!(lines.len(), 80);
    for who in [1, 2] {
        for i in 0..40 {
            assert!(lines.contains(format!("- 2026-12-02 Prozess {who} Zeile {i:02}").as_str()));
        }
    }
}

#[test]
fn interrupted_mv_is_completed_on_next_start() {
    let (_t, clean, clean_index) = fixture();
    let (_u, vault, index) = fixture();
    assert!(wb(&clean, &clean_index, &["mv", "Messung", "Neu/Größe Messung"]).status.success());
    assert!(wb(&vault, &index, &["sync"]).status.success());
    let o = Command::new(env!("CARGO_BIN_EXE_wb"))
        .arg("--vault")
        .arg(&vault)
        .arg("--index")
        .arg(&index)
        .args(["mv", "Messung", "Neu/Größe Messung"])
        .env("WB_FAULT_MV", "crash:1")
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(99), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(vault.join(".wb-mv-pending").exists(), "Ausführungsliste liegt nach dem Abbruch");
    assert_ne!(snapshot(&vault), snapshot(&clean), "Abbruch mitten im Umschreiben");
    // Nur-Lese-Aufruf rührt nichts an
    let before = snapshot(&vault);
    assert!(wb(&vault, &index, &["--read-only", "check"]).status.code().is_some());
    assert_eq!(snapshot(&vault), before);
    // nächster schreibfähiger Start führt zu Ende: gleicher Endzustand wie ohne Abbruch
    let o = wb(&vault, &index, &["check"]);
    assert!(o.status.code().is_some_and(|c| c < 3), "{}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(snapshot(&vault), snapshot(&clean));
    assert!(!stdout(&wb(&vault, &index, &["check"])).contains("link-dead"));
}

#[test]
fn write_commands_refuse_read_only_with_exit_4() {
    let (_t, vault, index) = fixture();
    assert!(wb(&vault, &index, &["sync"]).status.success());
    let before = snapshot(&vault);
    for args in [&["new", "Neu", "-p", "Demo"][..], &["log", "Text"], &["mv", "Messung", "Anders"]] {
        let mut a = vec!["--read-only"];
        a.extend_from_slice(args);
        let o = wb(&vault, &index, &a);
        assert_eq!(o.status.code(), Some(4), "{args:?}: {}", String::from_utf8_lossy(&o.stderr));
        assert!(stdout(&o).is_empty());
        let o = Command::new(env!("CARGO_BIN_EXE_wb")).arg("--vault").arg(&vault).arg("--index").arg(&index).args(args).env("WB_READONLY", "1").output().unwrap();
        assert_eq!(o.status.code(), Some(4), "WB_READONLY {args:?}");
    }
    assert_eq!(snapshot(&vault), before);
}

// ---------------- §19: wb mv abnahmereif ----------------

fn wb_env(vault: &Path, index: &Path, args: &[&str], envs: &[(&str, &str)]) -> Output {
    let mut c = Command::new(env!("CARGO_BIN_EXE_wb"));
    c.args(args).env("WB_VAULT", vault).env("WB_INDEX", index).env_remove("WB_READONLY").env_remove("WB_NO_SYNC").env_remove("WB_FAULT_MV");
    for (k, v) in envs {
        c.env(k, v);
    }
    c.output().expect("wb starten")
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

/// Notizen und Ordner ohne die Verwaltungsdateien der Schreibbefehle (`.wb-lock`, `.wb-tmp/`).
fn notes_snapshot(root: &Path) -> std::collections::BTreeMap<String, Vec<u8>> {
    snapshot(root).into_iter().filter(|(k, _)| k != ".wb-lock" && !k.starts_with(".wb-tmp/")).collect()
}

/// Fixture mit Journalzeile auf `Messung`, damit ein `mv` vier Schritte hat
/// (Ordner anlegen, Übersicht, Journal, Umbenennen).
fn mv_fixture() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let (t, vault, index) = fixture();
    fs::write(vault.join("Projects/Demo/Thema Zwei.md"), "---\ntype: topic\n---\n# Thema Zwei\n\nNoch ein Thema.\n").unwrap();
    let o = wb(&vault, &index, &["log", "-n", "Projects/Demo/Messung", "--date", "2026-11-09", "Lauf", "eins"]);
    assert!(o.status.success(), "{}", stderr(&o));
    (t, vault, index)
}

#[test]
fn mv_predictable_failures_exit_2_and_create_nothing() {
    let (_t, vault, index) = mv_fixture();
    let before = snapshot(&vault);
    let long_name = "x".repeat(256);
    let long_path = format!("Projects/Demo/{}/{}", "a".repeat(150), "b".repeat(120));
    for target in ["Projects/Demo/Thema Zwei.md/Neu", long_name.as_str(), long_path.as_str(), "", "  ", "/"] {
        let o = wb(&vault, &index, &["mv", "Projects/Demo/Messung", target]);
        assert_eq!(o.status.code(), Some(2), "{target:?}: {}", stderr(&o));
        assert!(stdout(&o).is_empty(), "{target:?}");
        assert_eq!(snapshot(&vault), before, "{target:?}: nichts angelegt, auch keine Sperre oder Liste");
    }
    let o = wb(&vault, &index, &["mv", "Projects/Demo/Messung", "Projects/Demo/Thema Zwei.md/Neu"]);
    assert!(stderr(&o).contains("Zielordner Projects/Demo/Thema Zwei.md gibt es schon als Datei"), "{}", stderr(&o));
    let o = wb(&vault, &index, &["mv", "Projects/Demo/Messung", &long_name]);
    assert!(stderr(&o).contains("Dateiname zu lang"), "{}", stderr(&o));
    // `wb new` prüft genauso
    let o = wb(&vault, &index, &["new", &long_name, "-p", "Demo"]);
    assert_eq!(o.status.code(), Some(2), "{}", stderr(&o));
    assert_eq!(snapshot(&vault), before);
}

#[test]
fn mv_failed_step_is_rolled_back_completely() {
    let (_t, vault, index) = mv_fixture();
    let before = notes_snapshot(&vault);
    for n in 0..4 {
        let spec = format!("fail:{n}");
        let o = wb_env(&vault, &index, &["mv", "Projects/Demo/Messung", "Neu/Größe Messung"], &[("WB_FAULT_MV", &spec)]);
        assert_eq!(o.status.code(), Some(3), "{spec}: {}", stderr(&o));
        assert!(stderr(&o).contains("vollständig zurückgenommen"), "{spec}: {}", stderr(&o));
        assert!(!vault.join(".wb-mv-pending").exists(), "{spec}: Liste entfernt");
        assert_eq!(notes_snapshot(&vault), before, "{spec}: alles wie vorher");
        assert_eq!(fs::read_dir(vault.join(".wb-tmp")).map(|d| d.count()).unwrap_or(0), 0, "{spec}: keine Reste");
    }
    // Danach gelingt derselbe Umzug.
    let o = wb(&vault, &index, &["mv", "Projects/Demo/Messung", "Neu/Größe Messung"]);
    assert!(o.status.success(), "{}", stderr(&o));
    assert!(vault.join("Neu/Größe Messung.md").exists());
}

/// Echter Fehler beim letzten Schritt: ein anderes Programm hält die Notiz ohne Löschfreigabe offen.
#[cfg(windows)]
#[test]
fn mv_with_note_held_open_is_rolled_back() {
    use std::os::windows::fs::OpenOptionsExt;
    let (_t, vault, index) = mv_fixture();
    let before = notes_snapshot(&vault);
    let held = fs::OpenOptions::new().read(true).share_mode(1 /* FILE_SHARE_READ */).open(vault.join("Projects/Demo/Messung.md")).unwrap();
    let o = wb(&vault, &index, &["mv", "Projects/Demo/Messung", "Neu/Größe Messung"]);
    drop(held);
    assert_eq!(o.status.code(), Some(3), "{}", stderr(&o));
    assert!(stderr(&o).contains("vollständig zurückgenommen"), "{}", stderr(&o));
    assert!(!vault.join(".wb-mv-pending").exists());
    assert_eq!(notes_snapshot(&vault), before, "Übersicht und Journal zurückgeschrieben, Ordner Neu entfernt");
}

#[test]
fn mv_stuck_keeps_reads_running_and_write_errors_name_the_cure() {
    let (_t, vault, index) = mv_fixture();
    let before = notes_snapshot(&vault);
    let stuck = [("WB_FAULT_MV", "stuck:2")];
    let o = wb_env(&vault, &index, &["mv", "Projects/Demo/Messung", "Neu/Größe Messung"], &stuck);
    assert_eq!(o.status.code(), Some(3), "{}", stderr(&o));
    assert!(stderr(&o).contains(".wb-mv-pending") && stderr(&o).contains("wb sync"), "{}", stderr(&o));
    assert!(vault.join(".wb-mv-pending").exists(), "Liste bleibt, solange nichts heilt");
    // Lesebefehle laufen weiter, mit einer note:-Zeile.
    for args in [&["search", "Okapi", "--limit", "1"][..], &["read", "Projects/Demo/00 Overview"], &["context", "Demo"], &["check", "Projects/Demo/A"]] {
        let o = wb_env(&vault, &index, args, &stuck);
        assert_eq!(o.status.code(), Some(0), "{args:?}: {}", stderr(&o));
        assert!(stderr(&o).contains("note: unterbrochenes wb mv hängt") && stderr(&o).contains(".wb-mv-pending"), "{args:?}: {}", stderr(&o));
    }
    // Schreibbefehle verweigern mit Datei und heilendem Befehl.
    for args in [&["new", "Noch neu", "-p", "Demo"][..], &["log", "Text"], &["mv", "Projects/Demo/Thema Zwei", "Thema Drei"]] {
        let o = wb_env(&vault, &index, args, &stuck);
        assert_eq!(o.status.code(), Some(3), "{args:?}: {}", stderr(&o));
        let e = stderr(&o);
        assert!(e.contains(".wb-mv-pending") && e.contains("heilt `wb sync`"), "{args:?}: {e}");
    }
    // Ursache behoben: `wb sync` nimmt den Umzug zurück.
    let o = wb(&vault, &index, &["sync"]);
    assert!(o.status.success(), "{}", stderr(&o));
    assert!(!stderr(&o).contains("note:"), "{}", stderr(&o));
    assert!(!vault.join(".wb-mv-pending").exists());
    assert_eq!(notes_snapshot(&vault), before);
    assert!(wb(&vault, &index, &["mv", "Projects/Demo/Messung", "Neu/Größe Messung"]).status.success());
}

#[test]
fn three_concurrent_mv_of_one_note_make_exactly_one_target() {
    for round in 0..2 {
        let (_t, vault, index) = mv_fixture();
        assert!(wb(&vault, &index, &["sync"]).status.success());
        let targets = ["Ziel A", "Ziel B", "Ziel C"];
        let children: Vec<_> = targets
            .iter()
            .map(|t| {
                Command::new(env!("CARGO_BIN_EXE_wb"))
                    .args(["mv", "Projects/Demo/Messung", t])
                    .env("WB_VAULT", &vault)
                    .env("WB_INDEX", &index)
                    .env_remove("WB_FAULT_MV")
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped())
                    .spawn()
                    .unwrap()
            })
            .collect();
        let outs: Vec<Output> = children.into_iter().map(|c| c.wait_with_output().unwrap()).collect();
        let ok = outs.iter().filter(|o| o.status.success()).count();
        assert_eq!(ok, 1, "Runde {round}: genau ein mv gelingt: {:?}", outs.iter().map(stderr).collect::<Vec<_>>());
        for o in outs.iter().filter(|o| !o.status.success()) {
            assert!(matches!(o.status.code(), Some(1) | Some(3)), "Runde {round}: {}", stderr(o));
        }
        let present: Vec<_> = targets.iter().filter(|t| vault.join(format!("Projects/Demo/{t}.md")).exists()).collect();
        assert_eq!(present.len(), 1, "Runde {round}: genau eine Zielnotiz");
        assert!(!vault.join("Projects/Demo/Messung.md").exists());
        assert!(!vault.join(".wb-mv-pending").exists());
        let overview = fs::read_to_string(vault.join("Projects/Demo/00 Overview.md")).unwrap();
        assert!(overview.contains(&format!("[[Projects/Demo/{}|Messreihe am Prüfstand]]", present[0])), "{overview}");
        assert!(!stdout(&wb(&vault, &index, &["check"])).contains("link-dead"));
    }
}

#[test]
fn write_command_waits_for_lock_then_exit_3_without_change() {
    let (_t, vault, index) = mv_fixture();
    let lock = fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(vault.join(".wb-lock")).unwrap();
    lock.lock().unwrap();
    let before = snapshot(&vault);
    let wait = [("WB_LOCK_WAIT_MS", "300")];
    for args in [&["mv", "Projects/Demo/Messung", "Neu/Größe Messung"][..], &["log", "Text"], &["new", "Noch neu", "-p", "Demo"]] {
        let t0 = std::time::Instant::now();
        let o = wb_env(&vault, &index, args, &wait);
        assert_eq!(o.status.code(), Some(3), "{args:?}: {}", stderr(&o));
        assert!(t0.elapsed() >= std::time::Duration::from_millis(300), "wartet kurz");
        assert!(stderr(&o).contains("ein anderer Schreibbefehl läuft") && stderr(&o).contains("nichts wurde geändert"), "{}", stderr(&o));
        assert_eq!(snapshot(&vault), before, "{args:?}");
    }
    drop(lock);
    assert!(wb(&vault, &index, &["mv", "Projects/Demo/Messung", "Neu/Größe Messung"]).status.success());
}

#[test]
fn write_commands_need_full_path_or_full_name() {
    let (_t, vault, index) = mv_fixture();
    let before = snapshot(&vault);
    // Namensanfang reicht nicht: Exit 1 mit Vorschlag.
    let o = wb(&vault, &index, &["mv", "Mess", "Anders"]);
    assert_eq!(o.status.code(), Some(1), "{}", stderr(&o));
    assert!(stderr(&o).contains("Projects/Demo/Messung"), "Vorschlag: {}", stderr(&o));
    let o = wb(&vault, &index, &["log", "-n", "Mess", "Text"]);
    assert_eq!(o.status.code(), Some(1), "{}", stderr(&o));
    // Pfadende ist kein voller Pfad.
    assert_eq!(wb(&vault, &index, &["mv", "Demo/Messung", "Anders"]).status.code(), Some(1));
    // Journaldateien sind nicht umbenennbar.
    for j in ["Journal/2026-11", "Journal/2026-11.md", "2026-11"] {
        let o = wb(&vault, &index, &["mv", j, "Anders"]);
        assert_eq!(o.status.code(), Some(2), "{j}: {}", stderr(&o));
        assert!(stderr(&o).contains("Journaldateien lassen sich nicht umbenennen"), "{j}: {}", stderr(&o));
    }
    assert_eq!(snapshot(&vault), before);
    // Voller Name und voller Pfad (auch mit .md, `\` und anderer Schreibweise) gehen.
    assert!(wb(&vault, &index, &["log", "-n", "messung", "Text"]).status.success());
    let o = wb(&vault, &index, &["mv", "Messung", "Messung alt"]);
    assert!(o.status.success(), "{}", stderr(&o));
    let o = wb(&vault, &index, &["mv", "Projects\\Demo\\Messung alt.md", "Messung"]);
    assert!(o.status.success(), "{}", stderr(&o));
    // Gleicher Name in zwei Ordnern bleibt mehrdeutig.
    assert_eq!(wb(&vault, &index, &["mv", "Gleich", "Anders"]).status.code(), Some(2));
}

#[test]
fn check_json_pages_with_limit_offset_and_more() {
    let (_t, vault, index) = fixture();
    let all: serde_json::Value = serde_json::from_slice(&wb(&vault, &index, &["check", "--json", "--limit", "1000"]).stdout).unwrap();
    let total = all["total"].as_u64().unwrap() as usize;
    assert!(total > 3, "Fixture hat Befunde: {all}");
    assert_eq!(all["findings"].as_array().unwrap().len(), total);
    assert!(all.get("more").is_none());
    let o = wb(&vault, &index, &["check", "--json", "--limit", "2", "--offset", "1"]);
    assert!(o.status.success(), "{}", stderr(&o));
    let page: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(page["total"].as_u64(), Some(total as u64));
    assert_eq!(page["offset"].as_u64(), Some(1));
    assert_eq!(page["shown"].as_u64(), Some(2));
    assert_eq!(page["findings"].as_array().unwrap()[..], all["findings"].as_array().unwrap()[1..3]);
    let more = page["more"].as_str().unwrap();
    assert!(more.starts_with("wb ") && more.contains(" check") && more.contains("--json") && more.contains("--limit 2") && more.contains("--offset 3"), "{more}");
    // Hinter dem Ende: leere Seite, Exit 0, kein more.
    let o = wb(&vault, &index, &["check", "--json", "--offset", "100000"]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    let end: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(end["shown"].as_u64(), Some(0));
    assert!(end.get("more").is_none());
}

/// §23.2: Fehler vor Warnungen vor Hinweisen; Hinweise über den ganzen Vault gebündelt.
#[test]
fn check_lists_errors_first_and_bundles_info() {
    let (_t, vault, index) = fixture();
    // Fehler in einer Notiz, die nach dem Pfad hinter 300 Hinweisen läge
    fs::write(vault.join("Projects/Demo/Zz Tot.md"), "---\ntype: topic\n---\n# Tot\n\n[[Gibt es nicht]]\n").unwrap();
    let rank = |l: &str| if l.contains(" error ") { 0 } else if l.contains(" warn ") { 1 } else { 2 };
    let o = wb(&vault, &index, &["check"]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    let s = stdout(&o);
    let lines: Vec<&str> = s.lines().collect();
    assert!(lines[0].starts_with("findings=") && lines[0].contains(" link-dead=1 ") && lines[0].contains(" orphan="), "{s}");
    let dead = lines.iter().position(|l| l.starts_with("Projects/Demo/Zz Tot:6 error link-dead: ")).expect(&s);
    let single: Vec<&str> = lines[1..].iter().copied().take_while(|l| !l.starts_with("info ") && !l.starts_with("note: ")).collect();
    assert!(dead <= single.len(), "{s}");
    assert!(single.iter().all(|l| rank(l) < 2), "kein Hinweis einzeln: {s}");
    assert!(single.windows(2).all(|w| rank(w[0]) <= rank(w[1])), "nach Schwere: {s}");
    let orphan = lines.iter().find(|l| l.starts_with("info orphan ")).expect(&s);
    assert!(orphan.contains(": Projects/Demo/ ") && orphan.ends_with(" | hint: aus der Übersicht verlinken"), "{orphan}");
    assert!(s.ends_with("note: Hinweise (info) je Regel gebündelt; einzeln mit `wb check <Ordner>` oder `wb check --all`\n"), "{s}");
    assert!(lines.len() < 20, "kurz trotz {} Befunden: {s}", lines[0]);
    assert_eq!(wb(&vault, &index, &["check", "--strict"]).status.code(), Some(1));

    // --all und Pfadpräfix: jeder Befund einzeln, die Fortsetzung führt --all mit
    let all = stdout(&wb(&vault, &index, &["check", "--all"]));
    assert!(all.contains(" info orphan: ") && !all.contains("gebündelt"), "{all}");
    let listed: Vec<&str> = all.lines().skip(1).filter(|l| !l.starts_with("note: ") && !l.starts_with("more: ")).collect();
    assert_eq!(listed.len(), 50);
    assert!(listed.windows(2).all(|w| rank(w[0]) <= rank(w[1])), "{all}");
    let more = all.lines().find(|l| l.starts_with("more: ")).expect(&all);
    assert!(more.contains(" --all") && more.contains("--offset 50"), "{more}");
    let p = stdout(&wb(&vault, &index, &["check", "Projects/Demo/Viele"]));
    assert!(p.contains("Projects/Demo/Viele/Notiz 000:1 info orphan: ") && !p.contains("gebündelt"), "{p}");

    // JSON: vollständig, dieselbe Reihenfolge wie der Text
    let j: serde_json::Value = serde_json::from_slice(&wb(&vault, &index, &["check", "--json", "--limit", "1000"]).stdout).unwrap();
    let f = j["findings"].as_array().unwrap();
    assert_eq!(Some(f.len() as u64), j["total"].as_u64());
    let sev = |x: &serde_json::Value| ["error", "warning", "info"].iter().position(|s| x["severity"] == *s).unwrap();
    assert!(f.windows(2).all(|w| sev(&w[0]) <= sev(&w[1])), "{j}");
    assert!(f.iter().filter(|x| x["code"] == "orphan").count() > 300);
    for (l, x) in listed.iter().zip(f) {
        assert!(l.contains(&format!(" {}: ", x["code"].as_str().unwrap())), "{l} / {x}");
    }
}

#[test]
fn small_fixes_of_review_19_5() {
    let (_t, vault, index) = mv_fixture();
    // Tage, die es nicht gibt
    for d in ["2026-02-29", "2026-02-30", "2026-04-31", "2026-13-01"] {
        assert_eq!(wb(&vault, &index, &["log", "--date", d, "Text"]).status.code(), Some(2), "log {d}");
        assert_eq!(wb(&vault, &index, &["new", "Datum", "-p", "Demo", "--date", d]).status.code(), Some(2), "new {d}");
    }
    assert!(wb(&vault, &index, &["log", "--date", "2028-02-29", "Schalttag"]).status.success());
    // Steuerzeichen fallen beim Schreiben weg
    let o = wb(&vault, &index, &["log", "--date", "2026-11-10", "Rot\u{1b}[31m und \u{202e}verkehrt\u{2028}Ende"]);
    assert!(o.status.success(), "{}", stderr(&o));
    let j = fs::read_to_string(vault.join("Journal/2026-11.md")).unwrap();
    assert!(j.contains("- 2026-11-10 Rot[31m und verkehrt Ende\n"), "{j}");
    let o = wb(&vault, &index, &["new", "Kon\u{7}trolle\u{202e}", "-p", "Demo", "--date", "2026-11-10"]);
    assert!(o.status.success(), "{}", stderr(&o));
    assert!(vault.join("Projects/Demo/Kontrolle.md").exists());
    // Ausgabe ersetzt U+202E, U+2028, U+2029 sichtbar
    fs::write(vault.join("Projects/Demo/Zeichen.md"), "# Zeichen\n\nVor\u{202e}her und\u{2028}dann\u{2029}fertig.\n").unwrap();
    let s = stdout(&wb(&vault, &index, &["read", "Projects/Demo/Zeichen"]));
    assert!(!s.contains('\u{202e}') && !s.contains('\u{2028}') && !s.contains('\u{2029}'), "{s}");
    assert!(s.contains("Vor\u{fffd}her und\u{2424}dann\u{2424}fertig."), "{s}");
    // Verwaiste Zwischendateien räumt der nächste Schreibbefehl weg
    fs::create_dir_all(vault.join(".wb-tmp")).unwrap();
    fs::write(vault.join(".wb-tmp/mv-1-2-3"), "Rest").unwrap();
    fs::write(vault.join("Projects/Demo/.Messung.md.wbtmp-42"), "Rest").unwrap();
    assert!(wb(&vault, &index, &["log", "--date", "2026-11-10", "aufräumen"]).status.success());
    assert!(!vault.join(".wb-tmp/mv-1-2-3").exists());
    assert!(!vault.join("Projects/Demo/.Messung.md.wbtmp-42").exists());
    // Unbekannter Pfad bei check: Exit 1
    let o = wb(&vault, &index, &["check", "Projects/Gibtsnicht"]);
    assert_eq!(o.status.code(), Some(1), "{}", stderr(&o));
    // context unter dem Mindestbudget nennt die Anhebung, auch wenn alles passt
    let s = stdout(&wb(&vault, &index, &["context", "Demo", "--budget", "50"]));
    assert!(s.lines().next().unwrap().contains("budget=200(angehoben)"), "{s}");
    // read hält sich an das angehobene Budget, auch mit langem Abschnittstitel
    let long = "Ein sehr langer Abschnittstitel mit vielen Woertern, der kaum noch in eine Zeile passt und weitergeht";
    let body: String = (0..12).map(|i| format!("Satz {i} mit etwas Text, der erklaert, was hier steht. Noch ein Satz folgt.\n")).collect();
    fs::write(vault.join("Projects/Demo/Lang.md"), format!("# Lang\n\n## {long}\n\n{body}")).unwrap();
    for b in ["1", "60", "61", "80"] {
        let s = stdout(&wb(&vault, &index, &["read", &format!("Projects/Demo/Lang#{long}"), "--budget", b]));
        // Mindestens 60; reicht das nicht für Kopf und Abschnittstitel, nennt die Kopfzeile die Anhebung.
        let first = s.lines().next().unwrap();
        let raised = first.split(" budget=").nth(1).and_then(|x| x.strip_suffix("(angehoben)")).map(|x| x.parse::<usize>().unwrap());
        let limit = raised.unwrap_or_else(|| b.parse::<usize>().unwrap().max(60));
        assert!(b.parse::<usize>().unwrap() >= 60 || raised.is_some(), "{first}");
        let used: usize = s.lines().map(|l| (l.chars().count() * 2).div_ceil(7) + 1).sum();
        assert!(used <= limit, "--budget {b}: {used} > {limit}\n{s}");
    }
}

/// §20.1: ein Index im Vault, adressiert über die UNC-Freigabe desselben Laufwerks, ist im
/// Nur-Lese-Modus verweigert (Exit 4) und wird nicht angelegt. Überspringt sich ohne Freigabe.
#[cfg(windows)]
#[test]
fn read_only_refuses_index_inside_vault_over_unc() {
    let (_t, vault, _index) = fixture();
    let local = fs::canonicalize(&vault).unwrap();
    let plain = local.to_string_lossy().trim_start_matches(r"\\?\").to_string();
    let (drive, rest) = plain.split_once(':').unwrap();
    for host in ["localhost", "127.0.0.1"] {
        let unc_vault = std::path::PathBuf::from(format!(r"\\{host}\{drive}${rest}"));
        if fs::metadata(&unc_vault).is_err() {
            eprintln!("übersprungen: keine Freigabe {}", unc_vault.display());
            return;
        }
        let before = snapshot(&vault);
        for (v, i) in [(&vault, unc_vault.join(".idx")), (&unc_vault, vault.join(".idx")), (&unc_vault, unc_vault.join("sub/.idx"))] {
            let o = Command::new(env!("CARGO_BIN_EXE_wb")).arg("--vault").arg(v).arg("--index").arg(&i).args(["--read-only", "search", "Okapi"]).output().unwrap();
            assert_eq!(o.status.code(), Some(4), "{} / {}: {}", v.display(), i.display(), String::from_utf8_lossy(&o.stderr));
            assert_eq!(snapshot(&vault), before, "nichts angelegt");
        }
    }
}
