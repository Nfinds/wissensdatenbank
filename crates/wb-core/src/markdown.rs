//! Überschriften, Abschnitte und Abschnittsarten.

use crate::links::fence_marker;
use crate::model::{Section, SectionKind};
use crate::text::{estimate_tokens, fold, words};

/// ATX-Überschrift (`#`…`######`, bis drei Leerzeichen Einrückung): Ebene und Titel.
pub fn heading(line: &str) -> Option<(u8, &str)> {
    let t = line.trim_start_matches(' ');
    if line.len() - t.len() > 3 {
        return None;
    }
    let n = t.bytes().take_while(|&b| b == b'#').count();
    if n == 0 || n > 6 {
        return None;
    }
    let rest = &t[n..];
    if !rest.is_empty() && !rest.starts_with(' ') && !rest.starts_with('\t') {
        return None;
    }
    let title = rest.trim();
    // schließende #-Folge entfernen
    let title = title.trim_end_matches('#').trim_end();
    Some((n as u8, title))
}

/// Zeilen eines Textes ohne Zeilenende; eine abschließende Leerzeile nach `\n` zählt nicht.
pub fn lines(text: &str) -> Vec<&str> {
    let mut v: Vec<&str> = text.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l)).collect();
    if v.last() == Some(&"") {
        v.pop();
    }
    v
}

/// Abschnitte ab `body_line` (1-basiert). Überschriften in Codeblöcken zählen nicht.
pub fn sections(all_lines: &[&str], body_line: u32) -> Vec<Section> {
    let n = all_lines.len() as u32;
    let mut heads: Vec<(u32, u8, String)> = Vec::new();
    let mut fence: Option<(u8, usize)> = None;
    for ln in body_line..=n {
        let line = all_lines[(ln - 1) as usize];
        if let Some((ch, len)) = fence_marker(line) {
            match fence {
                None => fence = Some((ch, len)),
                Some((fc, fl)) if fc == ch && len >= fl => fence = None,
                _ => {}
            }
            continue;
        }
        if fence.is_some() {
            continue;
        }
        if let Some((lvl, title)) = heading(line) {
            heads.push((ln, lvl, title.to_string()));
        }
    }
    let last_nonblank = |from: u32, to: u32| -> u32 {
        let mut e = to;
        while e > from && all_lines[(e - 1) as usize].trim().is_empty() {
            e -= 1;
        }
        e
    };
    let tokens = |from: u32, to: u32| -> u32 {
        if to < from {
            return 0;
        }
        let s: usize = (from..=to).map(|l| all_lines[(l - 1) as usize].chars().count() + 1).sum();
        // chars/3.5 wie estimate_tokens, ohne den Text zu kopieren
        ((s as u64 * 2).div_ceil(7)) as u32
    };
    let mut out = Vec::new();
    let first_head = heads.first().map(|h| h.0).unwrap_or(n + 1);
    if body_line < first_head {
        let pre_end = first_head - 1;
        let has_text = (body_line..=pre_end).any(|l| !all_lines[(l - 1) as usize].trim().is_empty());
        if has_text {
            let start = (body_line..=pre_end).find(|&l| !all_lines[(l - 1) as usize].trim().is_empty()).unwrap();
            let end = last_nonblank(start, pre_end);
            out.push(Section {
                index: 0,
                level: 0,
                title: String::new(),
                kind: SectionKind::Other,
                line_start: start,
                line_end: end,
                own_end: end,
                tokens: tokens(start, end),
                own_tokens: tokens(start, end),
            });
        }
    }
    for (i, (ln, lvl, title)) in heads.iter().enumerate() {
        let own_raw = heads.get(i + 1).map(|h| h.0 - 1).unwrap_or(n);
        let end_raw = heads[i + 1..].iter().find(|h| h.1 <= *lvl).map(|h| h.0 - 1).unwrap_or(n);
        let own_end = last_nonblank(*ln, own_raw);
        let line_end = last_nonblank(*ln, end_raw);
        out.push(Section {
            index: out.len() as u32,
            level: *lvl,
            title: title.clone(),
            kind: section_kind(title),
            line_start: *ln,
            line_end,
            own_end,
            tokens: tokens(*ln, line_end),
            own_tokens: tokens(*ln, own_end),
        });
    }
    out
}

