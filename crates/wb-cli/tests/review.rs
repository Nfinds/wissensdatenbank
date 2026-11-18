//! Prüfpunkte aus Vertrag §15 (Durchsicht des Kerns), Ende zu Ende gegen die Binärdatei.
//! Alle Inhalte sind erfunden und werden zur Testzeit erzeugt.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant, SystemTime};

struct Fx {
    _t: tempfile::TempDir,
    root: PathBuf,
    vault: PathBuf,
    index: PathBuf,
}

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_wb")
}

fn cmd(vault: &Path, index: &Path) -> Command {
    let mut c = Command::new(bin());
    c.env("WB_VAULT", vault).env("WB_INDEX", index).env_remove("WB_READONLY").env_remove("WB_NO_SYNC").env_remove("NO_COLOR");
    c
}

impl Fx {
    fn wb(&self, args: &[&str]) -> Output {
        cmd(&self.vault, &self.index).args(args).output().expect("wb starten")
    }
    fn write(&self, rel: &str, s: &str) {
        let p = self.vault.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, s).unwrap();
    }
}

fn out(o: &Output) -> String {
    String::from_utf8(o.stdout.clone()).expect("stdout UTF-8")
}
fn err(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

/// Erfundener Vault mit Namen, die das Zitieren prüfen (Leerzeichen, Umlaut, `'`, `#`, `$`).
fn fixture() -> Fx {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().to_path_buf();
    let fx = Fx { vault: root.join("vault"), index: root.join("index"), root, _t: t };
    fx.write(
        "Projects/Demo/00 Overview.md",
        "---\ntype: overview\nstatus: active\npaths: [\"C:/Arbeit/Demo\"]\n---\n# Demo\n\n## Stand\n\nDer Prüfstand läuft. Okapi steht.\n\n## Offene Punkte\n\n- Messung wiederholen\n- Okapi zählen\n\n## Themen\n\n- [[Messung]]\n- [[C# Grundlagen]]\n- [[Peter's Notiz]]\n",
    );
    fx.write("Projects/Demo/Messung.md", "---\ntype: topic\nchecked_on: 2026-11-01\n---\n# Messung\n\n## Stand\n\nDie Größe der Messreihe wächst. Okapi im Feld.\n\n## Belege\n\n- Lauf 1 ok.\n");
    fx.write("Projects/Demo/C# Grundlagen.md", "---\ntype: topic\n---\n# C# Grundlagen\n\n## Stand\n\nOkapi in C# gezählt.\n\n## C# Tipps\n\nOkapi zweimal.\n");
    fx.write("Projects/Demo/Peter's Notiz.md", "---\ntype: topic\n---\n# Peter's Notiz\n\n## Stand\n\nOkapi bei Peter.\n");
    fx.write("Projects/Demo/Größe $5 Kosten.md", "---\ntype: topic\n---\n# Größe $5 Kosten\n\n## Stand\n\nOkapi kostet.\n");
    for i in 0..40 {
        fx.write(
            &format!("Projects/Demo/Viele/Notiz {i:02}.md"),
            &format!("---\ntype: topic\n---\n# Notiz {i:02}\n\n## A\n\nOkapi Füllstoff {i}. Noch ein Satz mit Füllstoff.\n\n## B\n\nOkapi zweiter Teil {i}.\n"),
        );
    }
    assert!(fx.wb(&["sync"]).status.success());
    fx
}

/// Preis wie in `wb --help`: je Zeile Zeichen × 2/7 aufgerundet, plus 1.
fn price(s: &str) -> u32 {
    s.lines().map(|l| ((l.chars().count() as u64 * 2).div_ceil(7) + 1) as u32).sum()
}

/// Angehobenes Budget aus der Kopfzeile (`budget=N(angehoben)`) oder aus JSON.
fn announced_budget(s: &str, requested: u32) -> u32 {
    if let Some(i) = s.find("budget=") {
        let rest = &s[i + 7..];
        if let Some(j) = rest.find("(angehoben)") {
            return rest[..j].parse().unwrap();
        }
    }
    if s.starts_with('{') {
        let v: serde_json::Value = serde_json::from_str(s).unwrap();
        if v["budget_raised"] == true {
            return v["budget"].as_u64().unwrap() as u32;
        }
    }
    requested
}

/// Bash-artiges Zerlegen der Formen, die `wb` erzeugt: bar, '…', "…", '\''.
fn split_words(s: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut cur = String::new();
    let mut any = false;
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        match c {
            ' ' => {
                if any {
                    words.push(std::mem::take(&mut cur));
                    any = false;
                }
            }
            '\'' => {
                any = true;
                for d in it.by_ref() {
                    if d == '\'' {
                        break;
                    }
                    cur.push(d);
                }
            }
            '"' => {
                any = true;
                for d in it.by_ref() {
                    if d == '"' {
                        break;
                    }
                    cur.push(d);
                }
            }
            '\\' => {
                any = true;
                if let Some(d) = it.next() {
                    cur.push(d);
                }
            }
            c => {
                any = true;
                cur.push(c);
            }
        }
    }
    if any {
        words.push(cur);
    }
    words
}

fn more_lines(s: &str) -> Vec<String> {
    s.lines().filter_map(|l| l.strip_prefix("more: ")).map(str::to_string).collect()
}

/// `more:`-Befehl ausführen (ohne Shell, Wörter wie Bash sie sähe).
fn run_more(fx: &Fx, line: &str) -> Output {
    let w = split_words(line);
    assert_eq!(w[0], "wb", "{line}");
    let a: Vec<&str> = w[1..].iter().map(String::as_str).collect();
    fx.wb(&a)
}

