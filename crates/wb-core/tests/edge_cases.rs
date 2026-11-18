//! Randfälle nach Vertrag §10. Alle Fixtures sind synthetisch und entstehen zur Testzeit in
//! einem temporären Ordner; im Repo liegen keine Notizdateien.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use wb_core::frontmatter::FrontmatterState;
use wb_core::*;

// ---------------- Hilfen ----------------

struct Fx {
    _tmp: tempfile::TempDir,
    vault: PathBuf,
    index: PathBuf,
}

impl Fx {
    fn new() -> Fx {
        let tmp = tempfile::tempdir().expect("tempdir");
        let vault = tmp.path().join("vault");
        let index = tmp.path().join("index");
        fs::create_dir_all(&vault).unwrap();
        Fx { _tmp: tmp, vault, index }
    }
    fn write(&self, rel: &str, content: impl AsRef<[u8]>) -> PathBuf {
        let p = self.vault.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, content).unwrap();
        p
    }
    fn open(&self) -> Vault {
        Vault::open(OpenOptions::new(&self.vault).index_dir(&self.index)).expect("open")
    }
    fn synced(&self) -> Vault {
        let v = self.open();
        v.sync().expect("sync");
        v
    }
}

fn topic(title: &str, body: &str) -> String {
    format!("---\ntype: topic\nchecked_on: 2026-11-01\n---\n# {title}\n\n{body}\n")
}

fn q(text: &str) -> SearchQuery {
    SearchQuery { text: text.into(), limit: 50, ..Default::default() }
}

fn hit_paths(r: &SearchResult) -> Vec<String> {
    r.hits.iter().map(|h| h.path.as_str().to_string()).collect()
}

fn codes(findings: &[Finding], path: &str) -> Vec<String> {
    findings.iter().filter(|f| f.path.as_str() == path).map(|f| f.code.clone()).collect()
}

/// Status eines ausgehenden Links: `ok:<ziel>`, `ambiguous:<n>` oder `dead`.
fn link_status(v: &Vault, src: &str, target: &str) -> String {
    let l = v.links(&NotePath::new(src)).unwrap();
    let x = l.outgoing.iter().find(|x| x.target == target).unwrap_or_else(|| panic!("Link {target} fehlt in {src}: {:?}", l.outgoing));
    match &x.status {
        LinkStatus::Ok(p) => format!("ok:{}", p.as_str()),
        LinkStatus::Ambiguous(c) => format!("ambiguous:{}", c.len()),
        LinkStatus::Dead => "dead".into(),
    }
}

/// Pfad, Größe, Änderungszeit und Inhalt aller Dateien und Ordner unter `root`.
fn tree(root: &Path) -> BTreeMap<String, (bool, u64, std::time::SystemTime, Vec<u8>)> {
    let mut out = BTreeMap::new();
    fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<String, (bool, u64, std::time::SystemTime, Vec<u8>)>) {
        for e in fs::read_dir(dir).unwrap() {
            let e = e.unwrap();
            let p = e.path();
            // fs::metadata öffnet die Datei; DirEntry::metadata liefert unter Windows für Ordner
            // den verzögert aktualisierten Eintrag im Elternordner.
            let md = fs::metadata(&p).unwrap();
            let rel = p.strip_prefix(root).unwrap().to_string_lossy().to_string();
            if md.is_dir() {
                out.insert(rel, (true, 0, md.modified().unwrap(), Vec::new()));
                walk(root, &p, out);
            } else {
                out.insert(rel, (false, md.len(), md.modified().unwrap(), fs::read(&p).unwrap()));
            }
        }
    }
    walk(root, root, &mut out);
    out
}

// ---------------- Frontmatter ----------------

#[test]
fn frontmatter_missing_unclosed_broken_duplicate() {
    let p = NotePath::new("Projects/X/A.md");
    let n = parse_note(&p, b"# Ohne Frontmatter\n\nText.\n");
    assert!(matches!(n.frontmatter, FrontmatterState::Missing));
    assert_eq!(n.title, "Ohne Frontmatter");
    assert_eq!(n.body_line, 1);

    let n = parse_note(&p, b"---\ntype: topic\n# Kopf ohne Ende\n\nText.\n");
    assert!(matches!(n.frontmatter, FrontmatterState::Unclosed));
    assert_eq!(n.body_line, 1, "ungeschlossenes Frontmatter: ganzer Text ist Inhalt");

    let n = parse_note(&p, b"---\ntype: topic\nthis is not yaml\ntitle: \"offen\nchecked_on: 2026-11-01\n---\n# K\n");
    let fm = n.frontmatter.frontmatter().expect("Block erkannt");
    let pc: Vec<&str> = fm.problems.iter().map(|x| x.code.as_str()).collect();
    assert!(pc.contains(&"fm-invalid-line"), "{pc:?}");
    assert!(pc.contains(&"fm-unclosed-quote"), "{pc:?}");
    assert_eq!(fm.get_str("checked_on"), Some("2026-11-01"));
    assert_eq!(n.body_line, 7);

    let n = parse_note(&p, b"---\ntype: topic\nstatus: alt\nstatus: neu\n---\n# K\n");
    let fm = n.frontmatter.frontmatter().unwrap();
    assert!(fm.problems.iter().any(|x| x.code == "fm-duplicate-key" && x.line == 4), "{:?}", fm.problems);
    assert_eq!(fm.get_str("status"), Some("neu"), "der letzte Wert gilt");
    assert_eq!(n.status.as_deref(), Some("neu"));
}

#[test]
fn frontmatter_findings_in_check() {
    let fx = Fx::new();
    fx.write("Projects/X/00 Overview.md", "---\ntype: overview\n---\n# X\n\n- [[Ohne]] [[Offen]] [[Kaputt]] [[Doppelt]]\n");
    fx.write("Projects/X/Ohne.md", "# Ohne\n");
    fx.write("Projects/X/Offen.md", "---\ntype: topic\n# Offen\n");
    fx.write("Projects/X/Kaputt.md", "---\ntype: topic\nkein yaml hier\n---\n# Kaputt\n");
    fx.write("Projects/X/Doppelt.md", "---\ntype: topic\ntype: source\n---\n# Doppelt\n");
    let v = fx.synced();
    let f = v.check(&CheckOptions::default()).unwrap();
    assert_eq!(codes(&f, "Projects/X/Ohne.md"), ["fm-missing"]);
    assert_eq!(codes(&f, "Projects/X/Offen.md"), ["fm-unclosed"]);
    assert_eq!(codes(&f, "Projects/X/Kaputt.md"), ["fm-invalid-line"]);
    assert_eq!(codes(&f, "Projects/X/Doppelt.md"), ["fm-duplicate-key"]);
    assert_eq!(v.meta(&NotePath::new("Projects/X/Doppelt.md")).unwrap().note_type, NoteType::Source);
}

#[test]
fn bom_and_crlf() {
    let p = NotePath::new("Projects/X/B.md");
    let mut bytes = vec![0xEF, 0xBB, 0xBF];
    bytes.extend_from_slice(b"---\r\ntype: topic\r\nchecked_on: 2026-11-01\r\n---\r\n# Titel\r\n\r\n## Stand\r\n\r\nZeile mit CRLF.\r\n");
    let n = parse_note(&p, &bytes);
    assert!(n.bom);
    assert!(n.crlf);
    assert_eq!(n.note_type, NoteType::Topic);
    assert_eq!(n.checked_on.as_deref(), Some("2026-11-01"));
    assert_eq!(n.title, "Titel");
    let stand = n.sections.iter().find(|s| s.title == "Stand").expect("Abschnitt");
    assert_eq!(stand.kind, SectionKind::State);
    assert_eq!(stand.line_start, 7);
    assert!(!n.sections.iter().any(|s| s.title.ends_with('\r')));

    let fx = Fx::new();
    fx.write("Projects/X/B.md", &bytes);
    let v = fx.synced();
    let r = v.search(&q("CRLF")).unwrap();
    assert_eq!(hit_paths(&r), ["Projects/X/B.md"]);
    assert!(!r.hits[0].snippet.contains('\r'));
    let note = v.note(&NotePath::new("Projects/X/B.md")).unwrap();
    assert_eq!(note.meta.title, "Titel");
}

#[test]
fn invalid_utf8_is_read_lossy() {
    let n = parse_note(&NotePath::new("A.md"), b"# Kaputt \xff\xfe Bytes\n");
    assert!(n.invalid_utf8);
    assert!(n.title.starts_with("Kaputt"));
}

// ---------------- leere und riesige Dateien ----------------

