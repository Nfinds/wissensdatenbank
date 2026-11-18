//! Abfragen über den Index: Notizen, Adressen, Projekte, Links, Journal, Kontext.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use rusqlite::{OptionalExtension, Row, params};

use crate::error::{Error, Result};
use crate::fsutil::compare_key;
use crate::index::split_list;
use crate::markdown;
use crate::model::*;
use crate::text::fold;
use crate::vault::{Vault, split_address};

const META_COLS: &str = "n.path, n.title, n.ntype, n.type_raw, n.project, n.status, n.state, n.checked_on, n.summary, n.aliases,
  n.mtime_ns, n.size, n.lines, n.tokens,
  (SELECT count(*) FROM links l WHERE l.src = n.id),
  (SELECT count(DISTINCT l.src) FROM links l WHERE l.dst = n.id AND l.src <> n.id),
  n.fm_problems, n.id";

fn row_meta(r: &Row) -> rusqlite::Result<(NoteMeta, i64)> {
    let ntype: String = r.get(2)?;
    let fm: String = r.get(16)?;
    Ok((
        NoteMeta {
            path: NotePath::new(r.get::<_, String>(0)?),
            title: r.get(1)?,
            note_type: NoteType::parse(&ntype).unwrap_or(NoteType::Other),
            type_raw: r.get(3)?,
            project: r.get(4)?,
            status: r.get(5)?,
            state: r.get(6)?,
            checked_on: r.get(7)?,
            summary: r.get(8)?,
            aliases: split_list(&r.get::<_, String>(9)?),
            mtime: r.get::<_, i64>(10)? / 1_000_000_000,
            bytes: r.get::<_, i64>(11)? as u64,
            lines: r.get(12)?,
            tokens: r.get(13)?,
            links_out: r.get(14)?,
            links_in: r.get(15)?,
            frontmatter_problems: split_list(&fm),
        },
        r.get(17)?,
    ))
}