#[test]
fn budget_is_a_hard_cap_in_every_form() {
    let fx = fixture();
    let long: String = (0..400).map(|i| format!("Okapi Satz {i} mit etwas Text. ")).collect();
    fx.write("Projects/Demo/Lang.md", &format!("---\ntype: topic\n---\n# Lang\n\n## Stand\n\n{long}\n\n## Belege\n\n- [[Messung]] [[Fehlt]]\n"));
    let cases: Vec<Vec<&str>> = vec![
        vec!["search", "Okapi"],
        vec!["search", "Okapi", "--json"],
        vec!["search", "Okapi", "Füllstoff", "--limit", "30"],
        vec!["read", "Lang"],
        vec!["read", "Lang", "--json"],
        vec!["read", "Lang", "--links"],
        vec!["read", "Lang", "--links", "--json"],
        vec!["read", "Messung", "--links"],
        vec!["read", "Lang", "--outline"],
        vec!["read", "Lang#Stand"],
        vec!["context", "demo"],
        vec!["context", "demo", "--json"],
        vec!["context", "demo", "--journal", "50"],
    ];
    for c in &cases {
        for budget in [1u32, 30, 60, 81, 150, 300, 700, 5000] {
            let b = budget.to_string();
            let mut args = c.clone();
            args.extend(["--budget", &b]);
            let o = fx.wb(&args);
            assert!(o.status.success(), "{args:?}: {}", err(&o));
            let s = out(&o);
            let cap = announced_budget(&s, budget);
            assert!(cap >= budget, "{args:?}");
            assert!(price(&s) <= cap, "{args:?}: Preis {} > Budget {cap}\n{s}", price(&s));
            if cap > budget {
                assert!(s.contains("(angehoben)") || s.contains("\"budget_raised\":true"), "{args:?}: angehoben, aber nicht genannt\n{s}");
            }
            if args.contains(&"--json") {
                let _: serde_json::Value = serde_json::from_str(&s).unwrap_or_else(|e| panic!("{args:?}: {e}\n{s}"));
            }
        }
    }
    // Unter dem Mindestwert: angehoben und genannt, erster Treffer trotzdem da.
    let s = out(&fx.wb(&["search", "Okapi", "--budget", "10"]));
    assert!(s.lines().next().unwrap().contains("budget=80(angehoben)") || s.contains("(angehoben)"), "{s}");
    assert!(s.lines().nth(1).is_some_and(|l| l.starts_with("1 ")), "{s}");
    // Riesige Werte laufen nicht über.
    assert!(fx.wb(&["search", "Okapi", "--budget", "4294967295"]).status.success());
    assert!(fx.wb(&["context", "demo", "--budget", "4294967295"]).status.success());
    assert!(fx.wb(&["read", "Lang", "--budget", "4294967295", "--links"]).status.success());
}

#[test]
fn more_lines_execute_unchanged_and_make_progress() {
    let fx = fixture();
    // Suche: Seiten bis zum Ende folgen, keine Seite doppelt, alle Treffer genau einmal.
    let first = out(&fx.wb(&["search", "Okapi", "-p", "Demo", "--per-note", "1", "--limit", "5", "--budget", "200"]));
    let total: usize = first.split("hits=").nth(1).unwrap().split(' ').next().unwrap().split('/').nth(1).unwrap().parse().unwrap();
    let mut seen = Vec::new();
    let mut page = first;
    let mut steps = 0;
    loop {
        for l in page.lines().skip(1) {
            if let Some((rank, rest)) = l.split_once(' ')
                && rank.parse::<u32>().is_ok()
            {
                seen.push(rest.split(" L").next().unwrap().to_string());
            }
        }
        let m = more_lines(&page);
        if m.is_empty() {
            break;
        }
        assert_eq!(m.len(), 1);
        assert!(m[0].contains("-p Demo") && m[0].contains("--per-note 1") && m[0].contains("--limit 5") && m[0].contains("--budget 200"), "{}", m[0]);
        let o = run_more(&fx, &m[0]);
        assert!(o.status.success(), "{}: {}", m[0], err(&o));
        let next = out(&o);
        assert_ne!(next, page, "more: zeigt auf sich selbst");
        page = next;
        steps += 1;
        assert!(steps < 100);
    }
    let mut uniq = seen.clone();
    uniq.sort();
    uniq.dedup();
    assert_eq!(uniq.len(), seen.len(), "doppelte Treffer: {seen:?}");
    assert_eq!(seen.len(), total);
    assert!(seen.iter().any(|a| a.contains("Peter's Notiz")) && seen.iter().any(|a| a.contains("C# Grundlagen")) && seen.iter().any(|a| a.contains("$5")));

    // Lesen: Notizen mit Sonderzeichen in kleinen Stücken lesen, bis nichts mehr fehlt.
    for name in ["Peter's Notiz", "C# Grundlagen", "Größe $5 Kosten", "Messung"] {
        let mut page = out(&fx.wb(&["read", name, "--budget", "60"]));
        let mut steps = 0;
        while let Some(m) = more_lines(&page).first().cloned() {
            let o = run_more(&fx, &m);
            assert!(o.status.success(), "{m}: {}", err(&o));
            let next = out(&o);
            assert_ne!(next, page, "{m}");
            page = next;
            steps += 1;
            assert!(steps < 50, "{name}");
        }
    }
}