#[test]
fn empty_file() {
    let fx = Fx::new();
    fx.write("Projects/X/00 Overview.md", topic("X", "[[Leer]]"));
    fx.write("Projects/X/Leer.md", "");
    let v = fx.synced();
    let m = v.meta(&NotePath::new("Projects/X/Leer.md")).unwrap();
    assert_eq!(m.bytes, 0);
    assert_eq!(m.tokens, 0);
    let n = v.note(&m.path).unwrap();
    assert_eq!(n.text, "");
    assert!(codes(&v.check(&CheckOptions::default()).unwrap(), "Projects/X/Leer.md").contains(&"note-empty".to_string()));
    assert_eq!(v.search(&q("Leer")).unwrap().hits[0].path.as_str(), "Projects/X/Leer.md", "Titel aus dem Dateinamen");
}

#[test]
fn huge_file() {
    let fx = Fx::new();
    let line = "Ein ganz gewöhnlicher Satz über Wissen, Suche und Indexe, der sich oft wiederholt.\n";
    let mut s = String::from("---\ntype: topic\n---\n# Riesig\n\n");
    let mut i = 0;
    while s.len() < 20 * 1024 * 1024 {
        if i % 2000 == 0 {
            s.push_str(&format!("\n## Teil {i}\n\n"));
        }
        s.push_str(line);
        i += 1;
    }
    s.push_str("\n## Schluss\n\nDas Schlusswort heißt Quarkstrudel.\n");
    fx.write("Projects/X/Riesig.md", &s);
    fx.write("Projects/X/Klein.md", topic("Klein", "Kleine Notiz."));
    let v = fx.open();
    let rep = v.sync().unwrap();
    assert_eq!(rep.added, 2);
    assert!(rep.errors.is_empty(), "{:?}", rep.errors);
    let m = v.meta(&NotePath::new("Projects/X/Riesig.md")).unwrap();
    assert!(m.bytes >= 20 * 1024 * 1024);
    assert!(m.tokens > 5_000_000);
    let r = v.search(&q("Quarkstrudel")).unwrap();
    assert_eq!(r.hits.len(), 1);
    assert_eq!(r.hits[0].section_title, "Schluss");
    assert!(r.hits[0].snippet.chars().count() <= 240);
    assert!(v.check(&CheckOptions::default()).unwrap().iter().any(|f| f.code == "note-large"));
    // zweiter Sync ohne Änderung ist leer
    assert!(v.sync().unwrap().is_noop());
}

// ---------------- Umlaute, Unicode, Groß/Klein ----------------

#[test]
fn umlauts_sharp_s_nfc_nfd_case() {
    let fx = Fx::new();
    fx.write("Projects/Straße/Bäume und Größe.md", topic("Bäume und Größe", "Das Größenmaß der Wurzeln. Fußgänger übergueren die Brücke."));
    // NFD im Dateinamen und im Text: u + U+0308
    fx.write("Projects/Straße/Mu\u{0308}ll.md", topic("Mu\u{0308}ll", "Tru\u{0308}be Su\u{0308}ppe."));
    fx.write("Projects/Straße/Verweise.md", topic("Verweise", "[[Müll]] und [[bäume und größe]] und [[BÄUME UND GRÖßE]] und [[BÄUME UND GRÖSSE]]"));
    let v = fx.synced();
    let key = "Projects/Straße/Bäume und Größe.md";
    for qq in ["Größenmaß", "größenmass", "GROESSENMASS", "Groessenmass", "grossenmass"] {
        assert_eq!(hit_paths(&v.search(&q(qq)).unwrap()), [key], "Anfrage {qq}");
    }
    // NFD-Text ist über NFC und ASCII-Ersatz auffindbar
    for qq in ["Trübe", "Truebe", "TRU\u{0308}BE"] {
        assert!(hit_paths(&v.search(&q(qq)).unwrap()).iter().any(|p| p.contains("Müll")), "Anfrage {qq}");
    }
    // NFD-Dateiname wird als NFC geführt, die Datei bleibt lesbar
    let p = v.resolve("Müll").unwrap();
    assert_eq!(p.as_str(), "Projects/Straße/Müll.md");
    assert!(v.note(&p).unwrap().text.contains("Su\u{0308}ppe"));
    assert_eq!(v.resolve("projects/strasse/baume und grosse").unwrap().as_str(), key);
    assert_eq!(link_status(&v, "Projects/Straße/Verweise.md", "Müll"), "ok:Projects/Straße/Müll.md");
    assert_eq!(link_status(&v, "Projects/Straße/Verweise.md", "bäume und größe"), "ok:Projects/Straße/Bäume und Größe.md");
    // Groß/Klein: Windows-Dateisysteme unterscheiden das nicht, Links auch nicht
    assert_eq!(link_status(&v, "Projects/Straße/Verweise.md", "BÄUME UND GRÖßE"), "ok:Projects/Straße/Bäume und Größe.md");
    // ß und SS sind für NTFS (und Obsidian) verschiedene Namen: Link tot, die unscharfe Auflösung von `wb read` findet ihn
    assert_eq!(link_status(&v, "Projects/Straße/Verweise.md", "BÄUME UND GRÖSSE"), "dead");
    assert_eq!(v.resolve("BÄUME UND GRÖSSE").unwrap().as_str(), key);
}

// ---------------- Windows-Pfade ----------------

#[test]
fn windows_paths_backslash_spaces_case() {
    let fx = Fx::new();
    fx.write("Projects/Mein Projekt/Notiz mit Leerzeichen.md", topic("Notiz mit Leerzeichen", "Inhalt."));
    let v = fx.synced();
    let want = "Projects/Mein Projekt/Notiz mit Leerzeichen.md";
    assert_eq!(NotePath::new("Projects\\Mein Projekt\\Notiz mit Leerzeichen.md").as_str(), want);
    assert_eq!(v.resolve("Projects\\Mein Projekt\\Notiz mit Leerzeichen").unwrap().as_str(), want);
    assert_eq!(v.resolve("projects/mein projekt/NOTIZ MIT LEERZEICHEN.MD").unwrap().as_str(), want);
    // absoluter Pfad mit Backslashes
    let abs = fx.vault.join("Projects").join("Mein Projekt").join("Notiz mit Leerzeichen.md");
    assert_eq!(v.resolve(&abs.to_string_lossy()).unwrap().as_str(), want);
    let (n, s) = split_address("Projects\\Mein Projekt\\Notiz mit Leerzeichen#Stand");
    assert_eq!((n, s), ("Projects\\Mein Projekt\\Notiz mit Leerzeichen", Some("Stand")));
}

#[test]
fn windows_long_paths() {
    let fx = Fx::new();
    let mut rel = String::from("Projects/Lang");
    while fx.vault.join(&rel).to_string_lossy().len() < 300 {
        rel.push_str("/Unterordner mit langem Namen");
    }
    let rel = format!("{rel}/Tiefe Notiz.md");
    assert!(fx.vault.join(&rel).to_string_lossy().len() > 260);
    fx.write(&rel, topic("Tiefe Notiz", "Ganz unten liegt Tiefseeschlamm."));
    let v = fx.synced();
    let r = v.search(&q("Tiefseeschlamm")).unwrap();
    assert_eq!(hit_paths(&r), [rel.as_str()]);
    assert!(v.note(&NotePath::new(rel.as_str())).unwrap().text.contains("Tiefseeschlamm"));
    assert_eq!(v.resolve("Tiefe Notiz").unwrap().as_str(), rel);
}

#[test]
fn windows_reserved_names() {
    let fx = Fx::new();
    fx.write("Projects/R/00 Overview.md", topic("R", "[[CON]] [[aux]] [[NUL.md]]"));
    // Je nach Windows-Version lässt sich `CON.md` anlegen oder nicht; beides darf den Sync nicht stören.
    let created = fs::write(fx.vault.join("Projects/R/CON.md"), topic("CON", "Konsole.")).is_ok()
        && fs::read_dir(fx.vault.join("Projects/R")).unwrap().any(|e| e.unwrap().file_name() == "CON.md");
    let v = fx.open();
    let rep = v.sync().expect("Sync mit reservierten Namen");
    let con = v.resolve("CON");
    if created {
        assert_eq!(con.unwrap().as_str(), "Projects/R/CON.md");
        assert_eq!(rep.added, 2);
    } else {
        assert!(matches!(con, Err(Error::NotFound { .. })));
    }
    assert_eq!(link_status(&v, "Projects/R/00 Overview.md", "aux"), "dead");
    assert_eq!(link_status(&v, "Projects/R/00 Overview.md", "NUL.md"), "dead");
    // Schreiben unter reserviertem Namen endet in einem Fehler, nicht in einer Panik
    let _ = v.write_note(&NotePath::new("Projects/R/NUL.md"), "x");
}