pub(crate) fn all_meta(v: &Vault) -> Result<Vec<(NoteMeta, i64)>> {
    let sql = format!("SELECT {META_COLS} FROM notes n ORDER BY n.path");
    let mut st = v.db.conn.prepare_cached(&sql)?;
    let rows = st.query_map([], row_meta)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

fn eq_name(a: &str, b: &str) -> bool {
    fold(a) == fold(b)
}

pub(crate) fn notes(v: &Vault, f: &NoteFilter) -> Result<Vec<NoteMeta>> {
    let prefix = f.prefix.as_deref().map(|p| p.replace('\\', "/").to_lowercase());
    Ok(all_meta(v)?
        .into_iter()
        .map(|x| x.0)
        .filter(|m| f.include_journal || m.note_type != NoteType::Journal)
        .filter(|m| f.note_type.is_none_or(|t| t == m.note_type))
        .filter(|m| f.project.as_deref().is_none_or(|p| m.project.as_deref().is_some_and(|mp| eq_name(mp, p))))
        .filter(|m| prefix.as_deref().is_none_or(|p| m.path.as_str().to_lowercase().starts_with(p)))
        .collect())
}

pub(crate) fn meta(v: &Vault, path: &NotePath) -> Result<NoteMeta> {
    let sql = format!("SELECT {META_COLS} FROM notes n WHERE n.path = ?1");
    let mut st = v.db.conn.prepare_cached(&sql)?;
    st.query_row(params![path.as_str()], row_meta)
        .optional()?
        .map(|x| x.0)
        .ok_or_else(|| Error::NotFound { what: "Notiz", query: path.to_string(), suggestions: Vec::new() })
}

pub(crate) fn note_id(v: &Vault, path: &NotePath) -> Result<i64> {
    v.db.conn
        .query_row("SELECT id FROM notes WHERE path = ?1", params![path.as_str()], |r| r.get(0))
        .optional()?
        .ok_or_else(|| Error::NotFound { what: "Notiz", query: path.to_string(), suggestions: Vec::new() })
}

/// Adresse mit Abschnitt auflösen. Enthält sie `#`, gilt zuerst der längste Teil vor einem `#`,
/// der genau als Pfad existiert (Dateiname mit `#`), sonst die Trennung am ersten `#` (§15.5).
pub(crate) fn resolve_address(v: &Vault, address: &str) -> Result<(NotePath, Option<String>)> {
    let a = address.trim();
    let a = a.strip_prefix("[[").and_then(|x| x.strip_suffix("]]")).unwrap_or(a);
    let a = a.split('|').next().unwrap_or(a);
    if a.contains('#') {
        let mut cuts: Vec<usize> = a.match_indices('#').map(|(i, _)| i).collect();
        cuts.push(a.len());
        for &i in cuts.iter().rev() {
            let (name, rest) = (&a[..i], a.get(i + 1..).unwrap_or(""));
            if name.is_empty() {
                continue;
            }
            if let Some(p) = exact_path(v, name)? {
                return Ok((p, Some(rest.to_string()).filter(|s| !s.is_empty())));
            }
        }
    }
    let (_, sec) = split_address(a);
    Ok((resolve(v, a)?, sec.map(str::to_string)))
}

/// Notiz genau unter diesem Pfad (Groß/Klein und Normalform egal) oder mit eindeutigem
/// Pfadschluss bzw. Dateinamen; ohne unscharfes Raten.
fn exact_path(v: &Vault, raw: &str) -> Result<Option<NotePath>> {
    let a = raw.trim().trim_matches('"').replace('\\', "/");
    let key = path_key(&a);
    if key.is_empty() {
        return Ok(None);
    }
    let p: Option<String> =
        v.db.conn.query_row("SELECT path FROM notes WHERE path_key = ?1 ORDER BY path LIMIT 1", params![key], |r| r.get(0)).optional()?;
    if p.is_some() {
        return Ok(p.map(NotePath::new));
    }
    // Eindeutiger Pfadschluss oder Dateiname (`C# Grundlagen` ohne Ordner), nichts Unscharfes.
    let mut st = v.db.conn.prepare_cached("SELECT path, path_key, stem_key FROM notes ORDER BY path")?;
    let all: Vec<(String, String, String)> = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<rusqlite::Result<_>>()?;
    let suffix = format!("/{key}");
    let by_suffix: Vec<&String> = all.iter().filter(|x| x.1.ends_with(&suffix)).map(|x| &x.0).collect();
    if by_suffix.len() == 1 {
        return Ok(Some(NotePath::new(by_suffix[0])));
    }
    if by_suffix.is_empty() && !key.contains('/') {
        let by_stem: Vec<&String> = all.iter().filter(|x| x.2 == key).map(|x| &x.0).collect();
        if by_stem.len() == 1 {
            return Ok(Some(NotePath::new(by_stem[0])));
        }
    }
    Ok(None)
}

pub(crate) fn resolve(v: &Vault, address: &str) -> Result<NotePath> {
    let (raw, _) = split_address(address);
    let mut a = raw.trim().trim_matches('"').replace('\\', "/");
    // Absoluter Pfad im Vault → relativ
    let root_key = compare_key(&v.root);
    if a.len() > 2 && a.as_bytes()[1] == b':' {
        let ak = compare_key(std::path::Path::new(&a));
        if let Some(rest) = ak.strip_prefix(&format!("{root_key}/")) {
            a = rest.to_string();
        }
    }
    let key = path_key(&a);
    if key.is_empty() {
        return Err(Error::usage("leere Notizadresse"));
    }
    let conn = &v.db.conn;
    let exact: Option<String> =
        conn.query_row("SELECT path FROM notes WHERE path_key = ?1 ORDER BY path LIMIT 1", params![key], |r| r.get(0)).optional()?;
    if let Some(p) = exact {
        return Ok(NotePath::new(p));
    }
    let all: Vec<(String, String, String, String)> = {
        let mut st = conn.prepare_cached("SELECT path, path_key, stem_key, title FROM notes ORDER BY path")?;
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?.collect::<rusqlite::Result<_>>()?
    };
    let pick = |c: Vec<&String>, how: &str| -> Option<Result<NotePath>> {
        match c.len() {
            0 => None,
            1 => Some(Ok(NotePath::new(c[0]))),
            _ => {
                let _ = how;
                Some(Err(Error::Ambiguous {
                    what: "Notiz",
                    query: raw.to_string(),
                    candidates: c.iter().take(8).map(|p| crate::model::strip_md(p).to_string()).collect(),
                }))
            }
        }
    };
    let suffix = format!("/{key}");
    if let Some(r) = pick(all.iter().filter(|x| x.1.ends_with(&suffix)).map(|x| &x.0).collect(), "suffix") {
        return r;
    }
    if !key.contains('/')
        && let Some(r) = pick(all.iter().filter(|x| x.2 == key).map(|x| &x.0).collect(), "stem") {
            return r;
        }
    let fkey = fold(&key);
    if let Some(r) = pick(all.iter().filter(|x| fold(&x.3) == fkey).map(|x| &x.0).collect(), "title") {
        return r;
    }
    if let Some(r) = pick(all.iter().filter(|x| fold(&x.1) == fkey).map(|x| &x.0).collect(), "folded") {
        return r;
    }
    let contains: Vec<&String> = all.iter().filter(|x| fold(&x.1).contains(&fkey)).map(|x| &x.0).collect();
    if contains.len() == 1 {
        return Ok(NotePath::new(contains[0]));
    }
    Err(Error::NotFound {
        what: "Notiz",
        query: raw.to_string(),
        suggestions: contains.iter().take(5).map(|p| crate::model::strip_md(p).to_string()).collect(),
    })
}

/// Genaue Adresse für Schreibbefehle (§19.3): voller Pfad (ab Vault-Wurzel, mit oder ohne `.md`)
/// oder voller Dateiname, Groß/Klein, Normalform und Umlaut-Umschrift egal; nie ein Namensanfang,
/// ein Pfadschluss oder ein Titel. Ein `#Abschnitt` am Ende wird ignoriert. Findet die unscharfe
/// Auflösung eine Notiz, steht sie als Vorschlag im Fehler.
pub(crate) fn resolve_exact(v: &Vault, address: &str) -> Result<NotePath> {
    let a = address.trim();
    let a = a.strip_prefix("[[").and_then(|x| x.strip_suffix("]]")).unwrap_or(a);
    let a = a.split('|').next().unwrap_or(a);
    // Erst die ganze Eingabe (Dateiname mit `#`), dann ohne `#Abschnitt`.
    let mut tries = vec![a];
    let (cut, _) = split_address(a);
    if cut != a {
        tries.push(cut);
    }
    let all: Vec<(String, String, String)> = {
        let mut st = v.db.conn.prepare_cached("SELECT path, path_key, stem_key FROM notes ORDER BY path")?;
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<rusqlite::Result<_>>()?
    };
    let root_key = compare_key(&v.root);
    for raw in tries {
        let mut s = raw.trim().trim_matches('"').replace('\\', "/");
        if s.len() > 2 && s.as_bytes()[1] == b':' {
            let ak = compare_key(std::path::Path::new(&s));
            if let Some(rest) = ak.strip_prefix(&format!("{root_key}/")) {
                s = rest.to_string();
            }
        }
        let key = path_key(&s);
        if key.is_empty() {
            return Err(Error::usage("leere Notizadresse"));
        }
        let fkey = fold(&key);
        let pick = |c: Vec<&String>| -> Option<Result<NotePath>> {
            match c.len() {
                0 => None,
                1 => Some(Ok(NotePath::new(c[0]))),
                _ => Some(Err(Error::Ambiguous {
                    what: "Notiz",
                    query: raw.to_string(),
                    candidates: c.iter().take(8).map(|p| crate::model::strip_md(p).to_string()).collect(),
                })),
            }
        };
        let bare = !key.contains('/');
        // Erst genau, dann gefaltet (Groß/Klein, Umlaute).
        for folded in [false, true] {
            let hit = |path_key: &String, stem_key: &String| {
                if folded {
                    fold(path_key) == fkey || (bare && fold(stem_key) == fkey)
                } else {
                    *path_key == key || (bare && *stem_key == key)
                }
            };
            if let Some(r) = pick(all.iter().filter(|x| hit(&x.1, &x.2)).map(|x| &x.0).collect()) {
                return r;
            }
        }
    }
    let suggestions = match resolve(v, address) {
        Ok(p) => vec![format!("\"{}\"", p.without_ext())],
        Err(Error::Ambiguous { candidates, .. }) => candidates.into_iter().map(|c| format!("\"{c}\"")).collect(),
        Err(Error::NotFound { suggestions, .. }) => suggestions.into_iter().map(|c| format!("\"{c}\"")).collect(),
        Err(e) => return Err(e),
    };
    Err(Error::NotFound { what: "Notiz (genaue Adresse: voller Pfad oder voller Name)", query: address.trim().to_string(), suggestions })
}

pub(crate) fn fs_path(v: &Vault, p: &NotePath) -> PathBuf {
    let rel: Option<String> =
        v.db.conn.query_row("SELECT fs_path FROM notes WHERE path = ?1", params![p.as_str()], |r| r.get(0)).optional().ok().flatten();
    v.root.join(rel.unwrap_or_else(|| p.as_str().to_string()))
}

/// Datei lesen; weicht der Inhalt vom Index ab, kurz warten, bis er sich setzt
/// ([`crate::fsutil::read_settled`]).
fn read_settled(v: &Vault, path: &NotePath, file: &Path) -> std::io::Result<Vec<u8>> {
    let indexed: Option<Vec<u8>> =
        v.db.conn.query_row("SELECT hash FROM notes WHERE path = ?1", params![path.as_str()], |r| r.get(0)).optional().ok().flatten();
    crate::fsutil::read_settled(file, |b| indexed.as_deref() == Some(blake3::hash(b).as_bytes().as_slice()))
}

pub(crate) fn note(v: &Vault, path: &NotePath) -> Result<Note> {
    let file = fs_path(v, path);
    let bytes = read_settled(v, path, &file).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            Error::NotFound { what: "Notizdatei", query: path.to_string(), suggestions: Vec::new() }
        } else {
            Error::io(&file, e)
        }
    })?;
    let parsed = crate::note::parse_note(path, &bytes);
    let (text, _, _) = crate::note::decode(&bytes);
    let meta = match meta(v, path) {
        Ok(m) => m,
        Err(_) => NoteMeta {
            path: path.clone(),
            title: parsed.title.clone(),
            note_type: parsed.note_type,
            type_raw: parsed.type_raw.clone(),
            project: path.project().map(str::to_string),
            status: parsed.status.clone(),
            state: parsed.state.clone(),
            checked_on: parsed.checked_on.clone(),
            summary: parsed.summary.clone(),
            aliases: parsed.aliases.clone(),
            mtime: 0,
            bytes: parsed.bytes,
            lines: parsed.lines,
            tokens: parsed.tokens,
            links_out: parsed.links.len() as u32,
            links_in: 0,
            frontmatter_problems: parsed.frontmatter.problem_codes(),
        },
    };
    Ok(Note { meta, frontmatter: parsed.frontmatter, sections: parsed.sections, text })
}