#[test]
fn long_line_without_sentence_end_is_split_and_continued() {
    let fx = fixture();
    let word: String = "ä".repeat(7) + "x";
    let line: String = (0..1500).map(|i| format!("{word}{i}")).collect::<Vec<_>>().join("");
    fx.write("Projects/Demo/Zeile.md", &format!("---\ntype: topic\n---\n# Zeile\n\n{line}\n\nEnde.\n"));
    let mut page = out(&fx.wb(&["read", "Zeile", "--budget", "400"]));
    let mut text = String::new();
    let mut steps = 0;
    loop {
        for l in page.lines().skip(1).filter(|l| !l.starts_with("more: ") && !l.starts_with("note: ")) {
            text.push_str(l.strip_suffix('…').unwrap_or(l));
            text.push('\n');
        }
        let Some(m) = more_lines(&page).first().cloned() else { break };
        assert!(m.contains("--lines 6:") || m.contains("--lines 5-") || m.contains("--lines"), "{m}");
        let o = run_more(&fx, &m);
        assert!(o.status.success(), "{m}: {}", err(&o));
        assert!(price(&out(&o)) <= 400, "{}", out(&o));
        page = out(&o);
        steps += 1;
        assert!(steps < 200);
    }
    assert!(steps > 3, "lange Zeile muss in Stücken kommen");
    let joined: String = text.lines().filter(|l| !l.is_empty()).collect::<Vec<_>>().join("\n");
    assert!(joined.replace('\n', "").contains(&line), "Zeile vollständig und ohne Verlust");
    assert!(joined.ends_with("Ende."));
    // Spaltenform direkt
    let o = fx.wb(&["read", "Zeile", "--lines", "6:3-6", "--budget", "5000"]);
    assert_eq!(out(&o).lines().nth(1).unwrap(), &line[line.char_indices().nth(2).unwrap().0..]);
}

#[test]
fn json_is_valid_ascii_for_every_command_and_two_indexes_agree() {
    let fx = fixture();
    let idx2 = fx.root.join("index2");
    let runs: Vec<Vec<&str>> = vec![
        vec!["search", "Okapi", "--json"],
        vec!["search", "Prüfstand", "--json"],
        vec!["read", "Messung", "--json"],
        vec!["read", "Messung", "--links", "--json"],
        vec!["read", "Messung", "--outline", "--json"],
        vec!["read", "C# Grundlagen#C# Tipps", "--json"],
        vec!["context", "demo", "--json"],
        vec!["check", "--json"],
        vec!["stats", "--json"],
        vec!["sync", "--json"],
    ];
    let a: Vec<Vec<u8>> = runs.iter().map(|r| fx.wb(r).stdout).collect();
    assert!(cmd(&fx.vault, &idx2).args(["sync"]).output().unwrap().status.success());
    let b: Vec<Vec<u8>> = runs.iter().map(|r| cmd(&fx.vault, &idx2).args(r).output().unwrap().stdout).collect();
    for (i, r) in runs.iter().enumerate() {
        assert!(a[i].is_ascii(), "{r:?}");
        let _: serde_json::Value = serde_json::from_slice(&a[i]).unwrap_or_else(|e| panic!("{r:?}: {e}"));
        assert_eq!(a[i], b[i], "{r:?}: zwei unabhängig gebaute Indexe geben dasselbe aus");
    }
    // Projektliste ohne Ziel
    let o = cmd(&fx.vault, &fx.index).current_dir(&fx.root).args(["context", "--json"]).output().unwrap();
    assert!(o.stdout.is_ascii());
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(v["projects"][0]["name"], "Demo");
    // Textformen ebenfalls gleich
    for r in [&["search", "Okapi"][..], &["read", "Messung"], &["context", "demo"], &["check"], &["sync"]] {
        let x = fx.wb(r).stdout;
        let y = cmd(&fx.vault, &idx2).args(r).output().unwrap().stdout;
        assert_eq!(x, y, "{r:?}");
    }
    // Schreibbefehle mit --json in einer Kopie
    let o = fx.wb(&["new", "Neu Json", "-p", "Demo", "--date", "2026-11-09", "--json"]);
    assert!(o.stdout.is_ascii() && serde_json::from_slice::<serde_json::Value>(&o.stdout).is_ok(), "{}", err(&o));
    let o = fx.wb(&["log", "--date", "2026-11-09", "--json", "Zeile"]);
    assert!(o.stdout.is_ascii() && serde_json::from_slice::<serde_json::Value>(&o.stdout).is_ok(), "{}", err(&o));
    let o = fx.wb(&["mv", "Neu Json", "Neu Json zwei", "--json"]);
    assert!(o.stdout.is_ascii() && serde_json::from_slice::<serde_json::Value>(&o.stdout).is_ok(), "{}", err(&o));
}