// ---------------- Links ----------------

#[test]
fn links_dead_ambiguous_alias_heading_same_names() {
    let fx = Fx::new();
    fx.write("Projects/A/Notiz.md", topic("Notiz A", "## Stand\n\nA."));
    fx.write("Projects/B/Notiz.md", topic("Notiz B", "B."));
    fx.write(
        "Projects/A/Quelle.md",
        topic(
            "Quelle",
            "- tot: [[Gibt es nicht]]\n- mehrdeutig: [[Notiz]]\n- eindeutig: [[Projects/A/Notiz]]\n- Ordner und Name: [[B/Notiz]]\n\
             - Alias: [[Projects/A/Notiz|Anzeige A]]\n- Überschrift: [[Projects/A/Notiz#Stand]]\n- selbst: [[#Oben]]\n\
             - eingebettet: ![[Projects/B/Notiz]]\n\n| Spalte |\n|---|\n| [[Projects/B/Notiz\\|B in Tabelle]] |\n\n\
             ```\n[[Im Code]]\n```\n\nInline `[[auch nicht]]`.",
        ),
    );
    let v = fx.synced();
    let src = "Projects/A/Quelle.md";
    assert_eq!(link_status(&v, src, "Gibt es nicht"), "dead");
    assert_eq!(link_status(&v, src, "Notiz"), "ambiguous:2");
    assert_eq!(link_status(&v, src, "Projects/A/Notiz"), "ok:Projects/A/Notiz.md");
    assert_eq!(link_status(&v, src, "B/Notiz"), "ok:Projects/B/Notiz.md");
    assert_eq!(link_status(&v, src, ""), "ok:Projects/A/Quelle.md", "[[#Oben]] zeigt auf die Quelle selbst");
    let l = v.links(&NotePath::new(src)).unwrap();
    assert!(l.outgoing.iter().any(|x| x.alias.as_deref() == Some("Anzeige A")));
    assert!(l.outgoing.iter().any(|x| x.heading.as_deref() == Some("Stand")));
    assert!(l.outgoing.iter().any(|x| x.alias.as_deref() == Some("B in Tabelle") && x.target == "Projects/B/Notiz"));
    assert!(!l.outgoing.iter().any(|x| x.target == "Im Code" || x.target == "auch nicht"));
    // eingehende Links
    let a_in = v.links(&NotePath::new("Projects/A/Notiz.md")).unwrap().incoming;
    assert_eq!(a_in.len(), 3, "{a_in:?}");
    // Auflösung: gleicher Dateiname in zwei Ordnern
    assert!(matches!(v.resolve("Notiz"), Err(Error::Ambiguous { ref candidates, .. }) if candidates.len() == 2));
    assert_eq!(v.resolve("B/Notiz").unwrap().as_str(), "Projects/B/Notiz.md");
    assert_eq!(v.resolve("Notiz B").unwrap().as_str(), "Projects/B/Notiz.md", "Titel ist eindeutig");
    let f = v.check(&CheckOptions::default()).unwrap();
    let c = codes(&f, src);
    assert!(c.contains(&"link-dead".to_string()) && c.contains(&"link-ambiguous".to_string()), "{c:?}");
    // Abschnitt über Überschrift finden
    let n = v.note(&NotePath::new("Projects/A/Notiz.md")).unwrap();
    assert_eq!(v.find_section(&n, "stand").unwrap().title, "Stand");
    assert!(matches!(v.find_section(&n, "gibt es nicht"), Err(Error::NotFound { .. })));
}

// ---------------- Sync ----------------

#[test]
fn sync_add_change_touch_delete_rename() {
    let fx = Fx::new();
    fx.write("Projects/S/00 Overview.md", topic("S", "[[Alt]]"));
    let alt = fx.write("Projects/S/Alt.md", topic("Alt", "Das Wort Kolibri."));
    let v = fx.open();
    let r = v.sync().unwrap();
    assert_eq!((r.added, r.changed, r.removed), (2, 0, 0));
    assert!(v.sync().unwrap().is_noop());

    // Inhalt ändern
    fs::write(&alt, topic("Alt", "Das Wort Pelikan.")).unwrap();
    let r = v.sync().unwrap();
    assert_eq!((r.added, r.changed, r.removed), (0, 1, 0));
    assert!(v.search(&q("Kolibri")).unwrap().hits.is_empty());
    assert_eq!(v.search(&q("Pelikan")).unwrap().hits.len(), 1);

    // nur berühren (gleicher Inhalt, neue mtime)
    let f = fs::File::options().write(true).open(&alt).unwrap();
    f.set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(5)).unwrap();
    drop(f);
    let r = v.sync().unwrap();
    assert_eq!((r.changed, r.touched), (0, 1));

    // umbenennen
    fs::rename(&alt, fx.vault.join("Projects/S/Neu.md")).unwrap();
    let r = v.sync().unwrap();
    assert_eq!((r.added, r.removed), (1, 1));
    assert!(matches!(v.resolve("Projects/S/Alt"), Err(Error::NotFound { .. })));
    assert_eq!(hit_paths(&v.search(&q("Pelikan")).unwrap()), ["Projects/S/Neu.md"]);
    assert_eq!(link_status(&v, "Projects/S/00 Overview.md", "Alt"), "dead", "Link auf alten Namen ist jetzt tot");

    // löschen
    fs::remove_file(fx.vault.join("Projects/S/Neu.md")).unwrap();
    let r = v.sync().unwrap();
    assert_eq!(r.removed, 1);
    assert!(v.search(&q("Pelikan")).unwrap().hits.is_empty());

    // tote Links werden lebendig, wenn das Ziel wiederkommt
    fx.write("Projects/S/Alt.md", topic("Alt", "Zurück."));
    v.sync().unwrap();
    assert_eq!(link_status(&v, "Projects/S/00 Overview.md", "Alt"), "ok:Projects/S/Alt.md");

    // zweite Instanz sieht denselben Stand, Vollaufbau ergibt dasselbe
    let w = fx.open();
    assert!(w.sync().unwrap().is_noop());
    let before = serde_json::to_string(&w.notes(&NoteFilter::default()).unwrap()).unwrap();
    let r = w.rebuild().unwrap();
    assert!(r.full_rebuild);
    assert_eq!(serde_json::to_string(&w.notes(&NoteFilter::default()).unwrap()).unwrap(), before);
}

#[test]
fn hidden_folders_and_non_markdown_are_ignored() {
    let fx = Fx::new();
    fx.write(".obsidian/workspace.md", "# versteckt\n");
    fx.write("Projects/H/.trash/Weg.md", "# weg\n");
    fx.write("Projects/H/Bild.png", [0u8, 1, 2]);
    fx.write("Projects/H/Notiz.MD", topic("Notiz", "Großgeschriebene Endung."));
    let v = fx.open();
    let r = v.sync().unwrap();
    assert_eq!(r.scanned, 1);
    assert_eq!(v.resolve("Notiz").unwrap().as_str(), "Projects/H/Notiz.MD");
}

#[test]
fn concurrent_sync_from_two_connections() {
    let fx = Fx::new();
    for i in 0..200 {
        fx.write(&format!("Projects/P/Notiz {i:03}.md"), topic(&format!("Notiz {i}"), &format!("Inhalt Nummer {i} mit Zebra.")));
    }
    // Index anlegen, dann zwei unabhängige Verbindungen gleichzeitig angleichen lassen
    drop(fx.open());
    let (vault, index) = (fx.vault.clone(), fx.index.clone());
    let handles: Vec<_> = (0..2)
        .map(|_| {
            let (vault, index) = (vault.clone(), index.clone());
            std::thread::spawn(move || {
                let v = Vault::open(OpenOptions::new(&vault).index_dir(&index)).unwrap();
                let r = v.sync().unwrap();
                (r.added, v.search(&SearchQuery { text: "Zebra".into(), limit: 1, per_note: 1, ..Default::default() }).unwrap().total)
            })
        })
        .collect();
    let res: Vec<(u32, u32)> = handles.into_iter().map(|h| h.join().expect("kein Absturz")).collect();
    let added: u32 = res.iter().map(|r| r.0).sum();
    assert_eq!(added, 200, "genau eine Verbindung baut auf, die andere findet nichts mehr zu tun: {res:?}");
    assert!(res.iter().all(|r| r.1 == 200), "{res:?}");
}

// ---------------- Nur-Lese-Modus ----------------