pub(crate) fn find_section<'a>(note: &'a Note, spec: &str) -> Result<&'a Section> {
    let s = spec.trim();
    let nf = || Error::NotFound {
        what: "Abschnitt",
        query: format!("{}#{s}", note.meta.path.without_ext()),
        suggestions: note.sections.iter().filter(|x| !x.title.is_empty()).take(12).map(|x| x.title.clone()).collect(),
    };
    if let Some(n) = s.strip_prefix(['L', 'l']).map(|r| r.split('-').next().unwrap_or(r)).and_then(|r| r.parse::<u32>().ok()) {
        // Abschnitt, der an der Zeile beginnt, sonst der innerste, der sie enthält
        if let Some(x) = note.sections.iter().find(|x| x.line_start == n) {
            return Ok(x);
        }
        return note
            .sections
            .iter()
            .filter(|x| x.line_start <= n && n <= x.line_end)
            .max_by_key(|x| (x.level, x.line_start))
            .ok_or_else(nf);
    }
    if let Ok(i) = s.parse::<usize>() {
        return note.sections.iter().find(|x| x.index as usize == i).ok_or_else(nf);
    }
    let f = fold(s);
    if f.is_empty() {
        return Err(nf());
    }
    let ft = |x: &Section| fold(&x.title);
    note.sections
        .iter()
        .find(|x| ft(x) == f)
        .or_else(|| note.sections.iter().find(|x| ft(x).starts_with(&f)))
        .or_else(|| note.sections.iter().find(|x| ft(x).contains(&f)))
        .ok_or_else(nf)
}