#[test]
fn fts_special_characters_never_break_the_query() {
    let fx = fixture();
    for q in [
        &["\""][..],
        &["*"],
        &["NEAR"],
        &["AND"],
        &["OR", "NOT"],
        &["("],
        &[")"],
        &["Okapi*"],
        &["\"Okapi"],
        &["NEAR(Okapi", "Feld)"],
        &["^Okapi"],
        &["a:b"],
        &["{x}"],
        &["Okapi", "AND", "("],
        &["--", "-Okapi"],
        &["--", "-"],
    ] {
        let mut args = vec!["search"];
        args.extend_from_slice(q);
        let o = fx.wb(&args);
        // Treffer, leeres Ergebnis oder (nur Satzzeichen) ein Aufruffehler mit Hinweis, nie ein Indexfehler.
        match o.status.code() {
            Some(0 | 1) => assert!(err(&o).is_empty(), "{q:?}: {}", err(&o)),
            Some(2) => assert!(err(&o).starts_with("error[usage]: ") && err(&o).contains("
hint: "), "{q:?}: {}", err(&o)),
            c => panic!("{q:?}: {c:?} {}", err(&o)),
        }
    }
    assert_eq!(fx.wb(&["search", "Okapi*"]).status.code(), Some(0));
}

#[test]
fn addresses_outside_the_vault_are_never_read() {
    let fx = fixture();
    fs::write(fx.root.join("draussen.md"), "# Draußen\n\nGEHEIMTEXT\n").unwrap();
    let abs = fx.root.join("draussen.md").display().to_string();
    for a in [
        "../draussen",
        "../draussen.md",
        "Projects/../../draussen",
        abs.as_str(),
        "C:/Windows/win.ini",
        "C:draussen",
        "\\\\server\\share\\x",
        "//server/share/x",
        "/etc/passwd",
    ] {
        let o = fx.wb(&["read", a]);
        assert!(!out(&o).contains("GEHEIMTEXT"), "{a}");
        assert!(matches!(o.status.code(), Some(1 | 2)), "{a}: {:?} {}", o.status.code(), err(&o));
        assert!(err(&o).starts_with("error["), "{a}");
    }
    for to in ["../raus", "C:/raus", "\\\\server\\share\\raus", "/raus"] {
        let o = fx.wb(&["mv", "Messung", to]);
        assert_eq!(o.status.code(), Some(2), "{to}: {}", err(&o));
    }
    assert!(!fx.root.join("raus.md").exists());
}

#[test]
fn every_error_has_code_and_hint() {
    let fx = fixture();
    fx.write("Projects/Demo/A/Gleich.md", "# Gleich A
");
    fx.write("Projects/Demo/B/Gleich.md", "# Gleich B
");
    let cases: Vec<(Vec<&str>, i32)> = vec![
        (vec!["search", "gibtesnichtxyz"], 1),
        (vec!["read", "Fehlt ganz"], 1),
        (vec!["read", "Gleich"], 2),
        (vec!["read", "Messung", "--lines", "99"], 1),
        (vec!["read", "Messung", "--lines", "x-y"], 2),
        (vec!["read", "Messung#Gibtsnicht"], 1),
        (vec!["read", "Messung", "--part", "unsinn"], 2),
        (vec!["search", "Okapi", "--limit", "0"], 2),
        (vec!["search", "Okapi", "-t", "unsinn"], 2),
        (vec!["context", "gibtsnicht"], 1),
        (vec!["context", "demo", "--journal", "51"], 2),
        (vec!["search", "-x"], 2),
        (vec!["read"], 2),
        (vec!["gibtsnicht"], 2),
        (vec!["search", "Okapi", "--budget", "viel"], 2),
        (vec!["new", "Messung", "-p", "Demo"], 2),
        (vec!["log", "-n", "Fehlt ganz", "x"], 1),
    ];
    for (args, code) in &cases {
        let o = fx.wb(args);
        assert_eq!(o.status.code(), Some(*code), "{args:?}: {}", err(&o));
        let e = err(&o);
        if *code == 1 && e.is_empty() {
            continue; // leeres Suchergebnis ist kein Fehler
        }
        let mut l = e.lines();
        let first = l.next().unwrap_or("");
        assert!(first.starts_with("error[") && first.contains("]: "), "{args:?}: {e}");
        assert!(l.next().is_some_and(|h| h.starts_with("hint: ") && h.len() > 8), "{args:?}: {e}");
    }
    let e = err(&fx.wb(&["search", "-x"]));
    assert!(e.starts_with("error[usage]: ") && e.contains("wb search -- -begriff"), "{e}");
    assert_eq!(fx.wb(&["--help"]).status.code(), Some(0));
    assert_eq!(fx.wb(&["search", "--", "-Okapi"]).status.code(), Some(0), "führendes - nach -- ist ein Suchbegriff");
}

#[test]
fn read_only_refusals_exit_4_end_to_end() {
    let fx = fixture();
    let before = snapshot(&fx.vault);
    for ro in [true, false] {
        for args in [&["new", "Neu", "-p", "Demo"][..], &["log", "Zeile"], &["mv", "Messung", "Messung neu"]] {
            let mut c = cmd(&fx.vault, &fx.index);
            if ro {
                c.arg("--read-only");
            } else {
                c.env("WB_READONLY", "1");
            }
            let o = c.args(args).output().unwrap();
            assert_eq!(o.status.code(), Some(4), "{args:?}: {}", err(&o));
            assert!(err(&o).starts_with("error[read-only]: ") && err(&o).contains("\nhint: "), "{}", err(&o));
        }
    }
    // Index läge im Vault
    let o = Command::new(bin()).args(["--read-only", "search", "Okapi"]).env("WB_VAULT", &fx.vault).env_remove("WB_INDEX").output().unwrap();
    assert_eq!(o.status.code(), Some(4), "{}", err(&o));
    assert_eq!(before, snapshot(&fx.vault), "nichts im Vault verändert");
    // Lesen geht weiter
    let mut c = cmd(&fx.vault, &fx.index);
    assert_eq!(c.args(["--read-only", "search", "Okapi"]).output().unwrap().status.code(), Some(0));
}

#[test]
fn index_through_junction_into_vault_is_refused() {
    if !cfg!(windows) {
        return;
    }
    let fx = fixture();
    let link = fx.root.join("verweis");
    let o = Command::new("cmd").args(["/C", "mklink", "/J"]).arg(&link).arg(&fx.vault).output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stdout));
    let before = snapshot(&fx.vault);
    for idx in [link.join("neu").join("tiefer"), link.join(".wb2"), link.clone()] {
        let o = Command::new(bin()).arg("--read-only").arg("--index").arg(&idx).args(["search", "Okapi"]).env("WB_VAULT", &fx.vault).output().unwrap();
        assert_eq!(o.status.code(), Some(4), "{}: {}", idx.display(), err(&o));
    }
    assert_eq!(before, snapshot(&fx.vault), "kein Index über die Verknüpfung im Vault angelegt");
    let _ = fs::remove_dir(&link);
}

#[test]
fn check_exit_codes_text_and_json_agree() {
    let fx = fixture();
    fx.write("Projects/Demo/Tot.md", "---\ntype: topic\n---\n# Tot\n\n[[Gibt es nicht]]\n");
    for strict in [false, true] {
        let mut a = vec!["check"];
        if strict {
            a.push("--strict");
        }
        let t = fx.wb(&a);
        a.push("--json");
        let j = fx.wb(&a);
        assert_eq!(t.status.code(), j.status.code());
        assert_eq!(t.status.code(), Some(if strict { 1 } else { 0 }), "{}", out(&t));
    }
    fs::remove_file(fx.vault.join("Projects/Demo/Tot.md")).unwrap();
    // Begrenzte Ausgabe mit ausführbarer Fortsetzung
    let s = out(&fx.wb(&["check", "--limit", "1"]));
    if let Some(m) = more_lines(&s).first() {
        let o = run_more(&fx, m);
        assert!(o.status.success() && out(&o) != s, "{m}");
    }
}

#[test]
fn control_characters_are_shown_visibly() {
    let fx = fixture();
    fx.write("Projects/Demo/Steuer.md", "---\ntype: topic\n---\n# Steuer\n\n## Stand\n\nOkapi \u{1b}[31mrot\u{1b}[0m und Glocke \u{7} und \u{9b}2J Ende.\n");
    for args in [&["search", "Okapi", "rot"][..], &["read", "Steuer"], &["context", "demo", "--budget", "5000"], &["search", "Glocke", "--human"]] {
        let o = fx.wb(args);
        assert!(o.status.success(), "{args:?}: {}", err(&o));
        for bad in [0x1bu8, 0x07] {
            assert!(!o.stdout.contains(&bad), "{args:?}: rohes Steuerzeichen {bad:#x}");
        }
        assert!(!out(&o).contains('\u{9b}'), "{args:?}");
    }
    assert!(out(&fx.wb(&["read", "Steuer"])).contains("\u{241b}[31mrot"));
}

fn set_mtime(p: &Path, t: SystemTime) {
    fs::File::options().write(true).open(p).unwrap().set_modified(t).unwrap();
}

#[test]
fn same_size_and_mtime_change_is_pinned_and_sync_full_heals() {
    let fx = fixture();
    let p = fx.vault.join("Projects/Demo/Gleich gross.md");
    let old = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    fs::write(&p, "---\ntype: topic\n---\n# Gleich\n\n## Stand\n\nWortalpha steht.\n").unwrap();
    set_mtime(&p, old);
    assert!(fx.wb(&["sync"]).status.success());
    // Frisch geändert (Zeit nicht älter als der letzte Sync): wird gehasht und erkannt.
    let q = fx.vault.join("Projects/Demo/Frisch.md");
    fs::write(&q, "---\ntype: topic\n---\n# Frisch\n\n## Stand\n\nWortgamma steht.\n").unwrap();
    let t = fs::metadata(&q).unwrap().modified().unwrap();
    assert!(fx.wb(&["sync"]).status.success());
    fs::write(&q, "---\ntype: topic\n---\n# Frisch\n\n## Stand\n\nWortdelta steht.\n").unwrap();
    set_mtime(&q, t);
    assert_eq!(fx.wb(&["search", "Wortdelta"]).status.code(), Some(0), "gleiche Größe und Zeit, aber frisch: erkannt");
    // Alte Zeit absichtlich zurückgesetzt: bleibt unbemerkt (festgehalten, README nennt `wb sync --full`).
    fs::write(&p, "---\ntype: topic\n---\n# Gleich\n\n## Stand\n\nWortbetaa steht.\n").unwrap();
    set_mtime(&p, old);
    assert_eq!(fx.wb(&["search", "Wortbetaa"]).status.code(), Some(1), "Randfall: gleiche Größe, Zeit zurückgesetzt");
    assert!(fx.wb(&["sync", "--full"]).status.success());
    assert_eq!(fx.wb(&["search", "Wortbetaa"]).status.code(), Some(0), "sync --full heilt");
    // Leerlauf bleibt Leerlauf
    let s = out(&fx.wb(&["sync"]));
    let s2 = out(&fx.wb(&["sync"]));
    assert!(s2.contains("added=0 changed=0 removed=0 touched=0"), "{s} / {s2}");
}

#[test]
fn broken_index_is_rebuilt_by_sync_full() {
    let fx = fixture();
    let db = fx.index.join("index.db");
    for f in ["index.db-wal", "index.db-shm"] {
        let _ = fs::remove_file(fx.index.join(f));
    }
    fs::write(&db, b"kein SQLite, nur Text, lang genug fuer einen Kopf ................................................................").unwrap();
    let o = fx.wb(&["search", "Okapi"]);
    assert_eq!(o.status.code(), Some(3));
    assert!(err(&o).contains("wb sync --full"), "{}", err(&o));
    let o = fx.wb(&["sync", "--full"]);
    assert!(o.status.success(), "{}", err(&o));
    assert_eq!(fx.wb(&["search", "Okapi"]).status.code(), Some(0));
    // Fehlende Tabelle bei passender Version: wird neu angelegt
    let conn = rusqlite::Connection::open(&db).unwrap();
    conn.execute_batch("DROP TABLE links;").unwrap();
    drop(conn);
    let o = fx.wb(&["read", "Messung", "--links"]);
    assert!(o.status.success(), "{}", err(&o));
}

#[test]
fn nfc_nfd_name_collision_is_reported_and_stable() {
    let fx = fixture();
    let nfc = "Projects/Demo/Gr\u{f6}\u{df}e.md";
    let nfd = "Projects/Demo/Gro\u{308}\u{df}e.md";
    fx.write(nfc, "---\ntype: topic\n---\n# Größe\n\n## Stand\n\nWortnfc.\n");
    fx.write(nfd, "---\ntype: topic\n---\n# Größe\n\n## Stand\n\nWortnfd.\n");
    if fs::read_dir(fx.vault.join("Projects/Demo")).unwrap().count() < 2 {
        return;
    }
    assert!(fx.wb(&["sync"]).status.success());
    let s = out(&fx.wb(&["sync"]));
    assert!(s.contains("added=0 changed=0 removed=0 touched=0"), "zweiter Sync ändert nichts: {s}");
    let c = out(&fx.wb(&["check"]));
    assert!(c.contains("name-collision"), "{c}");
    let a = out(&fx.wb(&["read", "Größe"]));
    let idx2 = fx.root.join("index-nfd");
    let b = String::from_utf8(cmd(&fx.vault, &idx2).args(["read", "Größe"]).output().unwrap().stdout).unwrap();
    assert_eq!(a, b, "beide Indexe wählen dieselbe Datei");
    // Byteweise erste Datei: NFD (o + U+0308) kommt vor NFC (ö)
    assert!(a.contains("Wortnfd"), "{a}");
}

#[test]
fn every_search_address_is_readable() {
    let fx = fixture();
    fx.write("Projects/Demo/Raute #1 Plan.md", "---\ntype: topic\n---\n# Raute\n\n## Ziel #2\n\nOkapi Raute.\n");
    let s = out(&fx.wb(&["search", "Okapi", "--limit", "60", "--per-note", "3", "--budget", "50000"]));
    let mut n = 0;
    for l in s.lines().skip(1) {
        let Some((rank, rest)) = l.split_once(' ') else { continue };
        if rank.parse::<u32>().is_err() {
            continue;
        }
        let addr = rest.split(" L").next().unwrap();
        let o = fx.wb(&["read", addr, "--budget", "300"]);
        assert!(o.status.success(), "{addr}: {}", err(&o));
        n += 1;
    }
    assert!(n > 40, "{s}");
    // Abschnittstitel mit `#` erscheinen als Zeilenanker
    assert!(s.contains("Raute #1 Plan#L6 "), "{s}");
    assert!(fx.wb(&["read", "Raute #1 Plan#Ziel #2"]).status.success());
    assert!(fx.wb(&["read", "C# Grundlagen#C# Tipps"]).status.success());
}

#[test]
fn mixed_load_reads_never_fail() {
    let fx = Arc::new(fixture());
    let stop = Arc::new(AtomicBool::new(false));
    let fails = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let runs = Arc::new(AtomicUsize::new(0));
    let dur = Duration::from_secs(std::env::var("WB_TEST_LOAD_SECS").ok().and_then(|x| x.parse().ok()).unwrap_or(4));
    let mut hs = Vec::new();
    let mixes: Vec<Vec<&'static str>> = vec![
        vec!["--no-sync", "search", "Okapi", "--limit", "30", "--budget", "4000"],
        vec!["search", "Füllstoff", "--json"],
        vec!["--no-sync", "context", "demo"],
        vec!["read", "Projects/Demo/Viele/Notiz 03#B"],
        vec!["check"],
        vec!["sync"],
    ];
    for (i, args) in mixes.into_iter().enumerate() {
        let (fx, stop, fails, runs) = (fx.clone(), stop.clone(), fails.clone(), runs.clone());
        hs.push(std::thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                let o = fx.wb(&args);
                runs.fetch_add(1, Ordering::Relaxed);
                if !matches!(o.status.code(), Some(0 | 1)) || !o.stderr.is_empty() {
                    fails.lock().unwrap().push(format!("{i} {args:?} {:?} {}", o.status.code(), err(&o)));
                }
            }
        }));
    }
    {
        let (fx, stop) = (fx.clone(), stop.clone());
        hs.push(std::thread::spawn(move || {
            let mut k = 0;
            while !stop.load(Ordering::Relaxed) {
                k += 1;
                for i in 0..5 {
                    fx.write(
                        &format!("Projects/Demo/Viele/Notiz {i:02}.md"),
                        &format!("---\ntype: topic\n---\n# Notiz\n\n## A\n\nFüllstoff Okapi v{k}\n\n## B\n\nOkapi {k}\n"),
                    );
                }
                let m = fx.vault.join(format!("Projects/Demo/Mut/M{}.md", k % 4));
                if k % 3 == 0 {
                    let _ = fs::remove_file(&m);
                } else {
                    fx.write(&format!("Projects/Demo/Mut/M{}.md", k % 4), &format!("# M\n\nOkapi {k}\n"));
                }
                std::thread::sleep(Duration::from_millis(15));
            }
        }));
    }
    let t0 = Instant::now();
    while t0.elapsed() < dur {
        std::thread::sleep(Duration::from_millis(100));
    }
    stop.store(true, Ordering::Relaxed);
    for h in hs {
        h.join().unwrap();
    }
    let f = fails.lock().unwrap();
    assert!(f.is_empty(), "{} von {} Läufen fehlgeschlagen: {:?}", f.len(), runs.load(Ordering::Relaxed), &f[..f.len().min(5)]);
}

