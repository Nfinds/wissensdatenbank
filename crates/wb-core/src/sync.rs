//! Inkrementeller Sync: Verzeichnislauf mit Größe und Änderungszeit, Inhalt nur bei
//! Abweichung lesen und hashen, geänderte Notizen in einer Transaktion neu indexieren.
//! Notizdateien werden nur gelesen, nie geschrieben.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::time::{Instant, UNIX_EPOCH};

use rusqlite::{OptionalExtension, Transaction, TransactionBehavior, params};
use walkdir::WalkDir;

use crate::error::{Error, Result};
use crate::index::join_list;
use crate::markdown;
use crate::model::{NotePath, NoteType, SyncReport, is_journal_path, path_key};
use crate::note::{ParsedNote, infer_paths, parse_note};
use crate::text::{Analyzer, fold};
use crate::vault::Vault;

pub(crate) struct ScanEntry {
    pub path: NotePath,
    pub fs_rel: String,
    pub size: u64,
    pub mtime_ns: i64,
}

/// Ergebnis eines Verzeichnislaufs.
pub(crate) struct Scan {
    pub entries: Vec<ScanEntry>,
    pub errors: Vec<(String, String)>,
    /// Dateien, die sich nur in der Unicode-Normalform vom Namen einer indizierten Notiz
    /// unterscheiden: (Notizpfad, nicht indizierter Dateiname). Sortiert.
    pub collisions: Vec<(String, String)>,
}

/// Alle `*.md` unter `root`, versteckte Ordner (`.wb`, `.obsidian`, `.git`, `.trash`) ausgenommen.
/// Sortiert nach Pfad (deterministisch). Ergeben zwei Dateinamen denselben NFC-Pfad, gilt
/// beständig der Dateiname, der als Bytefolge zuerst kommt (§15.5); der andere steht in `collisions`.
pub(crate) fn scan(root: &Path) -> Scan {
    let (mut entries, errors) = scan_raw(root);
    entries.sort_by(|a, b| a.path.cmp(&b.path).then_with(|| a.fs_rel.as_bytes().cmp(b.fs_rel.as_bytes())));
    let mut collisions = Vec::new();
    entries.dedup_by(|later, first| {
        if later.path == first.path {
            collisions.push((first.path.as_str().to_string(), later.fs_rel.clone()));
            true
        } else {
            false
        }
    });
    Scan { entries, errors, collisions }
}

fn scan_raw(root: &Path) -> (Vec<ScanEntry>, Vec<(String, String)>) {
    let mut out = Vec::new();
    let mut errors = Vec::new();
    let walker = WalkDir::new(root).follow_links(false).sort_by_file_name().into_iter().filter_entry(|e| {
        e.depth() == 0 || !e.file_name().to_string_lossy().starts_with('.')
    });
    for entry in walker {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                errors.push((e.path().map(|p| p.display().to_string()).unwrap_or_default(), e.to_string()));
                continue;
            }
        };
        if !entry.file_type().is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy();
        if !(name.len() > 3 && crate::model::has_md_ext(&name)) {
            continue;
        }
        let rel = match entry.path().strip_prefix(root) {
            Ok(r) => r,
            Err(_) => continue,
        };
        let Some(rel_s) = rel.to_str() else {
            errors.push((rel.display().to_string(), "Dateiname ist kein gültiges Unicode".into()));
            continue;
        };
        let fs_rel = rel_s.replace('\\', "/");
        let md = match entry.metadata() {
            Ok(m) => m,
            Err(e) => {
                errors.push((fs_rel, e.to_string()));
                continue;
            }
        };
        let mtime_ns = md
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_nanos() as i64)
            .unwrap_or(0);
        out.push(ScanEntry { path: NotePath::new(&fs_rel), fs_rel, size: md.len(), mtime_ns });
    }
    (out, errors)
}