#[test]
fn read_only_writes_nothing() {
    let fx = Fx::new();
    fx.write("Projects/RO/00 Overview.md", "---\ntype: overview\npaths: [\"C:/Arbeit/RO\"]\n---\n# RO\n\n## Stand\n\nLäuft.\n\n## Themen\n\n- [[Thema]]\n");
    fx.write("Projects/RO/Thema.md", topic("Thema", "Lesestoff ohne Ende."));
    fx.write("Journal/2026-11.md", "- 2026-11-01 10:00 [[Projects/RO/Thema]] angelegt\n");
    let before = tree(&fx.vault);

    // Index im Vault ist im Nur-Lese-Modus verboten
    let inside = Vault::open(OpenOptions::new(&fx.vault).read_only(true).index_dir(fx.vault.join(".wb")).create_index(true));
    assert!(inside.is_err());
    // ohne Index-Angabe: kein stilles Anlegen von <vault>/.wb
    assert!(Vault::open(OpenOptions::new(&fx.vault).read_only(true)).is_err());

    let v = Vault::open(OpenOptions::new(&fx.vault).read_only(true).index_dir(&fx.index)).unwrap();
    assert!(v.is_read_only());
    v.sync().unwrap();
    v.rebuild().unwrap();
    v.search(&q("Lesestoff")).unwrap();
    v.context(&ContextTarget::Project("ro".into())).unwrap();
    v.context(&ContextTarget::Cwd("C:/Arbeit/RO/src".into())).unwrap();
    v.check(&CheckOptions::default()).unwrap();
    v.journal(&JournalFilter::default()).unwrap();
    v.projects().unwrap();
    let p = v.resolve("Thema").unwrap();
    v.note(&p).unwrap();
    v.links(&p).unwrap();
    assert!(matches!(v.write_note(&p, "überschrieben"), Err(Error::ReadOnly { .. })));
    assert!(matches!(v.write_note(&NotePath::new("Projects/RO/Neu.md"), "neu"), Err(Error::ReadOnly { .. })));
    drop(v);

    assert_eq!(tree(&fx.vault), before, "Vault unverändert: keine neuen, geänderten oder berührten Dateien");
    assert!(!fx.vault.join(".wb").exists());
}

#[test]
fn write_note_is_atomic_and_leaves_no_temp_files() {
    let fx = Fx::new();
    let v = fx.open();
    let p = NotePath::new("Projects/W/Neu.md");
    v.write_note(&p, &topic("Neu", "Erste Fassung.")).unwrap();
    v.write_note(&p, &topic("Neu", "Zweite Fassung.")).unwrap();
    let names: Vec<String> = fs::read_dir(fx.vault.join("Projects/W")).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    assert_eq!(names, ["Neu.md"]);
    v.sync().unwrap();
    assert_eq!(v.search(&q("Fassung")).unwrap().hits[0].snippet, "Zweite Fassung.");
}

// ---------------- Determinismus ----------------

fn det_fixture(fx: &Fx) {
    fx.write("Projects/D/00 Overview.md", "---\ntype: overview\nstatus: active\n---\n# D\n\n## Stand\n\nAlles gleich.\n\n## Offene Punkte\n\n- eins\n- zwei\n\n## Themen\n\n- [[Gleich A]]\n- [[Gleich B|B kurz erklärt]]\n");
    // gleiche Bewertung: Reihenfolge muss über den Pfad stabil sein
    fx.write("Projects/D/Gleich A.md", topic("Gleich", "Wort Tapir steht hier."));
    fx.write("Projects/D/Gleich B.md", topic("Gleich", "Wort Tapir steht hier."));
    fx.write("Projects/D/Gleich C.md", topic("Gleich", "Wort Tapir steht hier."));
    fx.write("Journal/2026-11.md", "- 2026-11-02 09:00 [[Projects/D/Gleich A]] eins\n- 2026-11-02 09:00 [[Projects/D/Gleich B]] zwei\n");
    // Änderungszeit ist echter Vault-Stand (last_modified); für den Vergleich zweier Kopien fest setzen
    let fixed = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_793_000_000);
    for (rel, (dir, ..)) in tree(&fx.vault) {
        if !dir {
            fs::File::options().write(true).open(fx.vault.join(rel)).unwrap().set_modified(fixed).unwrap();
        }
    }
}

/// Vergleich mit Ausschnitt um die erste Abweichung (die Ausgaben sind lang).
fn assert_same(a: &str, b: &str, what: &str) {
    if a != b {
        let i = a.bytes().zip(b.bytes()).position(|(x, y)| x != y).unwrap_or(a.len().min(b.len()));
        let lo = a.floor_char_boundary(i.saturating_sub(150));
        let hi_a = a.ceil_char_boundary((i + 150).min(a.len()));
        let hi_b = b.ceil_char_boundary((i + 150).min(b.len()));
        panic!("{what}: erste Abweichung bei Byte {i}\n  a: …{}…\n  b: …{}…", &a[lo..hi_a], &b[lo.min(b.len())..hi_b]);
    }
}

fn det_output(v: &Vault) -> String {
    let mut s = String::new();
    s += &serde_json::to_string(&v.search(&q("tapir wort")).unwrap()).unwrap();
    s += &serde_json::to_string(&v.context(&ContextTarget::Project("d".into())).unwrap()).unwrap();
    s += &serde_json::to_string(&v.check(&CheckOptions::default()).unwrap()).unwrap();
    s += &serde_json::to_string(&v.projects().unwrap()).unwrap();
    s += &serde_json::to_string(&v.journal(&JournalFilter::default()).unwrap()).unwrap();
    s += &serde_json::to_string(&v.notes(&NoteFilter::default()).unwrap()).unwrap();
    s
}

#[test]
fn deterministic_output() {
    let a = Fx::new();
    det_fixture(&a);
    let va = a.synced();
    let first = det_output(&va);
    assert_same(&det_output(&va), &first, "gleicher Index, gleiche Ausgabe");
    let hits = va.search(&q("tapir")).unwrap();
    assert_eq!(hit_paths(&hits), ["Projects/D/Gleich A.md", "Projects/D/Gleich B.md", "Projects/D/Gleich C.md"]);

    // zweiter Vault, Dateien in anderer Reihenfolge angelegt, eigener Index
    let b = Fx::new();
    b.write("Projects/D/Gleich C.md", topic("Gleich", "Wort Tapir steht hier."));
    det_fixture(&b);
    let vb = b.synced();
    let out_b = det_output(&vb);
    assert_same(&out_b, &first, "anderer Index, gleiche Ausgabe");

    // Vollaufbau ändert nichts
    vb.rebuild().unwrap();
    assert_same(&det_output(&vb), &first, "nach Vollaufbau");
}

#[test]
fn context_prefers_curated_summary_and_skips_alias_equal_to_title() {
    let fx = Fx::new();
    det_fixture(&fx);
    let v = fx.synced();
    let c = v.context(&ContextTarget::Project("D".into())).unwrap();
    let b = c.topics.iter().find(|t| t.path.as_str() == "Projects/D/Gleich B.md").unwrap();
    assert_eq!(b.summary.as_deref(), Some("B kurz erklärt"));
    assert!(!c.open.is_empty());
}

#[test]
fn project_findings_follow_index_changes() {
    let fx = Fx::new();
    fx.write("Projects/F/00 Overview.md", "---\ntype: overview\n---\n# F\n\n- [[Thema]]\n");
    fx.write("Projects/F/Thema.md", topic("Thema", "Gut."));
    let v = fx.synced();
    let dead = |v: &Vault| v.projects().unwrap()[0].findings.get("link-dead").copied().unwrap_or(0);
    assert_eq!(dead(&v), 0);
    assert_eq!(dead(&v), 0, "zweiter Aufruf aus dem Zwischenspeicher");
    // eigene Änderung
    fx.write("Projects/F/Thema.md", topic("Thema", "[[Fehlt eins]]"));
    v.sync().unwrap();
    assert_eq!(dead(&v), 1);
    // Änderung über eine andere Verbindung (anderer Prozess): ohne eigenen Sync sichtbar
    fx.write("Projects/F/Zwei.md", topic("Zwei", "[[Fehlt zwei]]"));
    fx.open().sync().unwrap();
    assert_eq!(dead(&v), 2);
}