// ---------- Datum ----------

/// Tage seit 1970-01-01 für `JJJJ-MM-TT`.
pub(crate) fn days_from_date(s: &str) -> Option<i64> {
    let y: i64 = s.get(0..4)?.parse().ok()?;
    let m: i64 = s.get(5..7)?.parse().ok()?;
    let d: i64 = s.get(8..10)?.parse().ok()?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let y2 = if m <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146097 + doe - 719468)
}

pub(crate) fn date_from_days(z: i64) -> String {
    let z = z + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

/// Datum der Änderungszeit (UTC).
pub fn date_of_mtime(secs: i64) -> String {
    date_from_days(secs.div_euclid(86400))
}

fn monday_of(days: i64) -> i64 {
    // 1970-01-01 war ein Donnerstag
    days - (days + 3).rem_euclid(7)
}

// ---------- Projekte ----------

pub(crate) fn projects(v: &Vault) -> Result<Vec<ProjectInfo>> {
    let metas = all_meta(v)?;
    let findings = crate::check::all_findings(v)?;
    let conn = &v.db.conn;
    let mut open_items: BTreeMap<String, u32> = BTreeMap::new();
    {
        let mut st = conn.prepare_cached(
            "SELECT n.project, sum(s.list_items) FROM sections s JOIN notes n ON n.id = s.note_id
             WHERE s.kind = 'open' AND n.project IS NOT NULL GROUP BY n.project",
        )?;
        for r in st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))? {
            let (p, c) = r?;
            open_items.insert(p, c as u32);
        }
    }
    // Aktivität: Journalzeilen je Projekt und Woche, die letzten 12 Wochen bis zum jüngsten Eintrag.
    let mut weekly: BTreeMap<String, BTreeMap<i64, u32>> = BTreeMap::new();
    let mut max_day = None::<i64>;
    {
        let mut st = conn.prepare_cached("SELECT project, date, count(*) FROM journal WHERE project IS NOT NULL GROUP BY project, date")?;
        for r in st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?)))? {
            let (p, d, c) = r?;
            if let Some(days) = days_from_date(&d) {
                max_day = Some(max_day.map_or(days, |m: i64| m.max(days)));
                *weekly.entry(fold(&p)).or_default().entry(monday_of(days)).or_default() += c as u32;
            }
        }
    }
    let weeks: Vec<i64> = match max_day {
        Some(m) => (0..12).rev().map(|i| monday_of(m) - 7 * i).collect(),
        None => Vec::new(),
    };
    let mut by_project: BTreeMap<String, Vec<&NoteMeta>> = BTreeMap::new();
    for (m, _) in &metas {
        if let Some(p) = &m.project {
            by_project.entry(p.clone()).or_default().push(m);
        }
    }
    let mut out = Vec::new();
    for (name, notes) in by_project {
        let overview = notes.iter().find(|m| m.note_type == NoteType::Overview && m.path.is_overview()).or_else(|| {
            notes.iter().find(|m| m.note_type == NoteType::Overview)
        });
        let (paths, inferred) = match overview {
            Some(o) => project_paths(v, &o.path)?,
            None => (Vec::new(), false),
        };
        let mut f: BTreeMap<String, u32> = BTreeMap::new();
        for x in findings.iter().filter(|x| x.path.project() == Some(name.as_str())) {
            *f.entry(x.code.clone()).or_default() += 1;
        }
        let wk = weekly.get(&fold(&name));
        out.push(ProjectInfo {
            overview: overview.map(|o| o.path.clone()),
            notes: notes.len() as u32,
            bytes: notes.iter().map(|m| m.bytes).sum(),
            tokens: notes.iter().map(|m| m.tokens as u64).sum(),
            last_modified: notes.iter().map(|m| m.mtime).max().unwrap_or(0),
            last_checked: notes.iter().filter_map(|m| m.checked_on.clone()).max(),
            status: overview.and_then(|o| o.status.clone()),
            open_items: open_items.get(&name).copied().unwrap_or(0),
            findings: f,
            activity: weeks
                .iter()
                .map(|w| WeekActivity { week: date_from_days(*w), count: wk.and_then(|m| m.get(w)).copied().unwrap_or(0) })
                .collect(),
            paths,
            paths_inferred: inferred,
            name,
        });
    }
    Ok(out)
}