/// Dateien, deren Änderungszeit nicht älter ist als der Beginn des letzten Syncs, werden auch bei
/// gleicher Größe und Zeit gehasht (§15.8); 2 s Spielraum für grobe Zeitstempel.
const RACY_SLACK_NS: i64 = 2_000_000_000;

fn now_ns() -> i64 {
    std::time::SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos() as i64).unwrap_or(0)
}

struct Stored {
    size: i64,
    mtime: i64,
    hash: Vec<u8>,
}

enum Kind {
    Add,
    Change,
    Touch,
}

struct Work {
    entry: ScanEntry,
    kind: Kind,
    hash: Vec<u8>,
    parsed: Option<ParsedNote>,
    text: String,
}

pub(crate) fn sync(v: &Vault, full: bool) -> Result<SyncReport> {
    let t0 = Instant::now();
    let full = full || v.db.fresh.get();
    let conn = &v.db.conn;
    let scan_start = now_ns();
    let Scan { entries, errors, collisions } = scan(&v.root);
    let mut report = SyncReport { scanned: entries.len() as u32, full_rebuild: full, errors, ..Default::default() };
    let meta = |k: &str| -> Result<Option<String>> {
        Ok(conn.query_row("SELECT value FROM meta WHERE key=?1", params![k], |r| r.get(0)).optional()?)
    };
    let last_sync: i64 = meta("last_sync_ns")?.and_then(|x| x.parse().ok()).unwrap_or(0);
    let racy_from = last_sync.saturating_sub(RACY_SLACK_NS);
    let collisions_s: Vec<String> = collisions.iter().map(|(a, b)| format!("{a}\t{b}")).collect();
    let collisions_s = join_list(&collisions_s);
    let collisions_changed = meta("name_collisions")?.unwrap_or_default() != collisions_s;
    let mut racy = 0u32;

    let mut stored: HashMap<String, Stored> = HashMap::new();
    {
        let mut st = conn.prepare_cached("SELECT 0, path, size, mtime_ns, hash FROM notes")?;
        let rows = st.query_map([], |r| {
            Ok((r.get::<_, String>(1)?, Stored { size: r.get(2)?, mtime: r.get(3)?, hash: r.get(4)? }))
        })?;
        for row in rows {
            let (p, s) = row?;
            stored.insert(p, s);
        }
    }
    let mut seen: HashSet<String> = entries.iter().map(|e| e.path.as_str().to_string()).collect();

    let mut work: Vec<Work> = Vec::new();
    for e in entries {
        let st = stored.get(e.path.as_str());
        let needs_read = full
            || match st {
                None => true,
                Some(s) => s.size != e.size as i64 || s.mtime != e.mtime_ns || e.mtime_ns >= racy_from,
            };
        if !needs_read {
            continue;
        }
        let same_stat = st.is_some_and(|s| s.size == e.size as i64 && s.mtime == e.mtime_ns);
        if same_stat && !full {
            racy += 1;
        }
        let full_path = v.root.join(&e.fs_rel);
        // War die Notiz bisher nicht leer, ist eine leere Datei eher ein laufendes Neuschreiben
        // durch ein anderes Programm: kurz abwarten statt den Zwischenstand zu indizieren.
        let was_full = st.is_some_and(|s| s.size > 0);
        let bytes = match crate::fsutil::read_settled(&full_path, |b| !b.is_empty() || !was_full) {
            Ok(b) => b,
            // Zwischen Verzeichnislauf und Lesen gelöscht oder umbenannt: wie nicht vorhanden.
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                seen.remove(e.path.as_str());
                continue;
            }
            Err(err) => {
                report.errors.push((e.fs_rel.clone(), err.to_string()));
                continue;
            }
        };
        let hash = blake3::hash(&bytes).as_bytes().to_vec();
        let kind = match st {
            None => Kind::Add,
            Some(_) if full => Kind::Add,
            Some(s) if s.hash == hash && same_stat => continue,
            Some(s) if s.hash == hash => Kind::Touch,
            Some(_) => Kind::Change,
        };
        let (parsed, text) = match kind {
            Kind::Touch => (None, String::new()),
            _ => {
                let p = parse_note(&e.path, &bytes);
                let (t, _, _) = crate::note::decode(&bytes);
                (Some(p), t)
            }
        };
        work.push(Work { entry: e, kind, hash, parsed, text });
    }

    // Nur zur Entscheidung, ob geschrieben werden muss; die Ids entstehen in der Schreibtransaktion.
    let any_removed = !full && stored.keys().any(|p| !seen.contains(p));
    if work.is_empty() && !any_removed && !full && !collisions_changed && racy == 0 {
        report.duration_ms = t0.elapsed().as_millis() as u64;
        return Ok(report);
    }

    let tx = Transaction::new_unchecked(conn, TransactionBehavior::Immediate)?;
    if full {
        tx.execute_batch(
            "DELETE FROM sec_fts; DELETE FROM sec_tri; DELETE FROM secrets; DELETE FROM journal; DELETE FROM links; DELETE FROM sections; DELETE FROM notes;",
        )?;
    } else {
        // Löschliste erst hier: ein anderer Prozess kann den Index inzwischen neu aufgebaut
        // und die Ids neu vergeben haben (§15.8).
        let removed: Vec<i64> = {
            let mut st = tx.prepare("SELECT id, path FROM notes ORDER BY path")?;
            let rows = st.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
            let mut out = Vec::new();
            for row in rows {
                let (id, p) = row?;
                if !seen.contains(&p) {
                    out.push(id);
                }
            }
            out
        };
        for id in &removed {
            delete_note(&tx, *id)?;
            report.removed += 1;
        }
    }
    let analyzer = Analyzer::new();
    for w in &work {
        // Ein anderer Prozess kann denselben Stand schon eingetragen haben.
        let current: Option<(i64, Vec<u8>)> = tx
            .query_row("SELECT id, hash FROM notes WHERE path = ?1", params![w.entry.path.as_str()], |r| Ok((r.get(0)?, r.get(1)?)))
            .optional()?;
        if matches!(w.kind, Kind::Touch) || matches!(&current, Some((_, h)) if *h == w.hash) {
            tx.execute(
                "UPDATE notes SET size=?1, mtime_ns=?2, fs_path=?3 WHERE path=?4",
                params![w.entry.size as i64, w.entry.mtime_ns, w.entry.fs_rel, w.entry.path.as_str()],
            )?;
            report.touched += 1;
            continue;
        }
        if let Some((id, _)) = current {
            delete_note(&tx, id)?;
            report.changed += 1;
        } else {
            report.added += 1;
        }
        insert_note(&tx, &analyzer, w)?;
    }
    resolve_links(&tx)?;
    tx.execute("INSERT OR REPLACE INTO meta(key, value) VALUES('core_version', ?1)", params![crate::VERSION])?;
    tx.execute("INSERT OR REPLACE INTO meta(key, value) VALUES('last_sync_ns', ?1)", params![scan_start.to_string()])?;
    if collisions_changed {
        tx.execute("INSERT OR REPLACE INTO meta(key, value) VALUES('name_collisions', ?1)", params![collisions_s])?;
    }
    tx.commit()?;
    v.db.fresh.set(false);
    report.duration_ms = t0.elapsed().as_millis() as u64;
    Ok(report)
}

