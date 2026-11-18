//! Schreibweg (Vertrag §5, §14 A.2): `new`, `log`, `mv`. Jeder Befehl verweigert im Nur-Lese-Modus.
//!
//! - `new` legt exklusiv an (temporäre Datei, dann Hardlink auf das Ziel, der scheitert, wenn es
//!   das Ziel schon gibt): nie überschreiben, nie halbe Dateien.
//! - `log` hängt eine ganze Zeile unter exklusiver Dateisperre an (`File::lock`), mit einem
//!   einzigen Schreibaufruf und `fsync`; zwei gleichzeitige Schreiber verlieren keine Zeile.
//! - `mv` prüft erst alles Vorhersehbare (Ziel, Namen- und Pfadlänge, Ordner auf dem Weg),
//!   berechnet dann alle neuen Inhalte und schreibt sie samt Sicherung der alten als
//!   Zwischendateien nach `.wb-tmp/`, legt eine Liste der Schritte an (`.wb-mv-pending` in der
//!   Vault-Wurzel) und führt sie aus. Scheitert ein Schritt, nimmt es alle zurück. Bricht der
//!   Prozess ab, schließt das nächste Öffnen des Vaults (nicht im Nur-Lese-Modus) die Liste ab
//!   oder nimmt sie zurück; bricht er davor ab, ist nichts geändert (§19.1).
//! - Alle drei halten während des Schreibens eine Sperre für den ganzen Vault (`.wb-lock`,
//!   §19.2); ein zweiter Schreiber wartet kurz und endet sonst mit Exit 3.

use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use rusqlite::params;

use crate::error::{Error, Result};
use crate::model::*;
use crate::vault::Vault;

pub(crate) const PENDING: &str = ".wb-mv-pending";
/// Sperrdatei der Schreibbefehle (bleibt liegen; gesperrt wird die offene Datei, nicht ihr Dasein).
const LOCK: &str = ".wb-lock";
/// Ordner für Zwischendateien und Sicherungen.
const TMP_DIR: &str = ".wb-tmp";
/// Erste Zeile einer Liste dieser Fassung.
const HEADER: &str = "wb-mv 2";
/// Marke am Ende der Liste: zurücknehmen statt abschließen.
const UNDO: &str = "undo";
/// So lange wartet ein Schreibbefehl auf die Sperre.
const LOCK_WAIT_MS: u64 = 5000;

// ---------------- Datum ----------------

/// Heutiges Datum `JJJJ-MM-TT` in Ortszeit (unter Windows über `GetLocalTime`, sonst UTC).
/// `WB_TODAY` überschreibt den Wert (für Tests und Wiederholungen).
pub fn local_today() -> String {
    if let Ok(t) = std::env::var("WB_TODAY")
        && crate::query::days_from_date(&t).is_some()
    {
        return t;
    }
    let (y, m, d, _, _) = local_now();
    format!("{y:04}-{m:02}-{d:02}")
}

/// Uhrzeit `HH:MM` in Ortszeit.
pub fn local_time() -> String {
    let (_, _, _, h, min) = local_now();
    format!("{h:02}:{min:02}")
}

#[cfg(windows)]
fn local_now() -> (u16, u16, u16, u16, u16) {
    #[repr(C)]
    struct SystemTime {
        year: u16,
        month: u16,
        day_of_week: u16,
        day: u16,
        hour: u16,
        minute: u16,
        second: u16,
        millis: u16,
    }
    unsafe extern "system" {
        fn GetLocalTime(st: *mut SystemTime);
    }
    let mut st = SystemTime { year: 0, month: 0, day_of_week: 0, day: 0, hour: 0, minute: 0, second: 0, millis: 0 };
    // SAFETY: GetLocalTime füllt nur die übergebene Struktur (kernel32, immer vorhanden).
    unsafe { GetLocalTime(&mut st) };
    (st.year, st.month, st.day, st.hour, st.minute)
}

#[cfg(not(windows))]
fn local_now() -> (u16, u16, u16, u16, u16) {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let date = crate::query::date_of_mtime(secs);
    let y = date[0..4].parse().unwrap_or(1970);
    let m = date[5..7].parse().unwrap_or(1);
    let d = date[8..10].parse().unwrap_or(1);
    let rem = secs.rem_euclid(86_400);
    (y, m, d, (rem / 3600) as u16, (rem % 3600 / 60) as u16)
}

// ---------------- Namen ----------------

const RESERVED: [&str; 22] = [
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8", "com9", "lpt1", "lpt2", "lpt3", "lpt4",
    "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

/// Prüft einen Datei- oder Ordnernamen (ohne `.md`) für Windows.
fn check_name(name: &str) -> Result<()> {
    let bad = |m: String| Err(Error::Usage { message: m, hint: Some("ohne \\ / : * ? \" < > | und ohne Punkt oder Leerzeichen am Ende".into()) });
    if name.trim().is_empty() {
        return bad("leerer Name".into());
    }
    if let Some(c) = name.chars().find(|c| "\\/:*?\"<>|".contains(*c) || c.is_control()) {
        return bad(format!("Zeichen {c:?} ist in Dateinamen nicht erlaubt: {name}"));
    }
    if name.ends_with('.') || name.ends_with(' ') || name.starts_with(' ') {
        return bad(format!("Name beginnt oder endet mit Leerzeichen oder Punkt: {name:?}"));
    }
    if name.starts_with('.') {
        return bad(format!("versteckte Namen werden nicht indexiert: {name}"));
    }
    let stem = name.split('.').next().unwrap_or(name).to_ascii_lowercase();
    if RESERVED.contains(&stem.as_str()) {
        return bad(format!("reservierter Name unter Windows: {name}"));
    }
    Ok(())
}

/// Vault-relativer Notizpfad aus einer Eingabe (`/` oder `\`, `.md` optional), jeder Teil geprüft.
fn note_path_from(input: &str) -> Result<NotePath> {
    let s = input.trim().trim_matches('"').replace('\\', "/");
    let b = s.as_bytes();
    if s.starts_with('/') || (b.len() >= 2 && b[1] == b':') {
        return Err(Error::Usage {
            message: format!("Ziel außerhalb des Vaults: {input}"),
            hint: Some("Pfad relativ zur Vault-Wurzel angeben, z. B. Projects/<Projekt>/<Name>".into()),
        });
    }
    let s = s.trim_start_matches("./").trim_end_matches('/');
    let s = s.strip_suffix(".md").or_else(|| s.strip_suffix(".MD")).unwrap_or(s);
    for part in s.split('/') {
        check_name(part)?;
    }
    Ok(NotePath::new(format!("{s}.md")))
}

fn ensure_writable(v: &Vault, action: &str) -> Result<()> {
    if v.read_only {
        return Err(Error::ReadOnly { action: action.into(), hint: None });
    }
    Ok(())
}

fn write_file(p: &Path, bytes: &[u8]) -> Result<()> {
    let mut f = fs::File::create(p).map_err(|e| Error::io(p, e))?;
    f.write_all(bytes).map_err(|e| Error::io(p, e))?;
    f.sync_all().map_err(|e| Error::io(p, e))
}

/// Legt `dest` exklusiv an; `false`, wenn es die Datei schon gibt.
fn create_new(root: &Path, dest: &Path, bytes: &[u8]) -> Result<bool> {
    if let Some(dir) = dest.parent() {
        fs::create_dir_all(dir).map_err(|e| Error::io(dir, e))?;
    }
    let tmp = tmp_file(root, "new")?;
    write_file(&tmp, bytes)?;
    let r = match fs::hard_link(&tmp, dest) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
        // Dateisystem ohne Hardlinks: prüfen und umbenennen (nicht ganz wettlauffrei, aber nie halb).
        Err(_) if !dest.exists() => fs::rename(&tmp, dest).map(|_| true).map_err(|e| Error::io(dest, e)),
        Err(_) => Ok(false),
    };
    let _ = fs::remove_file(&tmp);
    r
}