/// Projektwurzeln einer Übersicht: Frontmatter `paths`, sonst aus dem Text abgeleitet.
pub(crate) fn project_paths(v: &Vault, overview: &NotePath) -> Result<(Vec<String>, bool)> {
    let (p, i): (String, String) = v.db.conn.query_row(
        "SELECT paths, inferred_paths FROM notes WHERE path = ?1",
        params![overview.as_str()],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let strong: Vec<String> = split_list(&p).iter().filter_map(|x| crate::note::normalize_disk_path(x)).collect();
    if !strong.is_empty() {
        return Ok((strong, false));
    }
    Ok((split_list(&i), true))
}

// ---------- Links ----------

pub(crate) fn links(v: &Vault, path: &NotePath) -> Result<NoteLinks> {
    let id = note_id(v, path)?;
    let conn = &v.db.conn;
    let status = |s: &str, dst: Option<String>, cands: Option<String>| match s {
        "ok" => LinkStatus::Ok(NotePath::new(dst.unwrap_or_default())),
        "ambiguous" => LinkStatus::Ambiguous(split_list(&cands.unwrap_or_default()).into_iter().map(NotePath::new).collect()),
        _ => LinkStatus::Dead,
    };
    let mut out = NoteLinks::default();
    {
        let mut st = conn.prepare_cached(
            "SELECT l.line, l.target, l.heading, l.alias, l.status, d.path, l.candidates FROM links l
             LEFT JOIN notes d ON d.id = l.dst WHERE l.src = ?1 ORDER BY l.line, l.id",
        )?;
        for r in st.query_map(params![id], |r| {
            Ok((r.get::<_, u32>(0)?, r.get::<_, String>(1)?, r.get(2)?, r.get(3)?, r.get::<_, String>(4)?, r.get(5)?, r.get(6)?))
        })? {
            let (line, target, heading, alias, s, dst, c) = r?;
            out.outgoing.push(LinkInfo { source: path.clone(), line, target, heading, alias, status: status(&s, dst, c) });
        }
    }
    {
        let mut st = conn.prepare_cached(
            "SELECT s.path, l.line, l.target, l.heading, l.alias FROM links l JOIN notes s ON s.id = l.src
             WHERE l.dst = ?1 AND l.src <> ?1 ORDER BY s.path, l.line, l.id",
        )?;
        for r in st.query_map(params![id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, u32>(1)?, r.get::<_, String>(2)?, r.get(3)?, r.get(4)?))
        })? {
            let (src, line, target, heading, alias) = r?;
            out.incoming.push(LinkInfo {
                source: NotePath::new(src),
                line,
                target,
                heading,
                alias,
                status: LinkStatus::Ok(path.clone()),
            });
        }
    }
    Ok(out)
}

