//! Passagensuche über Abschnitte.
//!
//! Je Begriff: FTS5-Treffer über gefaltete Formen und Stämme (BM25, Spaltengewichte Titel 6,
//! Überschrift 4, Alias 5, Pfad 3, Text 1). Begriffe ab 4 Zeichen zusätzlich als Teilwort über
//! die Trigramm-Tabelle (Komposita). Ohne jeden Treffer: Tippfehlerkorrektur gegen den
//! Index-Wortschatz. Abschnittswert = Summe der Begriffswerte × Abdeckung², wobei die Abdeckung
//! Abschnitt und Notiz mittelt. Danach Notizfaktoren (Vertrag §13.10): Übersicht eines genannten
//! Projekts ×2, Quelle ×[`source_weight`], überholte Notiz ×[`superseded_weight`] (beides entfällt,
//! wenn der Filter genau danach fragt). Gleichstand: Pfad, dann Zeile.

use std::collections::{HashMap, HashSet};

use rusqlite::params;

use crate::error::{Error, Result};
use crate::markdown;
use crate::model::*;
use crate::text::{Analyzer, edit_distance, fold, is_stopword, sentences, words};
use crate::vault::Vault;

const W_TITLE: f64 = 6.0;
const W_HEADING: f64 = 4.0;
const W_ALIAS: f64 = 5.0;
const W_PATH: f64 = 3.0;
const W_BODY: f64 = 1.0;
/// Gewicht eines reinen Teilwort-Treffers relativ zur IDF des Teilworts.
const SUBSTRING_WEIGHT: f64 = 0.6;
/// Nennt die Anfrage einen Projektnamen als Wortfolge, zählt dessen Übersicht so viel mehr
/// (Einstiegsfragen wie „Gartenhaus Stand“ sollen in der Übersicht landen).
/// `WB_TUNE_OVERVIEW` überschreibt den Wert nur für Messreihen.
fn overview_boost() -> f64 {
    tune("WB_TUNE_OVERVIEW", 2.0)
}

/// Stellgröße für Messreihen: nur im Debug-Build aus der Umgebung, im Release immer der feste
/// Wert, damit keine Umgebungsvariable die Trefferreihenfolge ändert (§15.7).
fn tune(name: &str, default: f64) -> f64 {
    if cfg!(debug_assertions) { std::env::var(name).ok().and_then(|x| x.parse().ok()).unwrap_or(default) } else { default }
}

/// Faktor für Quellen (`Sources/`, `type: source`); am Entwicklungsteil des Golden-Sets eingestellt.
/// `WB_TUNE_SOURCE` überschreibt den Wert nur für Messreihen.
fn source_weight() -> f64 {
    tune("WB_TUNE_SOURCE", SOURCE_WEIGHT)
}
const SOURCE_WEIGHT: f64 = 0.7;

/// Faktor für überholte Notizen (`status: superseded`). `WB_TUNE_SUPERSEDED` nur für Messreihen.
fn superseded_weight() -> f64 {
    tune("WB_TUNE_SUPERSEDED", SUPERSEDED_WEIGHT)
}
// Eingestellt am migrierten Vault, Teil dev (2026-11-10): 1,0/0,7/0,5 lassen MRR gleich (0,923);
// überholte Notizen in den Top 5 gewöhnlicher Anfragen 9/3/2 von 135; nach Titel gesucht auf
// Platz 1 49/37/20 von 54. 0,7 nimmt fast alles Rauschen weg und lässt sie auffindbar.
const SUPERSEDED_WEIGHT: f64 = 0.7;

struct Term {
    raw: String,
    folded: String,
    forms: Vec<String>,
    stop: bool,
    corrected: Option<String>,
    substring: bool,
    hits: HashMap<i64, f64>,
}

struct SecMeta {
    note_id: i64,
    idx: u32,
    title: String,
    kind: SectionKind,
    line_start: u32,
    line_end: u32,
    tokens: u32,
    anchor: String,
    path: NotePath,
    ntype: NoteType,
    project: Option<String>,
    date: Option<String>,
    status: Option<NoteStatus>,
    /// Überschriftenebene, 0 für den Vorspann.
    level: u8,
}

impl SecMeta {
    /// Abschnitt, der nur aus seiner Überschrift besteht (kein Text, keine Unterabschnitte):
    /// `wb read` auf seine Adresse liefert nichts als die Überschrift.
    fn heading_only(&self) -> bool {
        self.level > 0 && self.line_end == self.line_start
    }
}