fn snapshot(root: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut out = BTreeMap::new();
    fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
        for e in fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            let rel = p.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/");
            if p.is_dir() {
                out.insert(rel + "/", Vec::new());
                walk(root, &p, out);
            } else {
                out.insert(rel, fs::read(&p).unwrap());
            }
        }
    }
    walk(root, root, &mut out);
    out
}

#[test]
fn read_waits_for_a_file_that_is_being_rewritten() {
    let fx = fixture();
    let p = fx.vault.join("Projects/Demo/Messung.md");
    let full = fs::read(&p).unwrap();
    // Ein anderes Programm leert die Datei und füllt sie 80 ms später wieder.
    fs::write(&p, b"").unwrap();
    let (p2, full2) = (p.clone(), full.clone());
    let h = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(80));
        fs::write(&p2, &full2).unwrap();
    });
    let o = fx.wb(&["--no-sync", "read", "Messung#Belege"]);
    h.join().unwrap();
    assert!(o.status.success(), "{}", err(&o));
    assert!(out(&o).contains("Lauf 1 ok."), "{}", out(&o));
    // Ein Sync während des Leerstands indiziert nicht den Zwischenstand.
    fs::write(&p, b"").unwrap();
    let (p2, full2) = (p.clone(), full.clone());
    let h = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(80));
        fs::write(&p2, &full2).unwrap();
    });
    let s = out(&fx.wb(&["sync"]));
    h.join().unwrap();
    assert!(s.contains("errors=0"), "{s}");
    assert_eq!(fx.wb(&["--no-sync", "search", "Lauf", "-p", "Demo"]).status.code(), Some(0));
    // Wirklich leer bleibt leer (nach Wartezeit).
    fs::write(&p, b"").unwrap();
    let o = fx.wb(&["read", "Messung"]);
    assert!(o.status.success(), "{}", err(&o));
}