/// Alle ausgehenden Links des Vaults in einer Abfrage, nach Quelle, Zeile sortiert.
pub(crate) fn all_links(v: &Vault) -> Result<Vec<LinkInfo>> {
    let mut st = v.db.conn.prepare_cached(
        "SELECT s.path, l.line, l.target, l.heading, l.alias, l.status, d.path, l.candidates FROM links l
         JOIN notes s ON s.id = l.src LEFT JOIN notes d ON d.id = l.dst ORDER BY s.path, l.line, l.id",
    )?;
    let rows = st.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, u32>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, Option<String>>(3)?,
            r.get::<_, Option<String>>(4)?,
            r.get::<_, String>(5)?,
            r.get::<_, Option<String>>(6)?,
            r.get::<_, Option<String>>(7)?,
        ))
    })?;
    let mut out = Vec::new();
    for r in rows {
        let (src, line, target, heading, alias, s, dst, cands) = r?;
        let status = match s.as_str() {
            "ok" => LinkStatus::Ok(NotePath::new(dst.unwrap_or_default())),
            "ambiguous" => LinkStatus::Ambiguous(split_list(&cands.unwrap_or_default()).into_iter().map(NotePath::new).collect()),
            _ => LinkStatus::Dead,
        };
        out.push(LinkInfo { source: NotePath::new(src), line, target, heading, alias, status });
    }
    Ok(out)
}

// ---------- Journal ----------

pub(crate) fn journal(v: &Vault, f: &JournalFilter) -> Result<Vec<JournalEntry>> {
    let target_key = f.note.as_ref().map(|n| resolve(v, n).map(|p| p.key()).unwrap_or_else(|_| path_key(n)));
    let mut st = v.db.conn.prepare_cached(
        "SELECT j.date, j.time, j.target, j.project, j.text, n.path, j.line FROM journal j JOIN notes n ON n.id = j.note_id
         WHERE (?1 IS NULL OR j.date >= ?1) AND (?2 IS NULL OR j.date <= ?2) AND (?3 IS NULL OR j.target_key = ?3)
         ORDER BY j.date DESC, coalesce(j.time, '') DESC, n.path DESC, j.line DESC",
    )?;
    let rows = st.query_map(params![f.since, f.until, target_key], |r| {
        Ok(JournalEntry {
            date: r.get(0)?,
            time: r.get(1)?,
            target: r.get(2)?,
            project: r.get(3)?,
            text: r.get(4)?,
            file: NotePath::new(r.get::<_, String>(5)?),
            line: r.get(6)?,
        })
    })?;
    let mut out = Vec::new();
    for r in rows {
        let e = r?;
        if let Some(p) = &f.project
            && !e.project.as_deref().is_some_and(|ep| eq_name(ep, p)) {
                continue;
            }
        out.push(e);
        if f.limit > 0 && out.len() >= f.limit {
            break;
        }
    }
    Ok(out)
}