fn quote_forms(forms: &[String]) -> String {
    forms.iter().map(|f| format!("\"{}\"", f.replace('"', ""))).collect::<Vec<_>>().join(" OR ")
}

pub(crate) fn search(v: &Vault, q: &SearchQuery) -> Result<SearchResult> {
    let an = Analyzer::new();
    let conn = &v.db.conn;
    let mut terms: Vec<Term> = Vec::new();
    let mut seen = HashSet::new();
    for (_, w) in words(&q.text) {
        let folded = fold(w);
        if !seen.insert(folded.clone()) {
            continue;
        }
        terms.push(Term {
            raw: w.to_string(),
            stop: is_stopword(&folded),
            forms: an.forms(w),
            folded,
            corrected: None,
            substring: false,
            hits: HashMap::new(),
        });
    }
    if terms.is_empty() {
        return Err(Error::Usage { message: "Suchanfrage ohne Wort (nur Satzzeichen oder Füllwörter)".into(), hint: Some("mindestens ein Wort aus Buchstaben oder Ziffern angeben, z. B. wb search Messung".into()) });
    }
    if terms.iter().all(|t| t.stop) {
        terms.iter_mut().for_each(|t| t.stop = false);
    }
    let total_sections: f64 = conn.query_row("SELECT count(*) FROM sections", [], |r| r.get::<_, i64>(0))? as f64;

    for t in terms.iter_mut().filter(|t| !t.stop) {
        let mut st = conn.prepare_cached(&format!(
            "SELECT rowid, bm25(sec_fts, {W_TITLE}, {W_HEADING}, {W_ALIAS}, {W_PATH}, {W_BODY}) FROM sec_fts WHERE sec_fts MATCH ?1"
        ))?;
        let rows = st.query_map(params![quote_forms(&t.forms)], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, f64>(1)?)))?;
        for r in rows {
            let (id, s) = r?;
            t.hits.insert(id, -s);
        }
        let flen = t.folded.chars().count();
        if flen >= 4 {
            let mut st = conn.prepare_cached("SELECT rowid, bm25(sec_tri) FROM sec_tri WHERE sec_tri MATCH ?1")?;
            let rows: Vec<(i64, f64)> = st
                .query_map(params![format!("\"{}\"", t.folded.replace('"', ""))], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<rusqlite::Result<_>>()?;
            let n = rows.len() as f64;
            if n > 0.0 {
                let idf = ((total_sections - n + 0.5) / (n + 0.5) + 1.0).ln();
                let best = rows.iter().map(|r| -r.1).fold(f64::MIN, f64::max).max(1e-9);
                for (id, s) in rows {
                    if let std::collections::hash_map::Entry::Vacant(e) = t.hits.entry(id) {
                        e.insert(SUBSTRING_WEIGHT * idf * (0.9 + 0.1 * (-s / best)));
                        t.substring = true;
                    }
                }
            }
        }
        if t.hits.is_empty() && flen >= 4
            && let Some(c) = correct(conn, &t.folded)? {
                let mut st = conn.prepare_cached(&format!(
                    "SELECT rowid, bm25(sec_fts, {W_TITLE}, {W_HEADING}, {W_ALIAS}, {W_PATH}, {W_BODY}) FROM sec_fts WHERE sec_fts MATCH ?1"
                ))?;
                let rows = st.query_map(params![quote_forms(std::slice::from_ref(&c))], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, f64>(1)?)))?;
                for r in rows {
                    let (id, s) = r?;
                    t.hits.insert(id, -s);
                }
                t.forms.push(c.clone());
                t.corrected = Some(c);
            }
    }

    // Kandidaten und ihre Metadaten
    let mut cand: Vec<i64> = terms.iter().flat_map(|t| t.hits.keys().copied()).collect::<HashSet<_>>().into_iter().collect();
    cand.sort_unstable();
    let metas = load_meta(v, &cand)?;
    let active: Vec<&Term> = terms.iter().filter(|t| !t.stop).collect();
    let n_terms = active.len().max(1) as f64;
    // Begriffe je Notiz
    let mut note_terms: HashMap<i64, HashSet<usize>> = HashMap::new();
    for (ti, t) in active.iter().enumerate() {
        for id in t.hits.keys() {
            if let Some(m) = metas.get(id) {
                note_terms.entry(m.note_id).or_default().insert(ti);
            }
        }
    }
    // Projekte, die die Anfrage als zusammenhängende Wortfolge nennt
    let qwords: Vec<String> = words(&q.text).map(|(_, w)| fold(w)).collect();
    let mut named: HashSet<String> = HashSet::new();
    {
        let mut st = conn.prepare_cached("SELECT DISTINCT project FROM notes WHERE project IS NOT NULL")?;
        for p in st.query_map([], |r| r.get::<_, String>(0))? {
            let p = p?;
            let pw: Vec<String> = words(&p).map(|(_, w)| fold(w)).collect();
            if !pw.is_empty() && qwords.windows(pw.len()).any(|w| w == pw.as_slice()) {
                named.insert(p);
            }
        }
    }
    let boost = overview_boost();
    let w_source = if q.note_type == Some(NoteType::Source) { 1.0 } else { source_weight() };
    let w_superseded = if q.status == Some(NoteStatus::Superseded) { 1.0 } else { superseded_weight() };
    let prefix = q.prefix.as_deref().map(|p| p.replace('\\', "/").to_lowercase());
    let mut scored: Vec<(f64, i64)> = Vec::new();
    for id in &cand {
        let Some(m) = metas.get(id) else { continue };
        if m.ntype == NoteType::Journal {
            continue;
        }
        if q.note_type.is_some_and(|t| t != m.ntype) {
            continue;
        }
        if q.kind.is_some_and(|k| k != m.kind) {
            continue;
        }
        if q.status.is_some() && q.status != m.status {
            continue;
        }
        if let Some(p) = &q.project
            && !m.project.as_deref().is_some_and(|mp| fold(mp) == fold(p)) {
                continue;
            }
        if let Some(p) = &prefix
            && !m.path.as_str().to_lowercase().starts_with(p) {
                continue;
            }
        let mut sum = 0.0;
        let mut matched = 0usize;
        for t in &active {
            if let Some(s) = t.hits.get(id) {
                sum += s;
                matched += 1;
            }
        }
        let in_note = note_terms.get(&m.note_id).map(|s| s.len()).unwrap_or(0);
        let coverage = (matched as f64 + in_note as f64) / (2.0 * n_terms);
        let mut score = sum * coverage * coverage;
        if m.path.is_overview() && m.project.as_ref().is_some_and(|p| named.contains(p)) {
            score *= boost;
        }
        if m.ntype == NoteType::Source {
            score *= w_source;
        }
        if m.status == Some(NoteStatus::Superseded) {
            score *= w_superseded;
        }
        scored.push(((score * 1e9).round() / 1e9, *id));
    }
    scored.sort_by(|a, b| {
        b.0.partial_cmp(&a.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| metas[&a.1].path.cmp(&metas[&b.1].path))
            .then_with(|| metas[&a.1].line_start.cmp(&metas[&b.1].line_start))
    });
    // §23.3: Ein Abschnitt nur aus Überschrift (leere Vorlage, leere Themenliste) ist kein
    // Leseziel. Er entfällt, wenn dieselbe Notiz einen Treffer mit Text hat; sonst bleibt der
    // beste als einziger Treffer der Notiz, damit sie über ihren Titel auffindbar ist.
    let with_text: HashSet<i64> = scored.iter().map(|(_, id)| &metas[id]).filter(|m| !m.heading_only()).map(|m| m.note_id).collect();
    let mut bare: HashSet<i64> = HashSet::new();
    scored.retain(|(_, id)| {
        let m = &metas[id];
        !m.heading_only() || (!with_text.contains(&m.note_id) && bare.insert(m.note_id))
    });

    // Höchstens per_note Abschnitte je Notiz
    let per_note = q.per_note.max(1);
    let mut kept: Vec<(f64, i64, u32)> = Vec::new();
    let mut count: HashMap<i64, (usize, usize)> = HashMap::new(); // note → (gelistet, Index des ersten)
    for (s, id) in scored {
        let nid = metas[&id].note_id;
        match count.get_mut(&nid) {
            Some((c, first)) if *c >= per_note => {
                kept[*first].2 += 1;
            }
            Some((c, _)) => {
                *c += 1;
                kept.push((s, id, 0));
            }
            None => {
                count.insert(nid, (1, kept.len()));
                kept.push((s, id, 0));
            }
        }
    }
    let total = kept.len() as u32;
    let mut hits = Vec::new();
    let term_forms: Vec<HashSet<String>> = active.iter().map(|t| t.forms.iter().cloned().collect()).collect();
    let substrings: Vec<Option<String>> = active.iter().map(|t| t.substring.then(|| t.folded.clone())).collect();
    for (i, (score, id, more)) in kept.iter().enumerate().skip(q.offset).take(q.limit) {
        let m = &metas[id];
        let body: String = conn.query_row("SELECT body FROM sections WHERE id = ?1", params![id], |r| r.get(0))?;
        let (snippet, highlights) = make_snippet(&an, &body, &m.title, &term_forms, &substrings);
        hits.push(SearchHit {
            rank: i as u32 + 1,
            address: format!("{}#{}", m.path.without_ext(), m.anchor),
            path: m.path.clone(),
            section_index: m.idx,
            section_title: m.title.clone(),
            kind: m.kind,
            line_start: m.line_start,
            line_end: m.line_end,
            note_type: m.ntype,
            project: m.project.clone(),
            status: m.status,
            date: m.date.clone(),
            tokens: m.tokens,
            score: *score,
            snippet,
            highlights,
            more_in_note: *more,
        });
    }
    let terms_out = terms
        .iter()
        .map(|t| QueryTerm {
            raw: t.raw.clone(),
            forms: t.forms.clone(),
            corrected: t.corrected.clone(),
            substring: t.substring,
            stopword: t.stop,
            sections: t.hits.len() as u32,
        })
        .collect();
    Ok(SearchResult { query: q.text.clone(), terms: terms_out, total, offset: q.offset, hits })
}