fn delete_note(tx: &Transaction, id: i64) -> Result<()> {
    tx.execute("DELETE FROM sec_fts WHERE rowid IN (SELECT id FROM sections WHERE note_id = ?1)", params![id])?;
    tx.execute("DELETE FROM sec_tri WHERE rowid IN (SELECT id FROM sections WHERE note_id = ?1)", params![id])?;
    tx.execute("DELETE FROM notes WHERE id = ?1", params![id])?;
    Ok(())
}

fn insert_note(tx: &Transaction, an: &Analyzer, w: &Work) -> Result<()> {
    let p = w.parsed.as_ref().expect("geparst");
    let journal = is_journal_path(&p.path);
    let mut flags = Vec::new();
    if p.bom {
        flags.push("bom".to_string());
    }
    if p.crlf {
        flags.push("crlf".to_string());
    }
    if p.invalid_utf8 {
        flags.push("invalid-utf8".to_string());
    }
    let fm_problems: Vec<String> = match &p.frontmatter {
        crate::FrontmatterState::Present { fm, .. } => fm.problems.iter().map(|x| format!("{}:{}:{}", x.code, x.line, x.message)).collect(),
        crate::FrontmatterState::Missing => vec!["fm-missing:1:kein Frontmatter".into()],
        crate::FrontmatterState::Unclosed => vec!["fm-unclosed:1:Frontmatter ohne schließendes ---".into()],
    };
    let inferred = if p.note_type == NoteType::Overview { infer_paths(&w.text) } else { Vec::new() };
    tx.execute(
        "INSERT INTO notes(path, path_key, stem_key, fs_path, size, mtime_ns, hash, title, ntype, type_raw, project, status, state,
           checked_on, summary, aliases, paths, inferred_paths, lines, tokens, fm_problems, flags, is_journal)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23)",
        params![
            p.path.as_str(),
            p.path.key(),
            path_key(p.path.stem()),
            w.entry.fs_rel,
            w.entry.size as i64,
            w.entry.mtime_ns,
            w.hash,
            p.title,
            p.note_type.as_str(),
            p.type_raw,
            p.path.project(),
            p.status,
            p.state,
            p.checked_on,
            p.summary,
            join_list(&p.aliases),
            join_list(&p.paths),
            join_list(&inferred),
            p.lines,
            p.tokens,
            join_list(&fm_problems),
            join_list(&flags),
            journal as i64,
        ],
    )?;
    let note_id = tx.last_insert_rowid();

    let all = markdown::lines(&w.text);
    let mut sections = p.sections.clone();
    if sections.is_empty() {
        // Leere Notiz oder nur Frontmatter: ein Ersatzabschnitt, damit Titel und Pfad auffindbar sind.
        let l = p.body_line.min(p.lines.max(1));
        sections.push(crate::Section {
            index: 0,
            level: 0,
            title: String::new(),
            kind: crate::SectionKind::Other,
            line_start: l,
            line_end: l.max(p.lines.min(l)),
            own_end: l,
            tokens: 0,
            own_tokens: 0,
        });
    }
    let title_idx = an.index_text(&p.title);
    let alias_idx = an.index_text(&p.aliases.join(" "));
    let path_plain = p.path.without_ext().replace(['/', '_', '-'], " ");
    let path_idx = an.index_text(&path_plain);
    let mut ins_sec = tx.prepare_cached(
        "INSERT INTO sections(note_id, idx, level, title, kind, line_start, line_end, own_end, tokens, own_tokens, anchor, body, list_items)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
    )?;
    let mut ins_fts = tx.prepare_cached("INSERT INTO sec_fts(rowid, title, heading, aliases, path, body) VALUES(?1,?2,?3,?4,?5,?6)")?;
    let mut ins_tri = tx.prepare_cached("INSERT INTO sec_tri(rowid, text) VALUES(?1,?2)")?;
    for (i, s) in sections.iter().enumerate() {
        let body = if all.is_empty() || s.line_start as usize > all.len() { String::new() } else { markdown::section_body(&all, s, false) };
        let anchor = markdown::anchor(&sections, i);
        ins_sec.execute(params![
            note_id,
            s.index,
            s.level,
            s.title,
            s.kind.as_str(),
            s.line_start,
            s.line_end,
            s.own_end,
            s.tokens,
            s.own_tokens,
            anchor,
            body,
            markdown::count_list_items(&body),
        ])?;
        let sid = tx.last_insert_rowid();
        if journal {
            continue;
        }
        ins_fts.execute(params![sid, title_idx, an.index_text(&s.title), alias_idx, path_idx, an.index_text(&body)])?;
        let tri = fold(&format!("{} {} {} {} {}", p.title, s.title, p.aliases.join(" "), path_plain, body));
        ins_tri.execute(params![sid, tri])?;
    }
    let mut ins_secret = tx.prepare_cached("INSERT INTO secrets(note_id, line, class) VALUES(?1,?2,?3)")?;
    for (line, class) in crate::secrets::scan(&w.text) {
        ins_secret.execute(params![note_id, line, class])?;
    }
    let mut ins_link = tx.prepare_cached(
        "INSERT INTO links(src, line, target, heading, alias, dst, status, candidates) VALUES(?1,?2,?3,?4,?5,NULL,'dead',NULL)",
    )?;
    if !journal {
        for l in &p.links {
            ins_link.execute(params![note_id, l.line, l.target, l.heading, l.alias])?;
        }
    } else {
        let mut ins_j = tx.prepare_cached(
            "INSERT INTO journal(note_id, line, date, time, project, target, target_key, text) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
        )?;
        for e in crate::journal::parse_journal(&p.path, &w.text) {
            ins_j.execute(params![note_id, e.line, e.date, e.time, e.project, e.target, e.target.as_deref().map(path_key), e.text])?;
        }
    }
    Ok(())
}