// ---------------- new ----------------

/// Vorlage einer neuen Notiz.
fn template(n: &NewNote, title: &str, date: &str) -> String {
    let mut s = String::from("---\n");
    s.push_str(&format!("type: {}\n", n.note_type.as_str()));
    if let Some(st) = n.status {
        s.push_str(&format!("status: {st}\n"));
    }
    s.push_str(&format!("checked_on: {date}\n"));
    match n.note_type {
        NoteType::Overview => {
            let paths: Vec<String> = n.paths.iter().map(|p| format!("\"{}\"", p.replace('\\', "/").replace('"', ""))).collect();
            s.push_str(&format!("paths: [{}]\n", paths.join(", ")));
        }
        NoteType::Source => {
            if let Some(u) = &n.source_url {
                s.push_str(&format!("source_url: {u}\n"));
            }
        }
        _ => {}
    }
    s.push_str(&format!("---\n# {title}\n\n"));
    match n.note_type {
        NoteType::Overview => s.push_str("## Stand\n\n## Offene Punkte\n\n## Themen\n"),
        NoteType::Source => s.push_str("## Inhalt\n"),
        _ => s.push_str("## Stand\n\n## Offen\n"),
    }
    s
}

/// Ein Feld, das in genau einer Zeile von Frontmatter oder Überschrift landet, darf keinen
/// Zeilenumbruch tragen. `field` nennt den Schalter für die Fehlermeldung (`--url`, `--path`, `Titel`).
fn single_line(field: &str, value: &str) -> Result<()> {
    if value.contains(['\n', '\r']) {
        return Err(Error::Usage {
            message: format!("{field} enthält einen Zeilenumbruch"),
            hint: Some(format!("{field} in einer Zeile angeben")),
        });
    }
    Ok(())
}

/// Steuerzeichen beim Schreiben entfernen (§19.5): C0/C1, Richtungssteuerung (U+202A–202E,
/// U+2066–2069, U+200E/F); Zeilen- und Absatztrenner U+2028/9 werden zu Leerzeichen.
pub(crate) fn strip_controls(s: &str) -> String {
    s.chars()
        .filter_map(|c| match c {
            '\u{2028}' | '\u{2029}' => Some(' '),
            '\t' | '\n' => Some(c),
            c if c.is_control() || is_bidi_control(c) => None,
            c => Some(c),
        })
        .collect()
}