/// Abschnittsart aus der Überschrift (Vertrag §4). Datumszusätze und Nummerierung stören nicht.
pub fn section_kind(title: &str) -> SectionKind {
    let f = fold(title);
    let ws: Vec<String> = words(&f).map(|(_, w)| w.to_string()).filter(|w| !w.chars().all(|c| c.is_ascii_digit())).collect();
    // Schlüsselwörter werden wie die Überschrift gefaltet (z. B. „quellen“ → „qullen“).
    let has = |w: &str| {
        let w = fold(w);
        ws.contains(&w)
    };
    let pre = |p: &str| {
        let p = fold(p);
        ws.iter().any(|x| x.starts_with(&p))
    };
    let contains = |p: &str| {
        let p = fold(p);
        ws.iter().any(|x| x.contains(&p))
    };
    let seq = |a: &str, b: &str| {
        let (a, b) = (fold(a), fold(b));
        ws.windows(2).any(|p| p[0] == a && p[1].starts_with(&b))
    };

    if pre("offen") || has("open") || pre("nachst") || seq("next", "step") || has("todo") || pre("ausstehend") || has("pending") || has("backlog") {
        return SectionKind::Open;
    }
    if has("stand") || has("status") || pre("kurzstatus") || has("zustand") || has("state") || pre("lagebild") || seq("current", "stat") {
        return SectionKind::State;
    }
    if contains("entscheid") || pre("decision") || pre("beschluss") {
        return SectionKind::Decisions;
    }
    if contains("sackgass") || contains("stolperstein") || pre("lesson") || pre("pitfall") || seq("dead", "end") || seq("was", "funktion") || pre("fallstrick") || pre("erkenntnis") || pre("gotcha") || pre("lehre") {
        return SectionKind::Lessons;
    }
    if pre("prufung") || pre("gepruft") || has("test") || has("tests") || pre("testergebnis") || pre("verifi") || pre("beleg") || pre("messung") || has("checks") || pre("validier") || pre("abnahme") {
        return SectionKind::Evidence;
    }
    if pre("quelle") || contains("quellen") || has("sources") || has("source") || pre("referenz") || has("references") {
        return SectionKind::Sources;
    }
    if has("pfade") || has("pfad") || has("dateien") || has("paths") || has("files") || pre("location") || seq("zweck", "und") && (has("ort") || has("orte") || has("pfade")) || pre("speicherort") || has("ablage") {
        return SectionKind::Paths;
    }
    if has("themen") || pre("themennotiz") || has("topics") || pre("unterseite") {
        return SectionKind::Topics;
    }
    SectionKind::Other
}

/// Anker für eine Adresse `Notiz#Anker`: der Titel, wenn er in der Notiz eindeutig ist,
/// sonst `L<Zeile>`.
pub fn anchor(sections: &[Section], idx: usize) -> String {
    let s = &sections[idx];
    if s.level == 0 || s.title.is_empty() {
        return format!("L{}", s.line_start);
    }
    let key = fold(&s.title);
    let dup = sections.iter().filter(|o| fold(&o.title) == key).count() > 1;
    if dup || s.title.contains(['#', '|', '[', ']']) { format!("L{}", s.line_start) } else { s.title.clone() }
}

/// Text eines Abschnitts ohne Überschriftszeile (eigener Text oder mit Unterabschnitten).
pub fn section_body(all_lines: &[&str], s: &Section, with_children: bool) -> String {
    let start = if s.level == 0 { s.line_start } else { s.line_start + 1 };
    let end = if with_children { s.line_end } else { s.own_end };
    if end < start {
        return String::new();
    }
    let mut out = String::new();
    for l in start..=end {
        out.push_str(all_lines[(l - 1) as usize]);
        out.push('\n');
    }
    out
}