/// §16.4, §16.7: `wb stats --width` wird geklemmt (40..240), `--ascii` gibt reines ASCII, ohne
/// Terminal nie Farbe; `wb ui --ascii` ist ein gültiger Schalter (ohne Terminal: Nutzungsfehler).
#[test]
fn stats_width_and_ascii_are_passed_to_the_view() {
    let fx = fixture();
    for (w, max) in [("10", 40usize), ("60", 60), ("500", 240)] {
        let o = fx.wb(&["stats", "--width", w]);
        assert!(o.status.success(), "{w}: {}", err(&o));
        let widest = out(&o).lines().map(|l| l.chars().count()).max().unwrap_or(0);
        assert!(widest <= max, "--width {w}: Zeile mit {widest} Zeichen");
    }
    let o = fx.wb(&["stats", "--ascii"]);
    assert!(o.status.success() && o.stdout.is_ascii() && !o.stdout.contains(&0x1b), "{}", out(&o));
    let o = fx.wb(&["ui", "--ascii"]);
    assert_eq!(o.status.code(), Some(2), "{}", err(&o));
    assert!(err(&o).contains("Terminal"), "{}", err(&o));
    // §18.8: `wb ui` lehnt die Agent-Schalter ab, die Hilfe zu `--ascii` sagt, was sie tut.
    for flag in ["--json", "--human"] {
        let o = fx.wb(&["ui", flag]);
        assert_eq!(o.status.code(), Some(2), "{flag}");
        assert!(err(&o).starts_with(&format!("error[usage]: `wb ui` kennt kein {flag}")), "{}", err(&o));
    }
    let help = out(&fx.wb(&["stats", "--help"]));
    assert!(help.contains("Alles als ASCII"), "{help}");
}