/// Unsichtbare Richtungssteuerung.
pub fn is_bidi_control(c: char) -> bool {
    matches!(c, '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
}

/// Datum, das es gibt (`2026-02-30` nicht).
fn valid_date(d: &str) -> bool {
    d.len() == 10 && crate::query::days_from_date(d).is_some_and(|n| crate::query::date_from_days(n) == d)
}

/// Beschreibung eines Links als Alias: eine Zeile, ohne die Zeichen, die einen Wikilink beenden
/// oder teilen (`|`, `[`, `]`).
fn link_text(s: &str) -> Option<String> {
    let t = strip_controls(s).split_whitespace().collect::<Vec<_>>().join(" ");
    let t = t.replace('|', "/").replace('[', "(").replace(']', ")");
    (!t.is_empty()).then_some(t)
}

/// Trägt eine neue Themennotiz in der Übersicht ihres Projekts ein (§23.1): `entry` als neue
/// Zeile ans Ende der Liste im ersten Abschnitt der Art „Themen“, in einem leeren Abschnitt
/// unter die Überschrift. Liefert die Zeile des Links; steht dort schon ein Link auf `target`,
/// dessen Zeile, ohne zu schreiben. `None`, wenn es Übersicht oder Abschnitt nicht gibt, der
/// Abschnitt Unterabschnitte oder Text ohne Liste hat, die Datei kein UTF-8 ist oder das
/// Schreiben scheitert; die Übersicht ist dann unverändert. Zeilenenden und BOM bleiben. Nur
/// unter der Schreibsperre aufrufen: geschrieben wird über eine Zwischendatei und Umbenennen.
fn link_in_overview(v: &Vault, overview: &NotePath, target: &str, entry: &str) -> Option<u32> {
    let p = crate::query::fs_path(v, overview);
    let bytes = fs::read(&p).ok()?;
    let bom = bytes.starts_with(b"\xEF\xBB\xBF");
    let text = std::str::from_utf8(&bytes[if bom { 3 } else { 0 }..]).ok()?;
    let (full, stem) = (path_key(target), path_key(target.rsplit('/').next().unwrap_or(target)));
    let known = crate::links::parse_wikilinks(text).into_iter().find(|l| {
        let k = path_key(&l.target);
        k == full || k == stem
    });
    if let Some(l) = known {
        return Some(l.line);
    }
    let all = crate::markdown::lines(text);
    let secs = crate::markdown::sections(&all, crate::frontmatter::split(text).body_line);
    let sec = secs.iter().find(|s| s.level > 0 && s.kind == SectionKind::Topics)?;
    if sec.line_end != sec.own_end {
        return None; // Unterabschnitte: in welche Gruppe der Link gehört, entscheidet der Schreiber
    }
    let indent = |l: &str| l.len() - l.trim_start().len();
    let is_item = |l: &str| ["- ", "* ", "+ "].iter().any(|b| l.trim_start().starts_with(b));
    // Letzte Listenzeile und die letzte mit der geringsten Einrückung (ihr Aufzählungszeichen gilt).
    let (mut last, mut top): (Option<u32>, Option<(usize, u32)>) = (None, None);
    let mut fence: Option<(u8, usize)> = None;
    for ln in sec.line_start + 1..=sec.own_end {
        let line = all[(ln - 1) as usize];
        if let Some((ch, len)) = crate::links::fence_marker(line) {
            match fence {
                None => fence = Some((ch, len)),
                Some((fc, fl)) if fc == ch && len >= fl => fence = None,
                _ => {}
            }
            continue;
        }
        if fence.is_none() && is_item(line) {
            last = Some(ln);
            if top.is_none_or(|(i, _)| indent(line) <= i) {
                top = Some((indent(line), ln));
            }
        }
    }
    let (after, bullet, empty) = match (last, top) {
        (Some(mut end), Some((ind, t))) => {
            // eingerückte Folgezeilen gehören zum letzten Listenpunkt
            while end < sec.own_end && all[end as usize].starts_with([' ', '\t']) && !all[end as usize].trim().is_empty() {
                end += 1;
            }
            (end, all[(t - 1) as usize][..ind + 2].to_string(), false)
        }
        _ if sec.own_end == sec.line_start => (sec.line_start, "- ".to_string(), true),
        _ => return None, // Text ohne Liste (Tabelle, Absatz)
    };
    let nl = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let item = entry.strip_prefix("- ").unwrap_or(entry);
    let mut out = String::with_capacity(text.len() + entry.len() + 8);
    if bom {
        out.push('\u{feff}');
    }
    for (i, raw) in text.split_inclusive('\n').enumerate() {
        out.push_str(raw);
        if i as u32 + 1 != after {
            continue;
        }
        if !raw.ends_with('\n') {
            out.push_str(nl);
        }
        if empty {
            out.push_str(nl);
        }
        out.push_str(&format!("{bullet}{item}{nl}"));
        if empty && all.get(after as usize).is_some_and(|l| !l.trim().is_empty()) {
            out.push_str(nl);
        }
    }
    let tmp = tmp_file(&v.root, "link").ok()?;
    if write_file(&tmp, out.as_bytes()).and_then(|_| rename_retry(&tmp, &p)).is_err() {
        let _ = fs::remove_file(&tmp);
        return None;
    }
    Some(after + 1 + u32::from(empty))
}

pub(crate) fn new_note(v: &Vault, n: &NewNote) -> Result<NewNoteReport> {
    let _lock = begin_write(v, "wb new")?;
    let n = &NewNote {
        title: n.title.as_deref().map(strip_controls),
        project: n.project.as_deref().map(strip_controls),
        paths: n.paths.iter().map(|p| strip_controls(p)).collect(),
        source_url: n.source_url.as_deref().map(strip_controls),
        ..n.clone()
    };
    if !matches!(n.note_type, NoteType::Overview | NoteType::Topic | NoteType::Source) {
        return Err(Error::Usage { message: format!("Notizart {} kann man nicht anlegen", n.note_type), hint: Some("overview | topic | source".into()) });
    }
    if n.summary.is_some() && n.note_type != NoteType::Topic {
        return Err(Error::Usage {
            message: format!("--summary gilt nur für Themennotizen, nicht für {}", n.note_type),
            hint: Some("die Beschreibung steht im Link der Übersicht: wb new <Titel> -p <Projekt> --summary <Satz>".into()),
        });
    }
    let summary = n.summary.as_deref().and_then(link_text);
    v.sync()?;
    let date = n.date.clone().unwrap_or_else(local_today);
    if !valid_date(&date) {
        return Err(Error::usage(format!("kein Datum: {date} (JJJJ-MM-TT, den Tag muss es geben)")));
    }
    // Projekt: vorhandenes Projekt in seiner Schreibweise, sonst nur für eine neue Übersicht.
    let project = match (&n.project, n.note_type) {
        (_, NoteType::Source) => None,
        (None, _) => return Err(Error::Usage { message: "Projekt fehlt".into(), hint: Some("-p <Projekt>".into()) }),
        (Some(p), t) => {
            let known = v.projects()?;
            let f = crate::text::fold(p);
            match known.iter().find(|k| crate::text::fold(&k.name) == f) {
                Some(k) => Some(k.name.clone()),
                None if t == NoteType::Overview => {
                    check_name(p.trim())?;
                    Some(p.trim().to_string())
                }
                None => {
                    let mut sugg: Vec<String> = known.iter().filter(|k| crate::text::fold(&k.name).contains(&f)).map(|k| k.name.clone()).collect();
                    sugg.truncate(5);
                    return Err(Error::NotFound { what: "Projekt", query: p.clone(), suggestions: sugg });
                }
            }
        }
    };
    let title = match (&n.title, n.note_type, &project) {
        (Some(t), _, _) if !t.trim().is_empty() => t.trim().to_string(),
        (_, NoteType::Overview, Some(p)) => p.clone(),
        _ => return Err(Error::Usage { message: "Titel fehlt".into(), hint: Some("wb new <Titel> -p <Projekt>".into()) }),
    };
    let rel = match (n.note_type, &project) {
        (NoteType::Overview, Some(p)) => format!("Projects/{p}/00 Overview"),
        (NoteType::Topic, Some(p)) => {
            check_name(&title)?;
            format!("Projects/{p}/{title}")
        }
        _ => {
            check_name(&title)?;
            format!("Sources/Web/{title}")
        }
    };
    let path = note_path_from(&rel)?;
    check_target(&v.root, &path)?;
    let full = v.root.join(path.as_str());
    let exists = |what: &str| Error::Usage { message: format!("{what} gibt es schon: {path}"), hint: Some(format!("wb read \"{}\"", path.without_ext())) };
    // Gleicher Name in anderer Schreibweise (Umlaute, Groß/Klein) wäre für Links mehrdeutig.
    let folded = crate::text::fold(path.without_ext());
    match crate::query::resolve(v, path.without_ext()) {
        Ok(found) if crate::text::fold(found.without_ext()) == folded => return Err(exists("Notiz")),
        Err(Error::Ambiguous { .. }) => return Err(exists("Notiz")),
        _ => {}
    }
    // Felder, die einzeln in eine Frontmatter- oder Überschriftzeile kommen (Audit 2026-11-18:
    // ein Zeilenumbruch darin schleuste zusätzliche Frontmatter-Schlüssel ein).
    single_line("Titel", &title)?;
    if let Some(u) = &n.source_url {
        single_line("--url", u)?;
    }
    for p in &n.paths {
        single_line("--path", p)?;
    }
    if !create_new(&v.root, &full, template(n, &title, &date).as_bytes())? {
        return Err(exists("Datei"));
    }
    let link = format!("[[{}]]", path.without_ext());
    let overview = project.as_ref().filter(|_| n.note_type != NoteType::Overview).map(|p| NotePath::new(format!("Projects/{p}/00 Overview.md")));
    let entry = overview.as_ref().map(|_| match &summary {
        Some(s) => format!("- [[{}|{s}]]", path.without_ext()),
        None => format!("- {link}"),
    });
    // Die Notiz ist angelegt; scheitert das Eintragen, bleibt es beim Hinweis (kein Fehler).
    let linked_line = match (&overview, &entry) {
        (Some(o), Some(e)) if !n.no_link => link_in_overview(v, o, path.without_ext(), e),
        _ => None,
    };
    v.sync()?;
    Ok(NewNoteReport { link, path, overview, entry, linked_line })
}

// ---------------- log ----------------

/// `HH:MM` mit 00–23 und 00–59.
fn valid_time(t: &str) -> bool {
    let b = t.as_bytes();
    t.len() == 5 && b[2] == b':' && b.iter().enumerate().all(|(i, c)| i == 2 || c.is_ascii_digit()) && t[..2] < *"24" && t[3..] < *"60"
}

pub(crate) fn append_journal(v: &Vault, e: &NewJournalEntry) -> Result<JournalWrite> {
    let _lock = begin_write(v, "wb log")?;
    let date = e.date.clone().unwrap_or_else(local_today);
    if !valid_date(&date) {
        return Err(Error::usage(format!("kein Datum: {date} (JJJJ-MM-TT, den Tag muss es geben)")));
    }
    if let Some(t) = &e.time
        && !valid_time(t)
    {
        return Err(Error::usage(format!("keine Uhrzeit: {t} (HH:MM)")));
    }
    let text: String = strip_controls(&e.text).split_whitespace().collect::<Vec<_>>().join(" ");
    if text.is_empty() {
        return Err(Error::Usage { message: "leerer Journaltext".into(), hint: Some("wb log \"Text\" oder Text über stdin".into()) });
    }
    let link = match &e.note {
        Some(a) => {
            v.sync()?;
            Some(format!("[[{}]]", crate::query::resolve_exact(v, a)?.without_ext()))
        }
        None => None,
    };
    let mut line = format!("- {date}");
    if let Some(t) = &e.time {
        line.push_str(&format!(" {t}"));
    }
    if let Some(l) = &link {
        line.push_str(&format!(" {l}"));
    }
    line.push_str(&format!(" {text}\n"));
    let file = NotePath::new(format!("Journal/{}.md", &date[..7]));
    let full = v.root.join(file.as_str());
    if let Some(d) = full.parent() {
        fs::create_dir_all(d).map_err(|e| Error::io(d, e))?;
    }
    let io = |err| Error::io(&full, err);
    let mut f = fs::OpenOptions::new().read(true).append(true).create(true).open(&full).map_err(io)?;
    f.lock().map_err(io)?;
    // Unter der Sperre: Ende prüfen (fehlender Zeilenumbruch nach Handbearbeitung), Zeilen zählen.
    let mut old = Vec::new();
    f.seek(SeekFrom::Start(0)).map_err(io)?;
    f.read_to_end(&mut old).map_err(io)?;
    let mut buf = Vec::with_capacity(line.len() + 1);
    if !old.is_empty() && !old.ends_with(b"\n") {
        buf.push(b'\n');
    }
    buf.extend_from_slice(line.as_bytes());
    f.write_all(&buf).map_err(io)?;
    f.sync_data().map_err(io)?;
    let line_no = old.iter().filter(|&&b| b == b'\n').count() as u32 + 1 + u32::from(!old.is_empty() && !old.ends_with(b"\n"));
    drop(f);
    Ok(JournalWrite { file, line: line_no, text: line.trim_end().to_string() })
}

// ---------------- Sperre, Zwischendateien ----------------

/// Sperre eines Schreibbefehls für den ganzen Vault (§19.2). Fällt mit dem Wert oder dem Prozess.
pub(crate) struct WriteLock {
    _file: fs::File,
}

fn lock_file(root: &Path) -> Result<fs::File> {
    let p = root.join(LOCK);
    fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(&p).map_err(|e| Error::io(&p, e))
}

/// Sperre ohne Warten; `None`, wenn ein anderer Schreibbefehl sie hält.
fn try_lock(root: &Path) -> Result<Option<WriteLock>> {
    let f = lock_file(root)?;
    match f.try_lock() {
        Ok(()) => Ok(Some(WriteLock { _file: f })),
        Err(fs::TryLockError::WouldBlock) => Ok(None),
        Err(fs::TryLockError::Error(e)) => Err(Error::io(root.join(LOCK), e)),
    }
}

fn lock_wait() -> Duration {
    let ms = if cfg!(debug_assertions) { std::env::var("WB_LOCK_WAIT_MS").ok().and_then(|x| x.parse().ok()) } else { None };
    Duration::from_millis(ms.unwrap_or(LOCK_WAIT_MS))
}

/// Sperre mit kurzem Warten; danach Exit 3 mit Hinweis, ohne etwas anzulegen.
fn lock(root: &Path, action: &str) -> Result<WriteLock> {
    let wait = lock_wait();
    let t0 = Instant::now();
    loop {
        if let Some(l) = try_lock(root)? {
            return Ok(l);
        }
        if t0.elapsed() >= wait {
            return Err(Error::Vault {
                message: format!("{action}: ein anderer Schreibbefehl läuft in diesem Vault (nach {} s aufgegeben)", wait.as_secs()),
                hint: Some(format!("später erneut ausführen; nichts wurde geändert (Sperre {})", root.join(LOCK).display())),
            });
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Zwischendatei im Ordner `.wb-tmp` der Vault-Wurzel: kurzer Name, unabhängig von der Länge
/// des Notiznamens, und auf demselben Laufwerk wie jede Notiz (Umbenennen bleibt atomar).
fn tmp_file(root: &Path, tag: &str) -> Result<PathBuf> {
    static SEQ: AtomicU32 = AtomicU32::new(0);
    let dir = root.join(TMP_DIR);
    fs::create_dir_all(&dir).map_err(|e| Error::io(&dir, e))?;
    let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0);
    Ok(dir.join(format!("{tag}-{}-{nonce}-{}", std::process::id(), SEQ.fetch_add(1, Ordering::Relaxed))))
}

/// Verwaiste Zwischendateien abgebrochener Schreibbefehle entfernen (§19.5): den Ordner
/// `.wb-tmp` und Reste älterer Fassungen (`.<Name>.md.wbtmp-…` neben den Notizen). Nur unter
/// der Sperre aufrufen und nur ohne ausstehende Liste.
fn cleanup_temps(root: &Path) {
    if let Ok(rd) = fs::read_dir(root.join(TMP_DIR)) {
        for e in rd.flatten() {
            let _ = fs::remove_file(e.path());
        }
    }
    let walk = walkdir::WalkDir::new(root).follow_links(false).into_iter().filter_entry(|e| {
        e.depth() == 0 || !e.file_type().is_dir() || !e.file_name().to_string_lossy().starts_with('.')
    });
    for e in walk.flatten() {
        let n = e.file_name().to_string_lossy();
        if e.file_type().is_file() && n.starts_with('.') && n.contains(".wbtmp-") {
            let _ = fs::remove_file(e.path());
        }
    }
}

/// Fehler der Schreibbefehle, solange ein unterbrochenes `mv` hängt (§19.1).
fn stuck_error(root: &Path, e: &Error) -> Error {
    Error::Vault {
        message: format!("unterbrochenes wb mv lässt sich weder abschließen noch zurücknehmen ({}): {e}", root.join(PENDING).display()),
        hint: Some(format!(
            "Ursache beheben (Datei in einem anderen Programm schließen, Schreibrechte prüfen), dann heilt `wb sync`; Sicherungen liegen in {}",
            root.join(TMP_DIR).display()
        )),
    }
}

/// Beginn jedes Schreibbefehls: Nur-Lese-Schutz, Sperre, hängendes `mv` abschließen oder
/// zurücknehmen, Reste wegräumen.
fn begin_write(v: &Vault, action: &str) -> Result<WriteLock> {
    ensure_writable(v, action)?;
    let l = lock(&v.root, action)?;
    if v.root.join(PENDING).exists() {
        recover(&v.root).map_err(|e| stuck_error(&v.root, &e))?;
    }
    cleanup_temps(&v.root);
    Ok(l)
}

/// Beim Öffnen (nicht im Nur-Lese-Modus): liegt eine Liste und hält niemand die Sperre, das
/// unterbrochene `mv` abschließen oder zurücknehmen. Gelingt beides nicht, kommt der Text der
/// `note:`-Zeile zurück; Lesebefehle laufen weiter.
pub(crate) fn recover_at_open(root: &Path) -> Option<String> {
    if !root.join(PENDING).exists() {
        return None;
    }
    let held = match try_lock(root) {
        Ok(Some(l)) => l,
        Ok(None) => return None, // ein `mv` läuft gerade
        Err(e) => return Some(format!("{e}")),
    };
    let r = recover(root);
    drop(held);
    match r {
        Ok(_) => None,
        Err(e) => Some(format!(
            "unterbrochenes wb mv hängt ({}): {e}; der Index kann einen halben Umzug zeigen, `wb sync` versucht es erneut",
            root.join(PENDING).display()
        )),
    }
}

// ---------------- mv ----------------

/// Sicherung einer Datei vor dem Überschreiben.
#[derive(Clone, Debug, PartialEq)]
enum Bak {
    /// Die Datei gab es vorher nicht.
    New,
    /// Ursprünglicher Inhalt in dieser Datei (relativ zur Vault-Wurzel).
    File(String),
    /// Liste einer älteren Fassung ohne Sicherung: nicht zurücknehmbar.
    Unknown,
}

impl Bak {
    fn field(&self) -> &str {
        match self {
            Bak::New => "-",
            Bak::File(f) => f,
            Bak::Unknown => "?",
        }
    }
    fn parse(s: &str) -> Bak {
        match s {
            "-" => Bak::New,
            "?" => Bak::Unknown,
            f => Bak::File(f.to_string()),
        }
    }
}

/// Ein Schritt der Ausführungsliste (Pfade relativ zur Vault-Wurzel, `/`).
#[derive(Debug)]
enum Op {
    /// Ordner anlegen, den es vorher nicht gab.
    Mkdir { dir: String },
    /// Temporäre Datei über das Ziel legen.
    Replace { tmp: String, dest: String, bak: Bak },
    /// Datei umbenennen (wenn die Quelle noch da und das Ziel frei ist).
    Rename { from: String, to: String },
    /// Datei löschen, falls vorhanden.
    Remove { path: String, bak: Bak },
}

impl Op {
    fn line(&self) -> String {
        match self {
            Op::Mkdir { dir } => format!("mkdir\t{dir}"),
            Op::Replace { tmp, dest, bak } => format!("replace\t{tmp}\t{dest}\t{}", bak.field()),
            Op::Rename { from, to } => format!("rename\t{from}\t{to}"),
            Op::Remove { path, bak } => format!("remove\t{path}\t{}", bak.field()),
        }
    }
    fn parse(l: &str) -> Option<Op> {
        let p: Vec<&str> = l.split('\t').collect();
        let s = |x: &&str| x.to_string();
        match p.as_slice() {
            ["mkdir", d] => Some(Op::Mkdir { dir: s(d) }),
            ["replace", t, d, b] => Some(Op::Replace { tmp: s(t), dest: s(d), bak: Bak::parse(b) }),
            ["replace", t, d] => Some(Op::Replace { tmp: s(t), dest: s(d), bak: Bak::Unknown }),
            ["rename", f, t] => Some(Op::Rename { from: s(f), to: s(t) }),
            ["remove", p, b] => Some(Op::Remove { path: s(p), bak: Bak::parse(b) }),
            ["remove", p] => Some(Op::Remove { path: s(p), bak: Bak::Unknown }),
            _ => None,
        }
    }
    /// Vorwärts; idempotent: zweimal ausführen schadet nicht.
    fn run(&self, root: &Path) -> Result<()> {
        match self {
            Op::Mkdir { dir } => {
                let d = root.join(dir);
                fs::create_dir_all(&d).map_err(|e| Error::io(&d, e))?;
            }
            Op::Replace { tmp, dest, .. } => {
                let (t, d) = (root.join(tmp), root.join(dest));
                if t.exists() {
                    rename_retry(&t, &d)?;
                }
            }
            Op::Rename { from, to } => {
                let (f, t) = (root.join(from), root.join(to));
                let case_only = crate::fsutil::compare_key(&f) == crate::fsutil::compare_key(&t);
                if f.exists() && (case_only || !t.exists()) {
                    rename_retry(&f, &t)?;
                }
            }
            Op::Remove { path, .. } => {
                let p = root.join(path);
                if p.exists() {
                    fs::remove_file(&p).map_err(|e| Error::io(&p, e))?;
                }
            }
        }
        Ok(())
    }
    /// Rückwärts; idempotent und auch für Schritte richtig, die nie liefen.
    fn undo(&self, root: &Path) -> Result<()> {
        let unknown = |p: &str| Error::Vault { message: format!("{p}: Liste einer älteren Fassung ohne Sicherung"), hint: None };
        match self {
            Op::Mkdir { dir } => {
                let _ = fs::remove_dir(root.join(dir)); // nur, wenn leer
            }
            Op::Replace { tmp, dest, bak } => {
                let (t, d) = (root.join(tmp), root.join(dest));
                match bak {
                    Bak::File(b) => {
                        let b = root.join(b);
                        if b.exists() {
                            rename_retry(&b, &d)?;
                        }
                    }
                    Bak::New => {
                        if d.exists() {
                            fs::remove_file(&d).map_err(|e| Error::io(&d, e))?;
                        }
                    }
                    Bak::Unknown => return Err(unknown(dest)),
                }
                if t.exists() {
                    fs::remove_file(&t).map_err(|e| Error::io(&t, e))?;
                }
            }
            Op::Rename { from, to } => {
                let (f, t) = (root.join(from), root.join(to));
                let case_only = crate::fsutil::compare_key(&f) == crate::fsutil::compare_key(&t);
                if t.exists() && (case_only || !f.exists()) {
                    rename_retry(&t, &f)?;
                }
            }
            Op::Remove { path, bak } => {
                let p = root.join(path);
                if !p.exists() {
                    match bak {
                        Bak::File(b) => rename_retry(&root.join(b), &p)?,
                        _ => return Err(unknown(path)),
                    }
                }
            }
        }
        Ok(())
    }
}

fn rename_retry(from: &Path, to: &Path) -> Result<()> {
    let mut last = None;
    for attempt in 0..20u64 {
        match fs::rename(from, to) {
            Ok(()) => return Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                last = Some(e);
                std::thread::sleep(Duration::from_millis(50 + attempt * 5));
            }
            Err(e) => return Err(Error::io(to, e)),
        }
    }
    Err(Error::io(to, last.unwrap_or_else(|| std::io::Error::other("rename fehlgeschlagen"))))
}

fn rel(root: &Path, p: &Path) -> String {
    p.strip_prefix(root).unwrap_or(p).to_string_lossy().replace('\\', "/")
}

/// Haken nur für Tests im Debug-Build: `WB_FAULT_MV=<art>:<n>` vor Schritt n.
/// `crash` beendet den Prozess (wie ein Abschuss), `fail` lässt den Schritt scheitern,
/// `stuck` lässt den Schritt scheitern und dazu jede Rücknahme und jede Wiederherstellung.
fn fault() -> Option<(String, usize)> {
    if !cfg!(debug_assertions) {
        return None;
    }
    let f = std::env::var("WB_FAULT_MV").ok()?;
    let (kind, n) = f.split_once(':')?;
    Some((kind.to_string(), n.parse().ok()?))
}

fn fault_error(what: &str) -> Error {
    Error::Vault { message: format!("simulierter Fehler ({what}, WB_FAULT_MV)"), hint: None }
}

fn stuck_fault() -> bool {
    fault().is_some_and(|f| f.0 == "stuck")
}

/// Rücknahmemarke ans Ende der Liste schreiben (danach nimmt die Wiederherstellung zurück).
fn mark_undo(root: &Path) -> Result<()> {
    let p = root.join(PENDING);
    let mut f = fs::OpenOptions::new().append(true).open(&p).map_err(|e| Error::io(&p, e))?;
    f.write_all(format!("{UNDO}\n").as_bytes()).map_err(|e| Error::io(&p, e))?;
    f.sync_all().map_err(|e| Error::io(&p, e))
}

fn undo_all(root: &Path, ops: &[Op]) -> Result<()> {
    if stuck_fault() {
        return Err(fault_error("Rücknahme"));
    }
    for op in ops.iter().rev() {
        op.undo(root)?;
    }
    Ok(())
}

/// Liste entfernen und Zwischendateien wegräumen (unter der Sperre).
fn finish(root: &Path) -> Result<()> {
    let list = root.join(PENDING);
    fs::remove_file(&list).map_err(|e| Error::io(&list, e))?;
    cleanup_temps(root);
    Ok(())
}

/// Unterbrochenes `mv` abschließen oder, wenn das scheitert oder die Liste die Rücknahmemarke
/// trägt, zurücknehmen. Nur unter der Sperre. Fehler nur, wenn beides scheitert.
pub(crate) fn recover(root: &Path) -> Result<bool> {
    let list = root.join(PENDING);
    let Ok(text) = fs::read_to_string(&list) else { return Ok(false) };
    let ops: Vec<Op> = text.lines().filter_map(Op::parse).collect();
    let undo = text.lines().any(|l| l == UNDO);
    if stuck_fault() {
        return Err(fault_error("Wiederherstellung"));
    }
    if !undo {
        let Err(fwd) = ops.iter().try_for_each(|op| op.run(root)) else {
            finish(root)?;
            return Ok(true);
        };
        mark_undo(root)?;
        return match undo_all(root, &ops) {
            Ok(()) => finish(root).map(|_| true),
            Err(back) => Err(Error::Vault { message: format!("vorwärts: {fwd}; zurück: {back}"), hint: None }),
        };
    }
    undo_all(root, &ops)?;
    finish(root)?;
    Ok(true)
}

/// Höchstlänge eines Datei- oder Ordnernamens in UTF-16-Einheiten (NTFS).
const NAME_MAX: usize = 255;
/// Höchstlänge des ganzen Pfads (MAX_PATH ohne Endzeichen): darüber scheitern viele Programme.
const PATH_MAX: usize = 259;

/// Vorhersehbares vor dem ersten Schreiben prüfen (§19.1): Länge von Namen und Pfad, Ordner
/// auf dem Weg, die es schon als Datei gibt. Liefert die fehlenden Ordner (von oben nach unten).
fn check_target(root: &Path, to: &NotePath) -> Result<Vec<String>> {
    let parts: Vec<&str> = to.as_str().split('/').collect();
    for (i, part) in parts.iter().enumerate() {
        let n = part.encode_utf16().count();
        if n > NAME_MAX {
            let what = if i + 1 == parts.len() { "Dateiname" } else { "Ordnername" };
            return Err(Error::Usage {
                message: format!("{what} zu lang: {n} Zeichen (höchstens {NAME_MAX}): {}…", part.chars().take(40).collect::<String>()),
                hint: Some("kürzeren Namen wählen".into()),
            });
        }
    }
    let full = root.join(to.as_str());
    let n = full.to_string_lossy().encode_utf16().count();
    if n > PATH_MAX {
        return Err(Error::Usage {
            message: format!("Pfad zu lang: {n} Zeichen (höchstens {PATH_MAX}): {to}"),
            hint: Some("kürzeren Namen oder flacheren Ordner wählen".into()),
        });
    }
    let mut missing = Vec::new();
    let mut acc = String::new();
    for part in &parts[..parts.len() - 1] {
        if !acc.is_empty() {
            acc.push('/');
        }
        acc.push_str(part);
        match fs::metadata(root.join(&acc)) {
            Ok(m) if m.is_dir() => {}
            Ok(_) => {
                return Err(Error::Usage { message: format!("Zielordner {acc} gibt es schon als Datei"), hint: Some("anderen Ordner wählen".into()) });
            }
            Err(_) => missing.push(acc.clone()),
        }
    }
    Ok(missing)
}

fn is_journal_path(p: &str) -> bool {
    p.len() > 8 && p.as_bytes()[..8].eq_ignore_ascii_case(b"journal/")
}

/// Quelle und Ziel eines `mv`, geprüft auf alles Vorhersehbare.
struct MvTarget {
    from: NotePath,
    to: NotePath,
    from_fs: PathBuf,
    to_fs: PathBuf,
    case_only: bool,
    missing_dirs: Vec<String>,
}

fn mv_target(v: &Vault, from_addr: &str, to_input: &str) -> Result<MvTarget> {
    let from = crate::query::resolve_exact(v, from_addr)?;
    if is_journal_path(from.as_str()) {
        return Err(Error::Usage {
            message: format!("Journaldateien lassen sich nicht umbenennen: {from}"),
            hint: Some("`wb log` schreibt nach Journal/JJJJ-MM.md; eine Notiz mit vollem Pfad oder vollem Namen angeben".into()),
        });
    }
    // Ziel: ohne `/` im selben Ordner, mit `/` am Ende in diesen Ordner, sonst ab Vault-Wurzel.
    let t = to_input.trim().trim_matches('"').replace('\\', "/");
    if t.trim_matches('/').trim().is_empty() {
        return Err(Error::Usage { message: "leeres Ziel".into(), hint: Some("wb mv <Notiz> <neuer Name | Ordner/ | Pfad ab Vault-Wurzel>".into()) });
    }
    let to_rel = if t.ends_with('/') {
        format!("{}{}", t, from.stem())
    } else if !t.contains('/') {
        let folder = from.folder();
        if folder.is_empty() { t.clone() } else { format!("{folder}/{t}") }
    } else {
        t.clone()
    };
    let to = note_path_from(&to_rel)?;
    if is_journal_path(to.as_str()) {
        return Err(Error::Usage { message: format!("Ziel im Journal-Ordner: {to}"), hint: Some("Notizen gehören nach Projects/ oder Sources/".into()) });
    }
    if to.key() == from.key() && to.as_str() == from.as_str() {
        return Err(Error::usage(format!("Ziel gleich Quelle: {from}")));
    }
    let from_fs = crate::query::fs_path(v, &from);
    let to_fs = v.root.join(to.as_str());
    let case_only = crate::fsutil::compare_key(&from_fs) == crate::fsutil::compare_key(&to_fs);
    if !case_only && (to_fs.exists() || crate::query::resolve(v, to.without_ext()).is_ok_and(|p| p.key() == to.key())) {
        return Err(Error::Usage { message: format!("Ziel gibt es schon: {to}"), hint: Some("anderen Namen wählen".into()) });
    }
    let missing_dirs = check_target(&v.root, &to)?;
    Ok(MvTarget { from, to, from_fs, to_fs, case_only, missing_dirs })
}

pub(crate) fn move_note(v: &Vault, from_addr: &str, to_input: &str) -> Result<MoveReport> {
    // Vorhersehbares zuerst ohne Sperre prüfen: ein Aufruf mit Exit 1 oder 2 legt nichts an,
    // auch keine Sperrdatei (§19.1). Hängt eine Liste, meldet `begin_write` das.
    ensure_writable(v, "wb mv")?;
    if !v.root.join(PENDING).exists() {
        v.sync()?;
        mv_target(v, from_addr, to_input)?;
    }
    let _lock = begin_write(v, "wb mv")?;
    // Unter der Sperre noch einmal: ein anderes `mv` kann inzwischen fertig sein (§19.2).
    v.sync()?;
    let MvTarget { from, to, from_fs, to_fs, case_only, missing_dirs } = mv_target(v, from_addr, to_input)?;
    let new_target = to.without_ext().to_string();

    // Eingehende Links aus dem Index: Quelle → (Zeile, Ziel wie geschrieben)
    let from_id: i64 = v.db.conn.query_row("SELECT id FROM notes WHERE path = ?1", params![from.as_str()], |r| r.get(0))?;
    let mut by_src: BTreeMap<String, HashSet<(u32, String)>> = BTreeMap::new();
    {
        let mut st = v.db.conn.prepare("SELECT n.path, l.line, l.target FROM links l JOIN notes n ON n.id = l.src WHERE l.dst = ?1 AND l.status = 'ok'")?;
        for r in st.query_map(params![from_id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, u32>(1)?, r.get::<_, String>(2)?)))? {
            let (src, line, target) = r?;
            if !target.trim().is_empty() {
                by_src.entry(src).or_default().insert((line, target));
            }
        }
    }
    // Journal: Links auf die alte Notiz (Journal steht nicht in der Linktabelle).
    let mut journals: Vec<String> = Vec::new();
    {
        let mut st = v.db.conn.prepare("SELECT path FROM notes WHERE is_journal = 1 ORDER BY path")?;
        for r in st.query_map([], |r| r.get::<_, String>(0))? {
            journals.push(r?);
        }
    }

    // Vorbereiten: neue Inhalte und Sicherungen in `.wb-tmp` (scheitert hier etwas, ist nichts geändert).
    let root = &v.root;
    let mut ops: Vec<Op> = missing_dirs.into_iter().map(|dir| Op::Mkdir { dir }).collect();
    let mut changed: Vec<(NotePath, u32)> = Vec::new();
    let prep = (|| -> Result<()> {
        // Liest eine Notiz und schreibt Links um: (Originalbytes, neuer Inhalt, Zahl).
        let edit = |src: &NotePath, rewrite: &dyn Fn(&WikiLink) -> bool| -> Result<Option<(Vec<u8>, String, u32)>> {
            let p = crate::query::fs_path(v, src);
            let bytes = fs::read(&p).map_err(|e| Error::io(&p, e))?;
            let bom = bytes.starts_with(b"\xEF\xBB\xBF");
            let text = String::from_utf8(bytes[if bom { 3 } else { 0 }..].to_vec())
                .map_err(|_| Error::Vault { message: format!("{src}: kein gültiges UTF-8, Links nicht umschreibbar"), hint: Some("Datei als UTF-8 speichern".into()) })?;
            let mut n = 0u32;
            let out = crate::links::rewrite_wikilinks(&text, |l| {
                if rewrite(l) {
                    n += 1;
                    Some(l.render_with_target(&new_target))
                } else {
                    None
                }
            });
            Ok((n > 0).then(|| (bytes, if bom { format!("\u{feff}{out}") } else { out }, n)))
        };
        // Neuer Inhalt und Sicherung des alten als Zwischendateien.
        let stage = |old: &[u8], content: &str| -> Result<(String, Bak)> {
            let tmp = tmp_file(root, "mv")?;
            write_file(&tmp, content.as_bytes())?;
            let bp = tmp_file(root, "bak")?;
            write_file(&bp, old)?;
            Ok((rel(root, &tmp), Bak::File(rel(root, &bp))))
        };
        let mut moved: Option<(Vec<u8>, String)> = None;
        for (src, set) in &by_src {
            let sp = NotePath::new(src.clone());
            if let Some((old, content, n)) = edit(&sp, &|l: &WikiLink| set.contains(&(l.line, l.target.clone())))? {
                changed.push((if sp.key() == from.key() { to.clone() } else { sp.clone() }, n));
                if sp.key() == from.key() {
                    moved = Some((old, content));
                } else {
                    let dest = crate::query::fs_path(v, &sp);
                    let (tmp, bak) = stage(&old, &content)?;
                    ops.push(Op::Replace { tmp, dest: rel(root, &dest), bak });
                }
            }
        }
        let from_key = from.without_ext().to_string();
        for j in &journals {
            let jp = NotePath::new(j.clone());
            let matches = |l: &WikiLink| {
                !l.target.trim().is_empty()
                    && (path_key(&l.target) == path_key(&from_key) || crate::query::resolve(v, &l.target).is_ok_and(|p| p.key() == from.key()))
            };
            if let Some((old, content, n)) = edit(&jp, &matches)? {
                changed.push((jp.clone(), n));
                let dest = crate::query::fs_path(v, &jp);
                let (tmp, bak) = stage(&old, &content)?;
                ops.push(Op::Replace { tmp, dest: rel(root, &dest), bak });
            }
        }
        match moved {
            // Die verschobene Notiz verlinkt sich selbst: neuer Inhalt ans Ziel, Quelle löschen.
            Some((old, content)) => {
                let (tmp, bak) = stage(&old, &content)?;
                if case_only {
                    ops.push(Op::Rename { from: rel(root, &from_fs), to: rel(root, &to_fs) });
                    ops.push(Op::Replace { tmp, dest: rel(root, &to_fs), bak });
                } else {
                    ops.push(Op::Replace { tmp, dest: rel(root, &to_fs), bak: Bak::New });
                    ops.push(Op::Remove { path: rel(root, &from_fs), bak });
                }
            }
            None => ops.push(Op::Rename { from: rel(root, &from_fs), to: rel(root, &to_fs) }),
        }
        let mut list = format!("{HEADER}\n");
        for o in &ops {
            list.push_str(&o.line());
            list.push('\n');
        }
        write_file(&root.join(PENDING), list.as_bytes())
    })();
    if let Err(e) = prep {
        let _ = fs::remove_file(root.join(PENDING));
        cleanup_temps(root);
        return Err(e);
    }
    // Ausführen. Scheitert ein Schritt, wird alles zurückgenommen; bricht der Prozess ab,
    // schließt das nächste Öffnen die Liste ab (oder nimmt sie zurück, siehe `recover`).
    let f = fault();
    let mut failed: Option<Error> = None;
    for (i, op) in ops.iter().enumerate() {
        if let Some((kind, n)) = &f
            && *n == i
        {
            if kind == "crash" {
                std::process::exit(99);
            }
            failed = Some(fault_error("Ausführung"));
            break;
        }
        if let Err(e) = op.run(root) {
            failed = Some(e);
            break;
        }
    }
    if let Some(e) = failed {
        let back = mark_undo(root).and_then(|_| undo_all(root, &ops)).and_then(|_| finish(root));
        let _ = v.sync();
        return Err(match back {
            Ok(()) => Error::Vault {
                message: format!("wb mv gescheitert und vollständig zurückgenommen: {e}"),
                hint: Some("nichts geändert; Ursache beheben (z. B. die Datei in einem anderen Programm schließen) und erneut ausführen".into()),
            },
            Err(b) => stuck_error(root, &Error::Vault { message: format!("vorwärts: {e}; zurück: {b}"), hint: None }),
        });
    }
    finish(root)?;
    v.sync()?;
    changed.sort_by(|a, b| a.0.cmp(&b.0));
    let links = changed.iter().map(|c| c.1).sum();
    Ok(MoveReport { from, to, links, changed })
}