/// Listenpunkte (`-`, `*`, `+`, `1.`) in einem Text zählen.
pub fn count_list_items(text: &str) -> u32 {
    text.lines()
        .filter(|l| {
            let t = l.trim_start();
            t.starts_with("- ") || t.starts_with("* ") || t.starts_with("+ ") || {
                let d = t.bytes().take_while(|b| b.is_ascii_digit()).count();
                d > 0 && t[d..].starts_with(". ")
            }
        })
        .count() as u32
}

/// Für Ausschnitte: Wikilinks zu ihrem Anzeigetext, `**`/`__` weg, Leerraum zusammengefasst.
pub fn plain_inline(s: &str) -> String {
    let links = crate::links::parse_wikilinks(s);
    let mut out = String::with_capacity(s.len());
    let mut pos = 0;
    for l in &links {
        out.push_str(&s[pos..l.start]);
        let shown = l.alias.clone().unwrap_or_else(|| {
            let t = l.target.rsplit('/').next().unwrap_or(&l.target).to_string();
            match &l.heading {
                Some(h) => format!("{t}#{h}"),
                None => t,
            }
        });
        out.push_str(&shown);
        pos = l.end;
    }
    out.push_str(&s[pos..]);
    let out = out.replace("**", "");
    crate::text::squash_ws(&out)
}

/// Erster Satz des ersten Absatzes (für Einzeiler), ohne Überschriften, Tabellen, Code.
pub fn first_sentence(all_lines: &[&str], body_line: u32, max_chars: usize) -> Option<String> {
    let mut fence = false;
    for ln in body_line as usize..=all_lines.len() {
        let l = all_lines[ln - 1];
        if fence_marker(l).is_some() {
            fence = !fence;
            continue;
        }
        let t = l.trim();
        if fence || t.is_empty() || heading(l).is_some() || t.starts_with('|') || t.starts_with('>') || t.starts_with("---") {
            continue;
        }
        let t = t.trim_start_matches(['-', '*', '+']).trim();
        let plain = plain_inline(t);
        if plain.chars().filter(|c| c.is_alphabetic()).count() < 3 {
            continue;
        }
        let sents = crate::text::sentences(&plain);
        let first = sents.first().map(|&(a, b)| &plain[a..b]).unwrap_or(&plain);
        let _ = estimate_tokens;
        return Some(crate::text::truncate_chars(first, max_chars));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds() {
        assert_eq!(section_kind("Stand (2026-10-16)"), SectionKind::State);
        assert_eq!(section_kind("Aktueller Stand"), SectionKind::State);
        assert_eq!(section_kind("Standort"), SectionKind::Other);
        assert_eq!(section_kind("Offene Punkte"), SectionKind::Open);
        assert_eq!(section_kind("Nächster Schritt"), SectionKind::Open);
        assert_eq!(section_kind("Kernentscheidungen"), SectionKind::Decisions);
        assert_eq!(section_kind("Was funktioniert hat und Sackgassen"), SectionKind::Lessons);
        assert_eq!(section_kind("Prüfungen 2026-11-09"), SectionKind::Evidence);
        assert_eq!(section_kind("Primärquellen"), SectionKind::Sources);
        assert_eq!(section_kind("Zweck und Pfade"), SectionKind::Paths);
        assert_eq!(section_kind("Purpose and locations"), SectionKind::Paths);
        assert_eq!(section_kind("Current status and gates"), SectionKind::State);
        assert_eq!(section_kind("Themen"), SectionKind::Topics);
    }

    #[test]
    fn section_ranges() {
        let t = "---\ntype: topic\n---\nVorspann\n\n# Titel\nText\n## A\na\n### A1\nx\n\n## B\nb\n```\n# kein Titel\n```\n";
        let ls = lines(t);
        let s = sections(&ls, 4);
        let v: Vec<_> = s.iter().map(|s| (s.level, s.title.as_str(), s.line_start, s.own_end, s.line_end)).collect();
        assert_eq!(
            v,
            [(0, "", 4, 4, 4), (1, "Titel", 6, 7, 17), (2, "A", 8, 9, 11), (3, "A1", 10, 11, 11), (2, "B", 13, 17, 17)]
        );
    }
}