/// Vertrag §17.2: `--json` darf beim Standardbudget nicht nur einen Treffer zeigen. Abschnitte
/// mit Ausschnitten in gewöhnlicher Länge und Umlauten (in JSON als `\uXXXX` teurer).
#[test]
fn json_search_shows_several_hits_at_the_default_budget() {
    let fx = fixture();
    for i in 0..8 {
        fx.write(
            &format!("Projects/Demo/Kalibrierung {i}.md"),
            &format!(
                "---\ntype: topic\nchecked_on: 2026-10-0{}\n---\n# Kalibrierung {i}\n\n## Stand\n\n\
                 Die Kalibrierung des Prüfstands {i} läuft über den Messschieber, der vor jedem Lauf \
                 gegen das Endmaß geprüft wird; Abweichungen über zwei Hundertstel führen zu einer \
                 neuen Messreihe und einem Eintrag im Journal. Danach folgt die Prüfung der Größe.\n\n\
                 ## Offen\n\n- Kalibrierung {i} wiederholen, sobald das Endmaß zurück ist.\n",
                i + 1
            ),
        );
    }
    let o = fx.wb(&["search", "Kalibrierung", "Prüfstand", "--json"]);
    assert!(o.status.success(), "{}", err(&o));
    let s = out(&o);
    let v: serde_json::Value = serde_json::from_str(&s).unwrap();
    assert_eq!(v["budget"], 700);
    assert_eq!(v["budget_raised"], false, "{s}");
    let shown = v["shown"].as_u64().unwrap();
    assert!(shown >= 4, "nur {shown} Treffer\n{s}");
    assert_eq!(v["hits"].as_array().unwrap().len() as u64, shown);
    assert!(price(&s) <= 700, "Preis {} > 700\n{s}", price(&s));
    // Eine Zeile je Treffer; was ein anderes Feld wiederholt, steht nicht dabei.
    assert_eq!(s.lines().filter(|l| l.starts_with("{\"rank\":")).count() as u64, shown, "{s}");
    let h = &v["hits"][0];
    assert!(h["address"].as_str().unwrap().starts_with("Projects/Demo/Kalibrierung"), "{s}");
    for gone in ["path", "section_title", "section_index", "score", "highlights"] {
        assert!(h.get(gone).is_none(), "{gone}: {s}");
    }
    assert!(v.get("terms").is_none(), "nichts zu sagen über die Begriffe: {s}");
    // Ein Tippfehler steht in `terms`.
    let t: serde_json::Value = serde_json::from_slice(&fx.wb(&["search", "Kalibrirung", "--json"]).stdout).unwrap();
    assert!(t["terms"][0]["corrected"].is_string(), "{t}");
    // Erster Treffer größer als das Budget: Ausschnitt gekürzt, Budget bleibt hart.
    let s = out(&fx.wb(&["search", "Kalibrierung", "--json", "--budget", "120"]));
    let v: serde_json::Value = serde_json::from_str(&s).unwrap();
    assert!(price(&s) <= announced_budget(&s, 120), "{s}");
    assert_eq!(v["shown"], 1, "{s}");
    // read und context: Abschnitt ohne Gliederung, Kontext ohne Wochenaktivität.
    let r: serde_json::Value = serde_json::from_slice(&fx.wb(&["read", "Kalibrierung 3#Stand", "--json"]).stdout).unwrap();
    assert!(r.get("sections").is_none() && r["meta"].get("mtime").is_none(), "{r}");
    assert!(r["text"].as_str().unwrap().contains("Endmaß"), "{r}");
    let r: serde_json::Value = serde_json::from_slice(&fx.wb(&["read", "Kalibrierung 3", "--json"]).stdout).unwrap();
    assert_eq!(r["sections"].as_array().map(Vec::len), Some(3), "ganze Notiz mit Gliederung: {r}");
    let c: serde_json::Value = serde_json::from_slice(&fx.wb(&["context", "demo", "--json"]).stdout).unwrap();
    assert!(c["project"].get("activity").is_none() && c["project"]["name"] == "Demo", "{c}");
    assert!(c["topics"].as_array().unwrap().len() >= 8, "{c}");
}

