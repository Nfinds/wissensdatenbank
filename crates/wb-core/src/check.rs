//! Prüfregeln (Ausgangspunkt: ein früheres Lint-Skript für das Wiki, erweitert um Frontmatter,
//! Mehrdeutigkeit, Größe, Verwaisung und Alter).

use std::collections::BTreeSet;

use rusqlite::params;

use crate::error::Result;
use crate::index::split_list;
use crate::model::*;
use crate::vault::Vault;

/// Übersicht länger als so viele Zeilen → Befund (wie `wiki_lint.py`).
pub const MAX_OVERVIEW_LINES: u32 = 100;
/// Notiz größer als so viele geschätzte Token → Befund.
pub const MAX_NOTE_TOKENS: u32 = 8000;

/// Alle Befunde ohne Filter, zwischengespeichert bis sich der Index ändert
/// (`projects()` und `context()` brauchen sie bei jedem Aufruf).
pub(crate) fn all_findings(v: &Vault) -> Result<std::rc::Rc<Vec<Finding>>> {
    let conn = &v.db.conn;
    let key = (conn.query_row("PRAGMA data_version", [], |r| r.get::<_, i64>(0))?, conn.total_changes());
    if let Some((k, f)) = v.db.findings.borrow().as_ref()
        && *k == key
    {
        return Ok(f.clone());
    }
    let f = std::rc::Rc::new(check(v, &CheckOptions::default())?);
    *v.db.findings.borrow_mut() = Some((key, f.clone()));
    Ok(f)
}