pub(crate) fn activity(v: &Vault, f: &ActivityFilter) -> Result<Vec<DayActivity>> {
    let mut st = v.db.conn.prepare_cached(
        "SELECT date, project, count(*) FROM journal WHERE (?1 IS NULL OR date >= ?1) AND (?2 IS NULL OR date <= ?2)
         GROUP BY date, project ORDER BY date, project",
    )?;
    let rows = st.query_map(params![f.since, f.until], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?, r.get::<_, i64>(2)?)))?;
    let mut agg: BTreeMap<(String, Option<String>), u32> = BTreeMap::new();
    for r in rows {
        let (d, p, c) = r?;
        match &f.project {
            Some(want) => {
                if p.as_deref().is_some_and(|x| eq_name(x, want)) {
                    *agg.entry((d, Some(want.clone()))).or_default() += c as u32;
                }
            }
            None => *agg.entry((d, p)).or_default() += c as u32,
        }
    }
    Ok(agg.into_iter().map(|((date, project), count)| DayActivity { date, project, count }).collect())
}

// ---------- Kontext ----------

fn find_project(v: &Vault, projects: &[ProjectInfo], target: &ContextTarget) -> Result<(usize, String)> {
    match target {
        ContextTarget::Project(name) => {
            let f = fold(name.trim());
            let names: Vec<String> = projects.iter().map(|p| fold(&p.name)).collect();
            let pick = |idx: Vec<usize>, how: &str| -> Option<Result<(usize, String)>> {
                match idx.len() {
                    0 => None,
                    1 => Some(Ok((idx[0], how.to_string()))),
                    _ => Some(Err(Error::Ambiguous {
                        what: "Projekt",
                        query: name.clone(),
                        candidates: idx.iter().map(|&i| projects[i].name.clone()).collect(),
                    })),
                }
            };
            let norm = |s: &str| s.replace(['_', '-', ' '], "");
            let fnorm = norm(&f);
            type Try<'a> = (&'static str, Box<dyn Fn(&str) -> bool + 'a>);
            let tries: [Try; 4] = [
                ("name", Box::new(|n: &str| n == f)),
                ("name", Box::new(|n: &str| norm(n) == fnorm)),
                ("name-prefix", Box::new(|n: &str| norm(n).starts_with(&fnorm))),
                ("name-contains", Box::new(|n: &str| norm(n).contains(&fnorm))),
            ];
            for (how, t) in tries.iter() {
                if let Some(r) = pick((0..names.len()).filter(|&i| t(&names[i])).collect(), how) {
                    return r;
                }
            }
            // Alias der Übersicht
            let mut hits = Vec::new();
            for (i, p) in projects.iter().enumerate() {
                if let Some(o) = &p.overview
                    && let Ok(m) = meta(v, o)
                        && (m.aliases.iter().any(|a| fold(a) == f) || fold(&m.title) == f) {
                            hits.push(i);
                        }
            }
            if let Some(r) = pick(hits, "alias") {
                return r;
            }
            Err(Error::NotFound {
                what: "Projekt",
                query: name.clone(),
                suggestions: projects.iter().map(|p| p.name.clone()).filter(|n| fold(n).chars().next() == f.chars().next()).take(6).collect(),
            })
        }
        ContextTarget::Cwd(dir) => {
            let key = compare_key(std::path::Path::new(dir));
            let root = compare_key(&v.root);
            if let Some(rest) = key.strip_prefix(&format!("{root}/projects/")) {
                let name = rest.split('/').next().unwrap_or(rest);
                if let Some(i) = projects.iter().position(|p| p.name.to_lowercase() == name) {
                    return Ok((i, "vault-folder".into()));
                }
            }
            // Der längste passende Pfad gewinnt, gleich ob aus dem Frontmatter oder aus dem Text
            // abgeleitet (Vertrag §17.3). Derselbe Pfad in zwei Projekten bleibt mehrdeutig.
            let mut best_len = 0usize;
            let mut best: Vec<(usize, bool, &String)> = Vec::new();
            for (i, p) in projects.iter().enumerate() {
                for path in &p.paths {
                    let pk = compare_key(std::path::Path::new(path));
                    if !(key == pk || key.starts_with(&format!("{pk}/"))) {
                        continue;
                    }
                    let len = pk.len();
                    if len > best_len {
                        best_len = len;
                        best.clear();
                    }
                    if len == best_len {
                        best.push((i, !p.paths_inferred, path));
                    }
                }
            }
            let mut names: Vec<usize> = best.iter().map(|b| b.0).collect();
            names.dedup();
            match names.len() {
                0 => Err(Error::NotFound { what: "Projekt zum Ordner", query: dir.clone(), suggestions: Vec::new() }),
                1 => {
                    let (i, strong, path) = best.iter().max_by_key(|b| b.1).copied().expect("ein Treffer");
                    let how = if strong { format!("paths:{path}") } else { format!("paths~:{path}") };
                    Ok((i, how))
                }
                _ => Err(Error::Ambiguous {
                    what: "Projekt",
                    query: dir.clone(),
                    candidates: names.iter().map(|&i| projects[i].name.clone()).collect(),
                }),
            }
        }
    }
}