/// Alle Linkziele gegen den Bestand auflösen: exakter Pfad, sonst eindeutiges Pfadende
/// bzw. eindeutiger Dateiname; leeres Ziel (`[[#Abschnitt]]`) zeigt auf die Quelle.
fn resolve_links(tx: &Transaction) -> Result<()> {
    let mut by_key: HashMap<String, i64> = HashMap::new();
    let mut by_stem: HashMap<String, Vec<(String, i64)>> = HashMap::new();
    let mut paths: HashMap<i64, String> = HashMap::new();
    {
        let mut st = tx.prepare("SELECT id, path_key, stem_key, path FROM notes ORDER BY path")?;
        let rows = st.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?)))?;
        for row in rows {
            let (id, key, stem, path) = row?;
            by_key.insert(key.clone(), id);
            by_stem.entry(stem).or_default().push((key, id));
            paths.insert(id, path);
        }
    }
    let links: Vec<(i64, i64, String, Option<i64>, String)> = {
        let mut st = tx.prepare("SELECT id, src, target, dst, status FROM links")?;
        let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))?;
        rows.collect::<std::result::Result<_, _>>()?
    };
    let mut upd = tx.prepare_cached("UPDATE links SET dst=?1, status=?2, candidates=?3 WHERE id=?4")?;
    for (id, src, target, old_dst, old_status) in links {
        let t = crate::links::normalize_target(&target);
        let (dst, status, cands): (Option<i64>, &str, Option<String>) = if t.is_empty() {
            (Some(src), "ok", None)
        } else if let Some(&d) = by_key.get(&t) {
            (Some(d), "ok", None)
        } else {
            let last = t.rsplit('/').next().unwrap_or(&t);
            let suffix = format!("/{t}");
            let c: Vec<i64> = by_stem
                .get(last)
                .map(|v| v.iter().filter(|(k, _)| !t.contains('/') || k.ends_with(&suffix)).map(|x| x.1).collect())
                .unwrap_or_default();
            match c.len() {
                0 => (None, "dead", None),
                1 => (Some(c[0]), "ok", None),
                _ => {
                    let names: Vec<String> = c.iter().filter_map(|i| paths.get(i).cloned()).collect();
                    (None, "ambiguous", Some(join_list(&names)))
                }
            }
        };
        if old_dst != dst || old_status != status || status == "ambiguous" {
            upd.execute(params![dst, status, cands, id])?;
        }
    }
    Ok(())
}

#[allow(dead_code)]
pub(crate) fn not_found(what: &'static str, q: &str) -> Error {
    Error::NotFound { what, query: q.to_string(), suggestions: Vec::new() }
}