pub(crate) fn check(v: &Vault, opts: &CheckOptions) -> Result<Vec<Finding>> {
    let metas = crate::query::all_meta(v)?;
    let prefix = opts.prefix.as_deref().map(|p| p.replace('\\', "/").to_lowercase());
    let in_scope = |p: &NotePath| prefix.as_deref().is_none_or(|x| p.as_str().to_lowercase().starts_with(x));
    let mut out = Vec::new();
    let push = |out: &mut Vec<Finding>, path: &NotePath, line: u32, code: &str, sev: Severity, msg: String, hint: &str| {
        out.push(Finding { path: path.clone(), line, code: code.into(), severity: sev, message: msg, hint: hint.into() });
    };
    let today = opts.today.as_deref().and_then(crate::query::days_from_date);
    let stale = opts.stale_days.unwrap_or(60) as i64;
    let mut projects_with_overview = BTreeSet::new();
    let mut projects = BTreeSet::new();
    for (m, _) in &metas {
        if let Some(p) = &m.project {
            projects.insert(p.clone());
            if m.path.is_overview() {
                projects_with_overview.insert(p.clone());
            }
        }
    }
    for (m, _) in &metas {
        if !in_scope(&m.path) || m.note_type == NoteType::Journal {
            continue;
        }
        for p in &m.frontmatter_problems {
            let mut it = p.splitn(3, ':');
            let code = it.next().unwrap_or("fm");
            let line = it.next().and_then(|l| l.parse().ok()).unwrap_or(1);
            let msg = it.next().unwrap_or("").to_string();
            let hint = match code {
                "fm-missing" => "Frontmatter mit `type` und `checked_on` ergänzen",
                "fm-unclosed" => "schließende Zeile `---` ergänzen",
                "fm-duplicate-key" => "doppelten Schlüssel entfernen",
                _ => "Zeile als `schlüssel: wert` schreiben oder entfernen",
            };
            let sev = if code == "fm-duplicate-key" || code == "fm-invalid-line" { Severity::Warning } else { Severity::Error };
            push(&mut out, &m.path, line, code, sev, msg, hint);
        }
        if let Some(t) = m.type_raw.as_deref()
            && !NoteType::VALID.contains(&t)
        {
            push(
                &mut out,
                &m.path,
                1,
                "fm-type",
                Severity::Info,
                format!("type `{t}` ist kein Wert des Formats"),
                "overview | topic | source",
            );
        }
        if let Some(s) = m.status.as_deref() {
            if NoteStatus::parse(s).is_none() {
                push(
                    &mut out,
                    &m.path,
                    1,
                    "fm-status",
                    Severity::Warning,
                    "status ist kein Wert der Aufzählung".into(),
                    "active | paused | done | superseded; freien Text nach `state` verschieben",
                );
            } else if m.note_type == NoteType::Source {
                push(&mut out, &m.path, 1, "fm-status", Severity::Info, "status gilt nur für overview und topic".into(), "status entfernen");
            }
        }
        if let Some(c) = &m.checked_on {
            match crate::query::days_from_date(c) {
                None => push(&mut out, &m.path, 1, "fm-date", Severity::Warning, format!("checked_on `{c}` ist kein Datum"), "JJJJ-MM-TT"),
                Some(d) => {
                    if let Some(t) = today
                        && t - d > stale {
                            push(
                                &mut out,
                                &m.path,
                                1,
                                "stale",
                                Severity::Info,
                                format!("checked_on {c} ist {} Tage alt", t - d),
                                "Inhalt prüfen und checked_on erneuern",
                            );
                        }
                }
            }
        }
        if m.lines == 0 || m.bytes == 0 {
            push(&mut out, &m.path, 1, "note-empty", Severity::Warning, "leere Notiz".into(), "füllen oder löschen");
        }
        if m.path.is_overview() && m.lines > MAX_OVERVIEW_LINES {
            push(
                &mut out,
                &m.path,
                MAX_OVERVIEW_LINES + 1,
                "overview-long",
                Severity::Warning,
                format!("Übersicht hat {} Zeilen (höchstens {MAX_OVERVIEW_LINES})", m.lines),
                "Details in Themennotizen auslagern",
            );
        }
        if m.tokens > MAX_NOTE_TOKENS {
            push(
                &mut out,
                &m.path,
                1,
                "note-large",
                Severity::Info,
                format!("Notiz hat ~{} Token (Grenze {MAX_NOTE_TOKENS})", m.tokens),
                "in Themennotizen teilen",
            );
        }
        let rootish = !m.path.as_str().contains('/') || m.path.is_overview();
        if m.links_in == 0 && !rootish && matches!(m.note_type, NoteType::Topic | NoteType::Source | NoteType::Other) {
            push(&mut out, &m.path, 1, "orphan", Severity::Info, "keine eingehenden Links".into(), "aus der Übersicht verlinken");
        }
    }
    for p in projects.difference(&projects_with_overview) {
        let path = NotePath::new(format!("Projects/{p}/00 Overview.md"));
        if in_scope(&path) {
            push(&mut out, &path, 0, "overview-missing", Severity::Error, format!("Projekt {p} hat keine 00 Overview.md"), "`wb new` mit --type overview");
        }
    }
    // Links
    let mut st = v.db.conn.prepare_cached(
        "SELECT n.path, l.line, l.target, l.status, l.candidates FROM links l JOIN notes n ON n.id = l.src
         WHERE l.status <> 'ok' ORDER BY n.path, l.line, l.id",
    )?;
    let rows = st.query_map(params![], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, u32>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?, r.get::<_, Option<String>>(4)?))
    })?;
    for r in rows {
        let (src, line, target, status, cands) = r?;
        let path = NotePath::new(src);
        if !in_scope(&path) {
            continue;
        }
        if status == "ambiguous" {
            let c = split_list(&cands.unwrap_or_default());
            push(
                &mut out,
                &path,
                line,
                "link-ambiguous",
                Severity::Warning,
                format!("[[{target}]] passt auf {} Notizen", c.len()),
                &format!("Pfad angeben, z. B. [[{}]]", c.first().map(|x| crate::model::strip_md(x)).unwrap_or("")),
            );
        } else {
            push(&mut out, &path, line, "link-dead", Severity::Error, format!("[[{target}]] zeigt ins Leere"), "Ziel anlegen oder Link korrigieren");
        }
    }
    // Verdacht auf Geheimnisse: beim Sync erkannt, nur Zeile und Klasse gespeichert (§13.9).
    let mut st = v.db.conn.prepare_cached(
        "SELECT n.path, s.line, s.class FROM secrets s JOIN notes n ON n.id = s.note_id ORDER BY n.path, s.line",
    )?;
    let rows = st.query_map(params![], |r| Ok((r.get::<_, String>(0)?, r.get::<_, u32>(1)?, r.get::<_, String>(2)?)))?;
    for r in rows {
        let (src, line, class) = r?;
        let path = NotePath::new(src);
        if in_scope(&path) {
            push(
                &mut out,
                &path,
                line,
                "secret-suspect",
                Severity::Warning,
                format!("mögliches Geheimnis (Klasse {class})"),
                "prüfen; echte Geheimnisse entfernen und den Schlüssel erneuern",
            );
        }
    }
    // Zwei Dateien mit gleichem Namen in verschiedener Unicode-Normalform (§15.5): indiziert ist
    // beständig die erste (Bytefolge), die zweite meldet dieser Befund.
    let coll: Option<String> = v.db.conn.query_row("SELECT value FROM meta WHERE key='name_collisions'", [], |r| r.get(0)).ok();
    for item in split_list(&coll.unwrap_or_default()) {
        let Some((kept, other)) = item.split_once('\t') else { continue };
        let path = NotePath::new(kept);
        if in_scope(&path) {
            push(
                &mut out,
                &path,
                0,
                "name-collision",
                Severity::Warning,
                format!("zweite Datei mit gleichem Namen in anderer Unicode-Form wird nicht indiziert: {other}"),
                "eine der beiden Dateien umbenennen oder zusammenführen",
            );
        }
    }
    out.sort_by(|a, b| a.path.cmp(&b.path).then(a.line.cmp(&b.line)).then(a.code.cmp(&b.code)).then(a.message.cmp(&b.message)));
    Ok(out)
}