/// Vertrag §17.3: `wb context --cwd` nimmt den längsten passenden Pfad, auch wenn er nur aus dem
/// Übersichtstext abgeleitet ist; derselbe Pfad in zwei Projekten ist Exit 2 mit einem
/// ausführbaren Befehl je Kandidat.
#[test]
fn context_cwd_takes_the_longest_path_and_names_commands_when_ambiguous() {
    let fx = fixture();
    let ov = |name: &str, fm: &str, body: &str| {
        fx.write(&format!("Projects/{name}/00 Overview.md"), &format!("---\ntype: overview\n{fm}---\n# {name}\n\n## Stand\n\n{body}\n"));
    };
    ov("Breit", "paths: [\"D:/Werk\"]\n", "Alles unter D:/Werk.");
    ov("Tief", "paths: [\"D:/Werk/Tief\"]\n", "Nur der Unterordner.");
    ov("Abgeleitet", "", "Liegt in `D:/Werk/Tief/Innen/plan.md`.");
    ov("Zwilling Eins", "paths: [\"D:/Geteilt\"]\n", "Gemeinsamer Ordner.");
    ov("Zwilling Zwei", "paths: [\"D:/Geteilt\"]\n", "Gemeinsamer Ordner.");
    assert!(fx.wb(&["sync"]).status.success());
    for (dir, want, how) in [
        ("D:/Werk/Anders", "Breit", "match=paths:D:/Werk"),
        ("D:/Werk/Tief/src", "Tief", "match=paths:D:/Werk/Tief"),
        ("d:\\werk\\tief", "Tief", "match=paths:D:/Werk/Tief"),
        ("D:/Werk/Tief/Innen/x", "Abgeleitet", "match=paths~:D:/Werk/Tief/Innen"),
    ] {
        let o = fx.wb(&["context", "--cwd", dir]);
        assert!(o.status.success(), "{dir}: {}", err(&o));
        let head = out(&o).lines().next().unwrap().to_string();
        assert!(head.starts_with(&format!("project={want} ")) && head.contains(how), "{dir}: {head}");
    }
    let o = fx.wb(&["context", "--cwd", "D:/Geteilt/a"]);
    assert_eq!(o.status.code(), Some(2), "{}", out(&o));
    let e = err(&o);
    assert!(e.starts_with("error[ambiguous]: Projekt mehrdeutig"), "{e}");
    let hint = e.lines().find(|l| l.starts_with("hint: ")).unwrap();
    assert!(hint.contains("`wb context 'Zwilling Eins'`") && hint.contains("`wb context 'Zwilling Zwei'`"), "{hint}");
    // Der genannte Befehl läuft.
    let o = fx.wb(&["context", "Zwilling Zwei"]);
    assert!(out(&o).starts_with("project=Zwilling Zwei "), "{}", err(&o));
}