/// Ähnlichstes Wort im Wortschatz (gleicher Anfangsbuchstabe, Abstand ≤1 bis 5 Zeichen, sonst ≤2).
fn correct(conn: &rusqlite::Connection, folded: &str) -> Result<Option<String>> {
    let first: String = folded.chars().next().map(|c| c.to_string()).unwrap_or_default();
    let max = if folded.chars().count() <= 5 { 1 } else { 2 };
    let mut st = conn.prepare_cached("SELECT term, doc FROM sec_vocab WHERE term >= ?1 AND term < ?2")?;
    let upper = format!("{first}\u{10FFFF}");
    let rows = st.query_map(params![first, upper], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
    let mut best: Option<(usize, i64, String)> = None;
    for r in rows {
        let (term, doc) = r?;
        if let Some(d) = edit_distance(folded, &term, max) {
            if d == 0 {
                continue;
            }
            let better = match &best {
                None => true,
                Some((bd, bdoc, bt)) => d < *bd || (d == *bd && (doc > *bdoc || (doc == *bdoc && term < *bt))),
            };
            if better {
                best = Some((d, doc, term));
            }
        }
    }
    Ok(best.map(|b| b.2))
}

fn load_meta(v: &Vault, ids: &[i64]) -> Result<HashMap<i64, SecMeta>> {
    let mut out = HashMap::with_capacity(ids.len());
    for chunk in ids.chunks(400) {
        let qs = vec!["?"; chunk.len()].join(",");
        let sql = format!(
            "SELECT s.id, s.note_id, s.idx, s.title, s.kind, s.line_start, s.line_end, s.tokens, s.anchor,
                    n.path, n.ntype, n.project, n.checked_on, n.mtime_ns, n.status, s.level
             FROM sections s JOIN notes n ON n.id = s.note_id WHERE s.id IN ({qs})"
        );
        let mut st = v.db.conn.prepare(&sql)?;
        let rows = st.query_map(rusqlite::params_from_iter(chunk.iter()), |r| {
            let kind: String = r.get(4)?;
            let ntype: String = r.get(10)?;
            let checked: Option<String> = r.get(12)?;
            let mtime: i64 = r.get(13)?;
            let status: Option<String> = r.get(14)?;
            Ok((
                r.get::<_, i64>(0)?,
                SecMeta {
                    note_id: r.get(1)?,
                    idx: r.get(2)?,
                    title: r.get(3)?,
                    kind: SectionKind::parse(&kind).unwrap_or(SectionKind::Other),
                    line_start: r.get(5)?,
                    line_end: r.get(6)?,
                    tokens: r.get(7)?,
                    anchor: r.get(8)?,
                    path: NotePath::new(r.get::<_, String>(9)?),
                    ntype: NoteType::parse(&ntype).unwrap_or(NoteType::Other),
                    project: r.get(11)?,
                    date: checked.or_else(|| Some(crate::query::date_of_mtime(mtime / 1_000_000_000))),
                    status: status.as_deref().and_then(NoteStatus::parse),
                    level: r.get(15)?,
                },
            ))
        })?;
        for r in rows {
            let (id, m) = r?;
            out.insert(id, m);
        }
    }
    Ok(out)
}

const SNIPPET_MAX: usize = 240;

/// Satzgenauer Ausschnitt mit Trefferstellen (Byte-Bereiche im Ausschnitt).
pub(crate) fn make_snippet(
    an: &Analyzer,
    body: &str,
    title: &str,
    term_forms: &[HashSet<String>],
    substrings: &[Option<String>],
) -> (String, Vec<(usize, usize)>) {
    let plain: String = body
        .lines()
        .map(|l| {
            let t = l.trim();
            if markdown::heading(l).is_some() || crate::links::fence_marker(l).is_some() { String::new() } else { markdown::plain_inline(t) }
        })
        .filter(|l| !l.is_empty() && !l.chars().all(|c| c == '|' || c == '-' || c == ':' || c == ' '))
        .collect::<Vec<_>>()
        .join("\n");
    let text = if plain.trim().is_empty() { title.to_string() } else { plain };
    if text.is_empty() {
        return (String::new(), Vec::new());
    }
    let sents = sentences(&text);
    let mut best: Option<(usize, usize, usize)> = None; // (Begriffe, Treffer, Satzindex)
    let mut sent_hits: Vec<Vec<(usize, usize)>> = Vec::with_capacity(sents.len());
    for (si, &(a, b)) in sents.iter().enumerate() {
        let s = &text[a..b];
        let mut terms_hit = HashSet::new();
        let mut ranges = Vec::new();
        for (off, w) in words(s) {
            let forms = an.forms(w);
            let fw = fold(w);
            for (ti, tf) in term_forms.iter().enumerate() {
                let hit = forms.iter().any(|f| tf.contains(f)) || substrings[ti].as_deref().is_some_and(|sub| fw.contains(sub));
                if hit {
                    terms_hit.insert(ti);
                    ranges.push((a + off, a + off + w.len()));
                    break;
                }
            }
        }
        let key = (terms_hit.len(), ranges.len());
        if key.0 > 0 && best.is_none_or(|b| (key.0, key.1) > (b.0, b.1)) {
            best = Some((key.0, key.1, si));
        }
        sent_hits.push(ranges);
    }
    let si = best.map(|b| b.2).unwrap_or(0);
    let Some(&(mut start, mut end)) = sents.get(si) else { return (String::new(), Vec::new()) };
    // kurzer Satz: Folgesatz anhängen
    let mut j = si + 1;
    while text[start..end].chars().count() < 120 && j < sents.len() && text[start..sents[j].1].chars().count() <= SNIPPET_MAX {
        end = sents[j].1;
        j += 1;
    }
    let mut ranges: Vec<(usize, usize)> = sent_hits[si..j].iter().flatten().copied().collect();
    let mut lead = false;
    let mut tail = false;
    if text[start..end].chars().count() > SNIPPET_MAX {
        // um den ersten Treffer zentrieren
        let anchor = ranges.first().map(|r| r.0).unwrap_or(start);
        let back = text[start..anchor].char_indices().rev().nth(80).map(|(i, _)| start + i).unwrap_or(start);
        let back = if back > start {
            lead = true;
            text[back..anchor].find(' ').map(|i| back + i + 1).filter(|&x| x <= anchor).unwrap_or(back)
        } else {
            back
        };
        start = back;
        let lim = text[start..end].char_indices().nth(SNIPPET_MAX).map(|(i, _)| start + i).unwrap_or(end);
        if lim < end {
            tail = true;
            end = text[start..lim].rfind(' ').map(|i| start + i).filter(|&x| x > start + SNIPPET_MAX / 2).unwrap_or(lim);
        }
    }
    ranges.retain(|r| r.0 >= start && r.1 <= end);
    let body = text[start..end].replace('\n', " ");
    let mut out = String::new();
    let shift = if lead {
        out.push('…');
        '…'.len_utf8()
    } else {
        0
    };
    out.push_str(&body);
    if tail {
        out.push('…');
    }
    let hl = ranges.into_iter().map(|(a, b)| (a - start + shift, b - start + shift)).collect();
    (out, hl)
}