pub(crate) fn context(v: &Vault, target: &ContextTarget) -> Result<ProjectContext> {
    let projects = projects(v)?;
    let (i, how) = find_project(v, &projects, target)?;
    let project = projects[i].clone();
    let mut state = Vec::new();
    let mut open = Vec::new();
    let mut overview_meta = None;
    let mut overview_aliases: BTreeMap<String, String> = BTreeMap::new();
    if let Some(op) = &project.overview {
        let n = note(v, op)?;
        let all = markdown::lines(&n.text);
        for (idx, s) in n.sections.iter().enumerate() {
            let bucket = match s.kind {
                SectionKind::State => &mut state,
                SectionKind::Open => &mut open,
                _ => continue,
            };
            bucket.push(SectionText {
                address: format!("{}#{}", op.without_ext(), markdown::anchor(&n.sections, idx)),
                title: s.title.clone(),
                kind: s.kind,
                line_start: s.line_start,
                line_end: s.line_end,
                tokens: s.tokens,
                text: markdown::section_body(&all, s, true),
            });
        }
        // Über die aufgelösten Links, damit auch Kurzlinks wie [[Thema|Beschreibung]] zählen.
        for l in links(v, op)?.outgoing {
            if let (Some(a), LinkStatus::Ok(dst)) = (l.alias, l.status) {
                overview_aliases.entry(dst.key()).or_insert(a);
            }
        }
        overview_meta = Some(n.meta);
    }
    // Abschnittsarten je Notiz
    let mut kinds: BTreeMap<String, BTreeSet<SectionKind>> = BTreeMap::new();
    {
        let mut st = v.db.conn.prepare_cached(
            "SELECT n.path, s.kind FROM sections s JOIN notes n ON n.id = s.note_id WHERE n.project = ?1 AND s.kind <> 'other'",
        )?;
        for r in st.query_map(params![project.name], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
            let (p, k) = r?;
            if let Some(k) = SectionKind::parse(&k) {
                kinds.entry(p).or_default().insert(k);
            }
        }
    }
    let (superseded, current): (Vec<NoteMeta>, Vec<NoteMeta>) = notes(v, &NoteFilter { project: Some(project.name.clone()), ..Default::default() })?
        .into_iter()
        .filter(|m| Some(&m.path) != project.overview.as_ref())
        .partition(|m| m.is_superseded());
    let superseded = superseded.len() as u32;
    let mut topics: Vec<TopicLine> = current
        .into_iter()
        .map(|m| {
            let date = m.checked_on.clone().or_else(|| Some(date_of_mtime(m.mtime)));
            // Ein Alias, der nur Titel oder Dateinamen wiederholt, beschreibt nichts.
            let alias = overview_aliases.get(&m.path.key()).cloned().filter(|a| {
                let f = crate::text::fold(a);
                f != crate::text::fold(&m.title) && f != crate::text::fold(m.path.stem())
            });
            // Beschreibung im Themenlink der Übersicht ist kuratiert und schlägt den ersten Satz.
            let summary = alias.or(m.summary.clone());
            TopicLine {
                kinds: kinds.get(m.path.as_str()).map(|s| s.iter().copied().collect()).unwrap_or_default(),
                title: m.title.clone(),
                summary,
                date,
                tokens: m.tokens,
                path: m.path,
            }
        })
        .collect();
    topics.sort_by(|a, b| b.date.cmp(&a.date).then_with(|| a.path.cmp(&b.path)));
    let journal = journal(v, &JournalFilter { project: Some(project.name.clone()), limit: 50, ..Default::default() })?;
    Ok(ProjectContext { project, matched_by: how, overview: overview_meta, state, open, topics, superseded, journal })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates() {
        assert_eq!(days_from_date("1970-01-01"), Some(0));
        assert_eq!(date_from_days(days_from_date("2026-11-09").unwrap()), "2026-11-09");
        // 2026-11-09 ist ein Montag
        let d = days_from_date("2026-11-12").unwrap();
        assert_eq!(date_from_days(monday_of(d)), "2026-11-09");
    }
}