#[test]
fn all_links_equals_outgoing_links_per_note() {
    let fx = Fx::new();
    fx.write("Projects/A/Notiz.md", topic("Notiz A", "[[Projects/B/Notiz]] [[Fehlt]]"));
    fx.write("Projects/B/Notiz.md", topic("Notiz B", "[[Notiz]] [[Projects/A/Notiz|A]]"));
    fx.write("Projects/B/Leer.md", topic("Leer", "kein Link"));
    let v = fx.synced();
    let mut per_note = Vec::new();
    for m in v.notes(&NoteFilter::default()).unwrap() {
        per_note.extend(v.links(&m.path).unwrap().outgoing);
    }
    let all = v.all_links().unwrap();
    assert_eq!(all.len(), 4);
    assert_eq!(serde_json::to_string(&all).unwrap(), serde_json::to_string(&per_note).unwrap());
}

// ---------------- Format Phase 2 (Vertrag §13.3, §13.5, §13.9) ----------------

#[test]
fn types_and_status_enumeration() {
    let fx = Fx::new();
    fx.write("Projects/T/00 Overview.md", "---\ntype: overview\nstatus: active\n---\n# T\n\n- [[A]] [[B]] [[C]] [[D]]\n");
    fx.write("Projects/T/A.md", "---\ntype: rule\n---\n# A\n");
    fx.write("Projects/T/B.md", "---\ntype: topic\nstatus: rc7 läuft noch\nstate: rc7 läuft noch\n---\n# B\n");
    fx.write("Projects/T/C.md", "---\ntype: topic\nstatus: Superseded\n---\n# C\n");
    fx.write("Projects/T/D.md", "---\ntype: topic\nstatus: done\n---\n# D\n");
    fx.write("Sources/Web/Q.md", "---\ntype: source\nstatus: done\n---\n# Q\n");
    fx.write("System/Regel.md", "---\ntype: rule\n---\n# Regel\n");
    let v = fx.synced();
    let f = v.check(&CheckOptions::default()).unwrap();
    assert_eq!(codes(&f, "Projects/T/A.md"), ["fm-type"], "rule ist kein Typ mehr");
    assert_eq!(codes(&f, "Projects/T/B.md"), ["fm-status"], "Freitext in status");
    assert!(codes(&f, "Projects/T/C.md").is_empty(), "Groß/Klein egal");
    assert!(codes(&f, "Projects/T/D.md").is_empty());
    assert_eq!(codes(&f, "Sources/Web/Q.md"), ["fm-status", "orphan"], "status nur für overview/topic");
    let meta = |p: &str| v.meta(&NotePath::new(p)).unwrap();
    assert_eq!(meta("Projects/T/A.md").note_type, NoteType::Topic, "unbekannter Wert: Pfad entscheidet");
    assert_eq!(meta("System/Regel.md").note_type, NoteType::Other);
    assert_eq!(meta("Projects/T/C.md").status_kind(), Some(NoteStatus::Superseded));
    assert!(meta("Projects/T/C.md").is_superseded());
    assert_eq!(meta("Projects/T/B.md").status_kind(), None);
    assert_eq!(meta("Projects/T/B.md").state.as_deref(), Some("rc7 läuft noch"));
}

#[test]
fn journal_time_is_optional() {
    let fx = Fx::new();
    fx.write("Projects/J/00 Overview.md", "---\ntype: overview\n---\n# J\n");
    fx.write("Journal/2026-11.md", "- 2026-11-08 [[Projects/J/00 Overview]] ohne Uhrzeit\n- 2026-11-09 14:05 [[Projects/J/00 Overview]] mit Uhrzeit\n");
    let v = fx.synced();
    let j = v.journal(&JournalFilter { project: Some("J".into()), ..Default::default() }).unwrap();
    let got: Vec<_> = j.iter().map(|e| (e.date.as_str(), e.time.as_deref())).collect();
    assert!(got.contains(&("2026-11-08", None)), "{got:?}");
    assert!(got.contains(&("2026-11-09", Some("14:05"))), "{got:?}");
}

#[test]
fn secret_suspect_names_line_and_class_never_the_value() {
    let fx = Fx::new();
    let value = format!("{}{}", "ghp_", "Zq9xW8vU7tS6rR5qP4oN3mL2kJ1iH0gF9eD8");
    fx.write("Projects/S/00 Overview.md", "---\ntype: overview\n---\n# S\n\n- [[Notiz]]\n");
    fx.write("Projects/S/Notiz.md", format!("---\ntype: topic\n---\n# Notiz\n\nPrüfsumme 3f786850e387550fdab836ed7e6dc881de23001b\n\n```\nGH_TOKEN={value}\n```\n"));
    let v = fx.synced();
    let f: Vec<_> = v.check(&CheckOptions::default()).unwrap().into_iter().filter(|f| f.code == "secret-suspect").collect();
    assert_eq!(f.len(), 1, "{f:?}");
    assert_eq!((f[0].path.as_str(), f[0].line, f[0].severity), ("Projects/S/Notiz.md", 9, Severity::Warning));
    assert!(f[0].message.contains("api-key"));
    let all = serde_json::to_string(&f).unwrap();
    assert!(!all.contains(&value) && !all.contains("Zq9xW8"), "Wert darf nirgends stehen");
    // Entfernt → Befund weg
    fx.write("Projects/S/Notiz.md", "---\ntype: topic\n---\n# Notiz\n\nsauber\n");
    v.sync().unwrap();
    assert!(v.check(&CheckOptions::default()).unwrap().iter().all(|f| f.code != "secret-suspect"));
}

// ---------------- Suche und Einstieg Phase 2 (§13.10, §13.12) ----------------

fn weights_fixture(fx: &Fx) {
    fx.write("Projects/W/00 Overview.md", "---\ntype: overview\nstatus: active\n---\n# W\n\n## Stand\n\nläuft\n\n## Themen\n\n- [[Neu]] [[Alt]]\n");
    fx.write("Projects/W/Neu.md", "---\ntype: topic\nstatus: active\n---\n# Neu\n\nDer Kranich fliegt.\n");
    fx.write("Projects/W/Alt.md", "---\ntype: topic\nstatus: superseded\n---\n# Alt\n\nDer Kranich fliegt.\n");
    fx.write("Sources/Web/Vogel.md", "---\ntype: source\n---\n# Quelle\n\nDer Kranich fliegt.\n");
}

/// §23.3: Ein Abschnitt nur aus Überschrift ist kein Treffer, solange die Notiz Text bietet.
#[test]
fn search_skips_heading_only_sections() {
    let fx = Fx::new();
    fx.write("Projects/S/00 Overview.md", "---\ntype: overview\n---\n# S\n\n## Stand\n\nDie Zebrafinken brüten.\n\n## Themen\n");
    // frische Vorlage: nur Überschriften, der Titel trägt den Begriff
    fx.write("Projects/S/Zebrafinken Zucht.md", "---\ntype: topic\n---\n# Zebrafinken Zucht\n\n## Stand\n\n## Offen\n");
    // ohne H1 bestehen alle Abschnitte nur aus ihrer Überschrift
    fx.write("Projects/S/Zebrafinken Plan.md", "---\ntype: topic\n---\n## Stand\n\n## Offen\n");
    let v = fx.synced();
    let all = |text: &str| v.search(&SearchQuery { per_note: 9, ..q(text) }).unwrap();
    let addr = |r: &SearchResult| r.hits.iter().map(|h| h.address.clone()).collect::<Vec<_>>();
    let r = all("Zebrafinken Themen");
    let a = addr(&r);
    assert!(a.contains(&"Projects/S/00 Overview#Stand".to_string()), "{a:?}");
    assert!(!a.contains(&"Projects/S/00 Overview#Themen".to_string()), "leere Themenliste ist kein Leseziel: {a:?}");
    // Vorlage mit H1: die Notiz als Ganzes bleibt, ihre leeren Abschnitte nicht
    let zucht: Vec<&String> = a.iter().filter(|x| x.starts_with("Projects/S/Zebrafinken Zucht#")).collect();
    assert_eq!(zucht, ["Projects/S/Zebrafinken Zucht#Zebrafinken Zucht"], "{a:?}");
    // nur leere Abschnitte: genau einer bleibt, die Notiz ist über den Titel auffindbar
    assert_eq!(a.iter().filter(|x| x.starts_with("Projects/S/Zebrafinken Plan#")).count(), 1, "{a:?}");
    assert_eq!(r.total as usize, r.hits.len(), "Gesamtzahl ohne die entfallenen Abschnitte");
    // Filter auf die Abschnittsart: der leere Abschnitt ist der einzige Kandidat und bleibt
    let t = v.search(&SearchQuery { kind: Some(SectionKind::Topics), per_note: 9, ..q("Themen") }).unwrap();
    assert_eq!(addr(&t), ["Projects/S/00 Overview#Themen"]);
}

#[test]
fn search_demotes_sources_and_superseded_and_filters_status() {
    let fx = Fx::new();
    weights_fixture(&fx);
    let v = fx.synced();
    let r = v.search(&q("Kranich")).unwrap();
    assert_eq!(hit_paths(&r), ["Projects/W/Neu.md", "Projects/W/Alt.md", "Sources/Web/Vogel.md"], "gleicher Text: aktuell > überholt (×0,7) = Quelle (×0,7), Gleichstand nach Pfad");
    assert!((r.hits[1].score - r.hits[2].score).abs() < 1e-6 && r.hits[1].score < r.hits[0].score);
    assert_eq!(r.hits[1].status, Some(NoteStatus::Superseded));
    assert_eq!(r.hits[0].status, Some(NoteStatus::Active));
    let only = v.search(&SearchQuery { status: Some(NoteStatus::Superseded), ..q("Kranich") }).unwrap();
    assert_eq!(hit_paths(&only), ["Projects/W/Alt.md"]);
    let src = v.search(&SearchQuery { note_type: Some(NoteType::Source), ..q("Kranich") }).unwrap();
    assert_eq!(hit_paths(&src), ["Sources/Web/Vogel.md"]);
    // gefiltert ohne Abwertung: gleicher Wert wie eine aktuelle Notiz
    assert!((only.hits[0].score - r.hits[0].score).abs() < 1e-6, "{} {}", only.hits[0].score, r.hits[0].score);
}

#[test]
fn context_counts_superseded_instead_of_listing() {
    let fx = Fx::new();
    weights_fixture(&fx);
    let v = fx.synced();
    let c = v.context(&ContextTarget::Project("W".into())).unwrap();
    let t: Vec<_> = c.topics.iter().map(|t| t.path.as_str()).collect();
    assert_eq!(t, ["Projects/W/Neu.md"]);
    assert_eq!(c.superseded, 1);
}

// ---------------- Schreibweg (§14 A.2) ----------------

fn file(fx: &Fx, rel: &str) -> String {
    fs::read_to_string(fx.vault.join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

fn new_req(title: &str, t: NoteType, p: Option<&str>) -> NewNote {
    NewNote { title: Some(title.into()), note_type: t, project: p.map(str::to_string), date: Some("2026-11-09".into()), ..Default::default() }
}

#[test]
fn new_creates_valid_notes_and_never_overwrites() {
    let fx = Fx::new();
    fx.write("Projects/Prüfstand/00 Overview.md", "---\ntype: overview\nchecked_on: 2026-11-01\n---\n# Prüfstand\n");
    let v = fx.synced();
    // Projekt per Faltung gefunden, Schreibweise des Bestands
    let r = v.new_note(&new_req("Größe der Messung", NoteType::Topic, Some("pruefstand"))).unwrap();
    assert_eq!(r.path.as_str(), "Projects/Prüfstand/Größe der Messung.md");
    assert_eq!(r.link, "[[Projects/Prüfstand/Größe der Messung]]");
    assert_eq!(r.overview.as_ref().map(|p| p.as_str()), Some("Projects/Prüfstand/00 Overview.md"));
    assert_eq!(file(&fx, r.path.as_str()), "---\ntype: topic\nchecked_on: 2026-11-09\n---\n# Größe der Messung\n\n## Stand\n\n## Offen\n");
    v.sync().unwrap();
    let f = v.check(&CheckOptions::default()).unwrap();
    assert_eq!(codes(&f, r.path.as_str()), ["orphan"], "gültiges Frontmatter, nur noch nicht verlinkt");
    // nie überschreiben, auch nicht in anderer Schreibweise
    let before = tree(&fx.vault);
    for t in ["Größe der Messung", "GROESSE der Messung"] {
        let e = v.new_note(&new_req(t, NoteType::Topic, Some("Prüfstand"))).unwrap_err();
        assert_eq!(e.exit_code(), exit::USAGE, "{t}: {e}");
    }
    // unzulässige Namen
    for bad in ["a/b", "CON", "x:y", "Ende.", ".versteckt", "", "  "] {
        assert_eq!(v.new_note(&new_req(bad, NoteType::Topic, Some("Prüfstand"))).unwrap_err().exit_code(), exit::USAGE, "{bad:?}");
    }
    // Thema in unbekanntem Projekt nicht, Thema ohne Projekt nicht
    assert_eq!(v.new_note(&new_req("X", NoteType::Topic, Some("Unbekannt"))).unwrap_err().exit_code(), exit::NOT_FOUND);
    assert_eq!(v.new_note(&new_req("X", NoteType::Topic, None)).unwrap_err().exit_code(), exit::USAGE);
    assert_eq!(tree(&fx.vault), before, "abgelehnt heißt: nichts geschrieben");
    // neue Übersicht legt das Projekt an
    let o = v
        .new_note(&NewNote { note_type: NoteType::Overview, project: Some("Neu Projekt".into()), paths: vec!["C:\\Arbeit\\Neu".into()], date: Some("2026-11-09".into()), ..Default::default() })
        .unwrap();
    assert_eq!(o.path.as_str(), "Projects/Neu Projekt/00 Overview.md");
    assert!(file(&fx, o.path.as_str()).contains("paths: [\"C:/Arbeit/Neu\"]\n"));
    let s = v.new_note(&NewNote { source_url: Some("https://example.org/a".into()), ..new_req("Eine Quelle", NoteType::Source, None) }).unwrap();
    assert_eq!(s.path.as_str(), "Sources/Web/Eine Quelle.md");
    assert!(file(&fx, s.path.as_str()).contains("source_url: https://example.org/a\n"));
    v.sync().unwrap();
    let f = v.check(&CheckOptions::default()).unwrap();
    assert!(f.iter().all(|f| !f.code.starts_with("fm-")), "alle neuen Notizen gültig: {f:?}");
    let leftovers: Vec<_> = tree(&fx.vault).into_keys().filter(|k| k.contains("wbtmp")).collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}

/// Audit 2026-11-18: ein Zeilenumbruch in `--url`, `--path` oder im Übersichtstitel darf kein
/// zusätzliches Frontmatter einschleusen; `new` lehnt ab und legt nichts an.
#[test]
fn new_rejects_line_breaks_in_single_line_fields() {
    let fx = Fx::new();
    let v = fx.synced();
    let usage = |r: Result<NewNoteReport>, field: &str| match r {
        Err(Error::Usage { message, .. }) => assert!(message.contains(field), "{message}"),
        other => panic!("{field}: erwartet Usage-Fehler, kam {other:?}"),
    };
    let url = NewNote { source_url: Some("https://x\nstatus: superseded".into()), ..new_req("Quelle", NoteType::Source, None) };
    usage(v.new_note(&url), "--url");
    let path = NewNote { note_type: NoteType::Overview, project: Some("P".into()), paths: vec!["C:/a\"]\nstatus: done".into()], date: Some("2026-11-09".into()), ..Default::default() };
    usage(v.new_note(&path), "--path");
    let title = NewNote { title: Some("Titel\n## Eingeschleust".into()), ..new_req("", NoteType::Overview, Some("P")) };
    usage(v.new_note(&title), "Titel");
    let files: Vec<_> = tree(&fx.vault).into_keys().filter(|k| k.ends_with(".md")).collect();
    assert!(files.is_empty(), "nichts angelegt: {files:?}");
    // Ein einzelnes `\r` entfernt schon `strip_controls`: die URL bleibt eine Zeile, kein neuer Schlüssel.
    let cr = NewNote { source_url: Some("https://x\rtype: overview".into()), ..new_req("Quelle", NoteType::Source, None) };
    let r = v.new_note(&cr).unwrap();
    let text = file(&fx, r.path.as_str());
    assert!(text.starts_with("---\ntype: source\n") && !text.contains("\ntype: overview"), "{text}");
}

/// §23.1: `new` trägt eine Themennotiz selbst in die Themenliste der Übersicht ein.
#[test]
fn new_links_topic_in_overview_topics_section() {
    let fx = Fx::new();
    let head = "---\ntype: overview\nchecked_on: 2026-11-01\n---\n";
    // Liste vorhanden: ans Ende, Aufzählungszeichen der Liste, Folgezeile bleibt beim Vorgänger
    fx.write("Projects/Liste/00 Overview.md", format!("{head}# Liste\n\n## Themen\n\n* [[Projects/Liste/Alt|Bestand]]\n  zweite Zeile\n\n## Danach\n\nText.\n"));
    fx.write("Projects/Liste/Alt.md", topic("Alt", "Bestand."));
    // leerer Abschnitt zwischen Überschriften, leerer Abschnitt am Dateiende ohne Zeilenende
    fx.write("Projects/Leer/00 Overview.md", format!("{head}# Leer\n\n## Themen\n## Danach\n"));
    fx.write("Projects/Ende/00 Overview.md", format!("{head}# Ende\n\n## Stand\n\nLäuft.\n\n## Themen"));
    // CRLF und BOM bleiben
    fx.write("Projects/Win/00 Overview.md", format!("\u{feff}{}", format!("{head}# Win\n\n## Themen\n\n- [[Projects/Win/Alt]]\n").replace('\n', "\r\n")));
    fx.write("Projects/Win/Alt.md", topic("Alt", "Bestand."));
    // nicht eintragbar: Tabelle statt Liste, Unterabschnitte, kein Abschnitt
    let table = format!("{head}# Tabelle\n\n## Themen\n\n| Notiz |\n|---|\n| [[Projects/Tabelle/Alt]] |\n");
    fx.write("Projects/Tabelle/00 Overview.md", &table);
    fx.write("Projects/Tabelle/Alt.md", topic("Alt", "Bestand."));
    let groups = format!("{head}# Gruppen\n\n## Themen\n\n### Eins\n\n- [[Projects/Gruppen/Alt]]\n");
    fx.write("Projects/Gruppen/00 Overview.md", &groups);
    fx.write("Projects/Gruppen/Alt.md", topic("Alt", "Bestand."));
    let v = fx.synced();
    let new = |title: &str, p: &str, summary: Option<&str>| {
        v.new_note(&NewNote { summary: summary.map(str::to_string), ..new_req(title, NoteType::Topic, Some(p)) }).unwrap()
    };

    let r = new("Neu", "Liste", Some("Was  die Notiz\nklärt | [roh]"));
    assert_eq!(r.entry.as_deref(), Some("- [[Projects/Liste/Neu|Was die Notiz klärt / (roh)]]"), "eine Zeile, ohne Linkzeichen");
    assert_eq!(r.linked_line, Some(11));
    assert_eq!(
        file(&fx, "Projects/Liste/00 Overview.md"),
        format!("{head}# Liste\n\n## Themen\n\n* [[Projects/Liste/Alt|Bestand]]\n  zweite Zeile\n* [[Projects/Liste/Neu|Was die Notiz klärt / (roh)]]\n\n## Danach\n\nText.\n")
    );
    let r = new("Neu", "Leer", None);
    assert_eq!((r.entry.as_deref(), r.linked_line), (Some("- [[Projects/Leer/Neu]]"), Some(9)));
    assert_eq!(file(&fx, "Projects/Leer/00 Overview.md"), format!("{head}# Leer\n\n## Themen\n\n- [[Projects/Leer/Neu]]\n\n## Danach\n"));
    let r = new("Neu", "Ende", None);
    assert_eq!(r.linked_line, Some(13));
    assert_eq!(file(&fx, "Projects/Ende/00 Overview.md"), format!("{head}# Ende\n\n## Stand\n\nLäuft.\n\n## Themen\n\n- [[Projects/Ende/Neu]]\n"));
    let r = new("Neu", "Win", Some("Kurz"));
    assert_eq!(r.linked_line, Some(10));
    assert_eq!(
        fs::read(fx.vault.join("Projects/Win/00 Overview.md")).unwrap(),
        format!("\u{feff}{}", format!("{head}# Win\n\n## Themen\n\n- [[Projects/Win/Alt]]\n- [[Projects/Win/Neu|Kurz]]\n").replace('\n', "\r\n")).into_bytes()
    );

    // nicht eintragbar: Notiz entsteht, Übersicht bleibt, der Eintrag steht im Bericht
    for (p, before) in [("Tabelle", &table), ("Gruppen", &groups)] {
        let r = new("Neu", p, Some("Kurz"));
        assert_eq!((r.linked_line, r.entry.as_deref()), (None, Some(format!("- [[Projects/{p}/Neu|Kurz]]").as_str())), "{p}");
        assert_eq!(&file(&fx, &format!("Projects/{p}/00 Overview.md")), before, "{p}");
        assert!(fx.vault.join(format!("Projects/{p}/Neu.md")).exists(), "{p}");
    }
    // abgeschaltet: Übersicht bleibt
    let before = file(&fx, "Projects/Liste/00 Overview.md");
    let r = v.new_note(&NewNote { no_link: true, ..new_req("Ohne", NoteType::Topic, Some("Liste")) }).unwrap();
    assert_eq!((r.linked_line, r.entry.as_deref()), (None, Some("- [[Projects/Liste/Ohne]]")));
    assert_eq!(file(&fx, "Projects/Liste/00 Overview.md"), before);
    // schon verlinkt (Link vor der Notiz geschrieben): kein zweiter Eintrag
    fx.write("Projects/Ende/00 Overview.md", format!("{head}# Ende\n\n## Stand\n\nSiehe [[Projects/Ende/Vorab]].\n\n## Themen\n"));
    let before = file(&fx, "Projects/Ende/00 Overview.md");
    assert_eq!(new("Vorab", "Ende", None).linked_line, Some(9));
    assert_eq!(file(&fx, "Projects/Ende/00 Overview.md"), before);
    // --summary nur für Themen; abgelehnt heißt nichts geschrieben
    let tree_before = tree(&fx.vault);
    for t in [NoteType::Source, NoteType::Overview] {
        let e = v.new_note(&NewNote { summary: Some("x".into()), ..new_req("Quelle X", t, Some("Neues Projekt")) }).unwrap_err();
        assert_eq!(e.exit_code(), exit::USAGE, "{t}: {e}");
    }
    assert_eq!(tree(&fx.vault), tree_before);

    // Wirkung: nicht verwaist, die Beschreibung steht in der Themenliste, keine Reste
    v.sync().unwrap();
    let f = v.check(&CheckOptions::default()).unwrap();
    assert!(codes(&f, "Projects/Liste/Neu.md").is_empty(), "{f:?}");
    assert_eq!(codes(&f, "Projects/Liste/Ohne.md"), ["orphan"]);
    assert!(f.iter().all(|f| f.code != "link-dead" && f.code != "link-ambiguous"), "{f:?}");
    let c = v.context(&ContextTarget::Project("Liste".into())).unwrap();
    let t = c.topics.iter().find(|t| t.path.as_str() == "Projects/Liste/Neu.md").expect("Thema in der Liste");
    assert_eq!(t.summary.as_deref(), Some("Was die Notiz klärt / (roh)"));
    let leftovers: Vec<_> = fs::read_dir(fx.vault.join(".wb-tmp")).unwrap().flatten().map(|e| e.file_name()).collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}

#[test]
fn log_appends_lines_and_concurrent_writers_lose_nothing() {
    let fx = Fx::new();
    fx.write("Projects/L/00 Overview.md", "---\ntype: overview\nchecked_on: 2026-11-01\n---\n# L\n");
    fx.write("Journal/2026-11.md", "- 2026-11-01 alt ohne Zeilenende");
    let v = fx.synced();
    let w = v
        .append_journal(&NewJournalEntry { text: "Mehrzeilig\n gesammelt".into(), note: Some("projects/l/00 overview".into()), date: Some("2026-11-09".into()), time: None })
        .unwrap();
    assert_eq!(w.file.as_str(), "Journal/2026-11.md");
    assert_eq!(w.line, 2);
    assert_eq!(w.text, "- 2026-11-09 [[Projects/L/00 Overview]] Mehrzeilig gesammelt");
    assert_eq!(file(&fx, "Journal/2026-11.md"), "- 2026-11-01 alt ohne Zeilenende\n- 2026-11-09 [[Projects/L/00 Overview]] Mehrzeilig gesammelt\n");
    let w = v.append_journal(&NewJournalEntry { text: "mit Zeit".into(), date: Some("2026-11-09".into()), time: Some("07:05".into()), ..Default::default() }).unwrap();
    assert_eq!((w.line, w.text.as_str()), (3, "- 2026-11-09 07:05 mit Zeit"));
    let before = tree(&fx.vault);
    for bad in [
        NewJournalEntry { text: " \n ".into(), ..Default::default() },
        NewJournalEntry { text: "x".into(), note: Some("Fehlt".into()), ..Default::default() },
        NewJournalEntry { text: "x".into(), date: Some("2026-13-01".into()), ..Default::default() },
        NewJournalEntry { text: "x".into(), time: Some("25:00".into()), ..Default::default() },
    ] {
        assert!(v.append_journal(&bad).is_err(), "{bad:?}");
    }
    assert_eq!(tree(&fx.vault), before);
    // zwei Schreiber gleichzeitig (eigene Verbindungen, eigene Dateihandles)
    let handles: Vec<_> = (0..2)
        .map(|t| {
            let (vault, index) = (fx.vault.clone(), fx.index.clone());
            std::thread::spawn(move || {
                let v = Vault::open(OpenOptions::new(&vault).index_dir(&index)).unwrap();
                for i in 0..150 {
                    v.append_journal(&NewJournalEntry { text: format!("Schreiber {t} Zeile {i:03}"), date: Some("2026-12-01".into()), ..Default::default() }).unwrap();
                }
            })
        })
        .collect();
    handles.into_iter().for_each(|h| h.join().unwrap());
    let text = file(&fx, "Journal/2026-12.md");
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 300);
    let set: std::collections::BTreeSet<&str> = lines.iter().copied().collect();
    assert_eq!(set.len(), 300, "keine Zeile doppelt oder zerrissen");
    assert!(lines.iter().all(|l| l.starts_with("- 2026-12-01 Schreiber ")));
    v.sync().unwrap();
    assert_eq!(v.journal(&JournalFilter { since: Some("2026-12-01".into()), ..Default::default() }).unwrap().len(), 300);
}

fn mv_fixture(fx: &Fx) {
    fx.write(
        "Projects/M/00 Overview.md",
        "---\ntype: overview\nchecked_on: 2026-11-01\n---\n# M\n\n## Themen\n\n- [[Projects/M/Prüf Thema|Prüfung im Überblick]]\n- [[Prüf Thema#Stand]] kurz\n- ![[Projects/M/Prüf Thema]]\n\n| Spalte | Link |\n|---|---|\n| a | [[Projects/M/Prüf Thema\\|Tabelle]] |\n\n`[[Projects/M/Prüf Thema]]` als Code\n\n```\n[[Projects/M/Prüf Thema]] im Block\n```\n\n- [[Projects/M/Andere]] bleibt\n",
    );
    fx.write("Projects/M/Prüf Thema.md", "---\ntype: topic\nchecked_on: 2026-11-01\n---\n# Prüf Thema\n\n## Stand\n\nSelbst: [[Projects/M/Prüf Thema#Stand]] und [[#Stand]].\n");
    fx.write("Projects/M/Andere.md", "---\ntype: topic\nchecked_on: 2026-11-01\n---\n# Andere\n\nSiehe [[projects/m/prüf thema]].\n");
    fx.write("Journal/2026-11.md", "- 2026-11-08 [[Projects/M/Prüf Thema]] angelegt\n- 2026-11-08 [[Projects/M/Andere]] daneben\n");
}

#[test]
fn mv_rewrites_all_incoming_links_and_keeps_alias_heading_code() {
    let fx = Fx::new();
    mv_fixture(&fx);
    let v = fx.synced();
    let r = v.move_note("Projects/M/Prüf Thema", "Größe neu").unwrap();
    assert_eq!((r.from.as_str(), r.to.as_str()), ("Projects/M/Prüf Thema.md", "Projects/M/Größe neu.md"));
    let changed: Vec<(&str, u32)> = r.changed.iter().map(|(p, n)| (p.as_str(), *n)).collect();
    assert_eq!(changed, [("Journal/2026-11.md", 1), ("Projects/M/00 Overview.md", 4), ("Projects/M/Andere.md", 1), ("Projects/M/Größe neu.md", 1)]);
    assert_eq!(r.links, 7);
    assert!(!fx.vault.join("Projects/M/Prüf Thema.md").exists());
    let o = file(&fx, "Projects/M/00 Overview.md");
    assert!(o.contains("- [[Projects/M/Größe neu|Prüfung im Überblick]]\n"), "{o}");
    assert!(o.contains("- [[Projects/M/Größe neu#Stand]] kurz\n"), "{o}");
    assert!(o.contains("- ![[Projects/M/Größe neu]]\n"), "{o}");
    assert!(o.contains("| a | [[Projects/M/Größe neu\\|Tabelle]] |\n"), "{o}");
    assert!(o.contains("`[[Projects/M/Prüf Thema]]` als Code"), "Inline-Code unberührt");
    assert!(o.contains("```\n[[Projects/M/Prüf Thema]] im Block\n```"), "Codeblock unberührt");
    assert!(o.contains("- [[Projects/M/Andere]] bleibt\n"));
    assert_eq!(file(&fx, "Projects/M/Größe neu.md"), "---\ntype: topic\nchecked_on: 2026-11-01\n---\n# Prüf Thema\n\n## Stand\n\nSelbst: [[Projects/M/Größe neu#Stand]] und [[#Stand]].\n");
    assert_eq!(file(&fx, "Projects/M/Andere.md"), "---\ntype: topic\nchecked_on: 2026-11-01\n---\n# Andere\n\nSiehe [[Projects/M/Größe neu]].\n");
    assert_eq!(file(&fx, "Journal/2026-11.md"), "- 2026-11-08 [[Projects/M/Größe neu]] angelegt\n- 2026-11-08 [[Projects/M/Andere]] daneben\n");
    v.sync().unwrap();
    let f = v.check(&CheckOptions::default()).unwrap();
    assert!(f.iter().all(|f| f.code != "link-dead"), "{f:?}");
    assert_eq!(link_status(&v, "Projects/M/00 Overview.md", "Projects/M/Größe neu"), "ok:Projects/M/Größe neu.md");
    // in einen anderen Ordner, Name bleibt
    let r = v.move_note("Größe neu", "Projects/Z/").unwrap();
    assert_eq!(r.to.as_str(), "Projects/Z/Größe neu.md");
    assert!(file(&fx, "Projects/M/Andere.md").contains("[[Projects/Z/Größe neu]]"));
    // keine temporären Dateien, keine Ausführungsliste
    let leftovers: Vec<_> = tree(&fx.vault).into_keys().filter(|k| k.contains("wbtmp") || k.contains("wb-mv-pending")).collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}

#[test]
fn write_commands_refuse_without_side_effects() {
    let fx = Fx::new();
    mv_fixture(&fx);
    let v = fx.synced();
    let before = tree(&fx.vault);
    for (from, to, code) in [
        ("Projects/M/Prüf Thema", "Andere", exit::USAGE),
        ("Projects/M/Prüf Thema", "projects/m/ANDERE", exit::USAGE),
        ("Projects/M/Fehlt", "Neu", exit::NOT_FOUND),
        ("Projects/M/Prüf Thema", "Neu:Name", exit::USAGE),
    ] {
        assert_eq!(v.move_note(from, to).unwrap_err().exit_code(), code, "{from} -> {to}");
    }
    assert_eq!(tree(&fx.vault), before, "abgelehnt heißt: nichts geändert");
    // Nur-Lese-Modus: alle Schreibbefehle verweigern mit Exit 4
    let ro = Vault::open(OpenOptions::new(&fx.vault).index_dir(&fx.index).read_only(true)).unwrap();
    assert_eq!(ro.move_note("Projects/M/Prüf Thema", "Neu").unwrap_err().exit_code(), exit::READ_ONLY);
    assert_eq!(ro.append_journal(&NewJournalEntry { text: "x".into(), ..Default::default() }).unwrap_err().exit_code(), exit::READ_ONLY);
    assert_eq!(ro.new_note(&new_req("Neu", NoteType::Topic, Some("M"))).unwrap_err().exit_code(), exit::READ_ONLY);
    assert_eq!(tree(&fx.vault), before);
}

#[test]
fn mv_case_only_rename() {
    let fx = Fx::new();
    mv_fixture(&fx);
    let v = fx.synced();
    let r = v.move_note("Projects/M/Andere", "andere").unwrap();
    assert_eq!(r.to.as_str(), "Projects/M/andere.md");
    let names: Vec<String> = fs::read_dir(fx.vault.join("Projects/M")).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    assert!(names.contains(&"andere.md".to_string()) && !names.contains(&"Andere.md".to_string()), "{names:?}");
    assert!(file(&fx, "Projects/M/00 Overview.md").contains("- [[Projects/M/andere]] bleibt\n"));
}

/// Schreibbefehle nehmen auch den absoluten Pfad einer Notiz (Laufwerksbuchstabe oder `/…`).
#[test]
fn mv_accepts_absolute_source_path() {
    let fx = Fx::new();
    mv_fixture(&fx);
    let v = fx.synced();
    let abs = fx.vault.join("Projects").join("M").join("Andere.md");
    let r = v.move_note(&abs.to_string_lossy(), "Umbenannt").unwrap();
    assert_eq!(r.to.as_str(), "Projects/M/Umbenannt.md");
}
