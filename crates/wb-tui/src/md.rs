//! Markdown lesbar setzen (Leser, Vorschau) und Wikilinks erkennen.
//!
//! Absichtlich klein: Überschriften, Listen, Zitate, Code, Tabellenzeilen, Hervorhebungen,
//! Wikilinks und Verweise. Absätze stehen im Bestand als eine einzige sehr lange Zeile; der
//! Umbruch nach Anzeigebreite ist deshalb Kern der Aufgabe.

use crate::text::{truncate, width, wrap, Spans};
use crate::theme::Theme;
use ratatui::style::{Modifier, Style};

/// Ein Wikilink im Rohtext.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WikiLink {
    pub target: String,
    pub anchor: Option<String>,
    pub alias: Option<String>,
    pub start: usize,
    pub end: usize,
}

/// Alle `[[Ziel#Anker|Alias]]` in Reihenfolge des Auftretens.
pub fn wikilinks(text: &str) -> Vec<WikiLink> {
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(p) = text[i..].find("[[") {
        let s = i + p;
        let Some(q) = text[s + 2..].find("]]") else { break };
        let e = s + 2 + q;
        let inner = &text[s + 2..e];
        if !inner.contains('\n') && !inner.is_empty() {
            let (left, alias) = match inner.split_once('|') {
                Some((l, a)) => (l, Some(a.trim().to_string())),
                None => (inner, None),
            };
            let (target, anchor) = match left.split_once('#') {
                Some((t, a)) => (t.trim().to_string(), Some(a.trim().to_string())),
                None => (left.trim().to_string(), None),
            };
            out.push(WikiLink { target, anchor, alias, start: s, end: e + 2 });
        }
        i = e + 2;
    }
    out
}

/// Anzeigename eines Links: Alias, sonst letzter Pfadteil.
pub fn link_label(l: &WikiLink) -> String {
    if let Some(a) = &l.alias {
        if !a.is_empty() {
            return a.clone();
        }
    }
    l.target.rsplit('/').next().unwrap_or(&l.target).to_string()
}

pub struct Rendered {
    pub lines: Vec<Spans>,
    /// Zeilenindex jeder Überschrift (Reihenfolge wie im Text).
    pub headings: Vec<usize>,
    /// Quellzeile (0-basiert, mit Frontmatter) → erste gesetzte Zeile.
    pub src_to_out: Vec<usize>,
}

/// Setzt `text` auf Breite `w`. `link_no` liefert zu einem Wikilink-Ziel die Nummer in der Linkliste.
pub fn render(text: &str, w: usize, theme: &Theme, link_no: &dyn Fn(&str) -> Option<usize>) -> Rendered {
    let g = theme.glyphs();
    let w = w.max(8);
    let mut lines: Vec<Spans> = Vec::new();
    let mut headings = Vec::new();
    let mut in_code = false;
    let mut raw = text.lines().map(|l| l.trim_end_matches('\r')).peekable();
    let mut src_to_out: Vec<usize> = Vec::new();

    // Frontmatter überspringen
    if raw.peek().map(|l| l.trim() == "---").unwrap_or(false) {
        let rest: Vec<&str> = text.lines().map(|l| l.trim_end_matches('\r')).collect();
        if let Some(end) = rest.iter().skip(1).position(|l| l.trim() == "---") {
            for _ in 0..(end + 2) {
                raw.next();
            }
            src_to_out.resize(end + 2, 0);
        }
    }

    let blank = |lines: &mut Vec<Spans>| {
        if lines.last().map(|l| !l.is_empty()).unwrap_or(false) {
            lines.push(Vec::new());
        }
    };

    for line in raw {
        src_to_out.push(lines.len());
        let t = line.trim_end();
        if t.trim_start().starts_with("```") {
            in_code = !in_code;
            continue;
        }
        if in_code {
            let body = truncate(t, w.saturating_sub(2), g.ellipsis);
            lines.push(vec![(format!("{} ", g.sep), theme.dim()), (body, theme.code())]);
            continue;
        }
        if t.trim().is_empty() {
            blank(&mut lines);
            continue;
        }
        let trimmed = t.trim_start();
        let indent = t.len() - trimmed.len();

        if let Some(rest) = heading(trimmed) {
            let (level, title) = rest;
            blank(&mut lines);
            let style = match level {
                1 => theme.accent().add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
                2 => theme.accent().add_modifier(Modifier::BOLD),
                _ => theme.title(),
            };
            let spans = vec![(strip_inline(title), style)];
            headings.push(lines.len());
            lines.extend(wrap(&spans, w, 0, 0));
            continue;
        }
        if is_rule(trimmed) {
            lines.push(vec![(g.rule.to_string().repeat(w.min(40)), theme.dim())]);
            continue;
        }
        if trimmed.starts_with('|') {
            lines.push(vec![(truncate(trimmed, w, g.ellipsis), theme.dim())]);
            continue;
        }
        if let Some(q) = trimmed.strip_prefix('>') {
            let spans = inline(q.trim_start(), theme, theme.quote(), link_no);
            for mut l in wrap(&spans, w, 2, 2) {
                if let Some(first) = l.first_mut() {
                    first.0 = format!("{} ", g.quote_bar);
                    first.1 = theme.dim();
                }
                lines.push(l);
            }
            continue;
        }
        if let Some((marker, rest)) = list_item(trimmed) {
            let level = indent / 2;
            let bullet = if marker == "-" || marker == "*" || marker == "+" {
                if level == 0 { g.bullet.to_string() } else { g.bullet2.to_string() }
            } else {
                marker.to_string()
            };
            let ind = (level * 2).min(w / 3);
            let mut spans = vec![(format!("{bullet} "), theme.dim())];
            spans.extend(inline(rest, theme, theme.text(), link_no));
            lines.extend(wrap(&spans, w, ind, ind + width(&bullet) + 1));
            continue;
        }
        let spans = inline(trimmed, theme, theme.text(), link_no);
        lines.extend(wrap(&spans, w, 0, 0));
    }
    while lines.last().map(|l| l.is_empty()).unwrap_or(false) {
        lines.pop();
    }
    Rendered { lines, headings, src_to_out }
}

fn heading(s: &str) -> Option<(usize, &str)> {
    let n = s.chars().take_while(|&c| c == '#').count();
    if (1..=6).contains(&n) && s[n..].starts_with(' ') {
        Some((n, s[n..].trim()))
    } else {
        None
    }
}

fn is_rule(s: &str) -> bool {
    let t = s.trim();
    t.len() >= 3 && (t.chars().all(|c| c == '-') || t.chars().all(|c| c == '*') || t.chars().all(|c| c == '_'))
}

fn list_item(s: &str) -> Option<(&str, &str)> {
    for m in ["- ", "* ", "+ "] {
        if let Some(r) = s.strip_prefix(m) {
            return Some((&m[..1], r));
        }
    }
    let digits = s.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits > 0 && digits <= 3 && s[digits..].starts_with(". ") {
        return Some((&s[..digits + 1], &s[digits + 2..]));
    }
    None
}

/// Text ohne Markdown-Zeichen (für Überschriften).
fn strip_inline(s: &str) -> String {
    s.replace("**", "").replace('`', "")
}

/// Hervorhebungen, Code, Wikilinks und Verweise.
pub fn inline(s: &str, theme: &Theme, base: Style, link_no: &dyn Fn(&str) -> Option<usize>) -> Spans {
    let mut out: Spans = Vec::new();
    let mut plain = String::new();
    let b = s.as_bytes();
    let mut i = 0;

    fn flush(out: &mut Spans, plain: &mut String, base: Style) {
        if !plain.is_empty() {
            out.push((std::mem::take(plain), base));
        }
    }

    while i < s.len() {
        let rest = &s[i..];
        if rest.starts_with("[[") {
            if let Some(q) = rest.strip_prefix("[[").and_then(|r| r.find("]]")) {
                let inner = &rest[2..2 + q];
                if !inner.is_empty() {
                    flush(&mut out, &mut plain, base);
                    let link = wikilinks(&rest[..q + 4]).into_iter().next();
                    if let Some(l) = link {
                        out.push((link_label(&l), theme.link()));
                        if let Some(n) = link_no(&l.target) {
                            out.push((format!("[{n}]"), theme.dim()));
                        }
                    }
                    i += q + 4;
                    continue;
                }
            }
        }
        if rest.starts_with("**") {
            if let Some(q) = rest.strip_prefix("**").and_then(|r| r.find("**")) {
                if q > 0 {
                    flush(&mut out, &mut plain, base);
                    out.push((rest[2..2 + q].to_string(), base.add_modifier(Modifier::BOLD)));
                    i += q + 4;
                    continue;
                }
            }
        }
        if rest.starts_with('`') {
            if let Some(q) = rest.strip_prefix('`').and_then(|r| r.find('`')) {
                if q > 0 {
                    flush(&mut out, &mut plain, base);
                    out.push((rest[1..1 + q].to_string(), theme.code()));
                    i += q + 2;
                    continue;
                }
            }
        }
        if rest.starts_with('*') && !rest.starts_with("**") && rest.len() > 2 && !rest[1..].starts_with(' ') {
            if let Some(q) = rest[1..].find('*') {
                let inner = &rest[1..1 + q];
                if q > 0 && !inner.ends_with(' ') {
                    flush(&mut out, &mut plain, base);
                    out.push((inner.to_string(), base.add_modifier(Modifier::ITALIC)));
                    i += q + 2;
                    continue;
                }
            }
        }
        if rest.starts_with('[') && !rest.starts_with("[[") {
            if let Some(q) = rest.find("](") {
                if let Some(e) = rest[q + 2..].find(')') {
                    let label = &rest[1..q];
                    if !label.is_empty() && !label.contains('\n') && !label.contains(']') {
                        flush(&mut out, &mut plain, base);
                        out.push((label.to_string(), theme.link()));
                        i += q + 2 + e + 1;
                        continue;
                    }
                }
            }
        }
        // ein Zeichen übernehmen
        let ch = rest.chars().next().unwrap();
        plain.push(ch);
        i += ch.len_utf8();
        let _ = b;
    }
    flush(&mut out, &mut plain, base);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(r: &Rendered) -> Vec<String> {
        r.lines
            .iter()
            .map(|l| l.iter().map(|(t, _)| t.as_str()).collect::<String>())
            .collect()
    }

    #[test]
    fn finds_wikilinks() {
        let l = wikilinks("Siehe [[Projects/A/Note#Teil|Alias]] und [[B/C]].");
        assert_eq!(l.len(), 2);
        assert_eq!(l[0].target, "Projects/A/Note");
        assert_eq!(l[0].anchor.as_deref(), Some("Teil"));
        assert_eq!(l[0].alias.as_deref(), Some("Alias"));
        assert_eq!(link_label(&l[1]), "C");
        assert!(wikilinks("[[offen").is_empty());
    }

    #[test]
    fn renders_blocks_within_width() {
        let t = Theme::mono();
        let text = "---\ntype: topic\n---\n# Titel\n\nEin sehr langer Absatz der sich über mehrere Zeilen erstrecken muss weil er länger ist.\n\n- Punkt eins mit [[X/Y|Alias]]\n  - Unterpunkt\n> Zitat hier\n```\ncode zeile\n```\n| a | b |\n";
        let r = render(text, 24, &t, &|_| Some(1));
        let p = plain(&r);
        assert_eq!(p[0], "Titel");
        assert_eq!(r.headings, vec![0]);
        for l in &p {
            assert!(width(l) <= 24, "{l:?}");
        }
        assert!(p.iter().any(|l| l.contains("Alias[1]")));
        assert!(p.iter().any(|l| l.starts_with("▎ Zitat")));
        assert!(p.iter().any(|l| l.contains("code zeile")));
        assert!(!p.iter().any(|l| l.contains("type: topic")));
    }

    #[test]
    fn inline_styles() {
        let t = Theme::color();
        let s = inline("a **fett** `code` *kursiv* [t](http://x) [[P/Q]]", &t, t.text(), &|_| None);
        let joined: String = s.iter().map(|(x, _)| x.as_str()).collect();
        assert_eq!(joined, "a fett code kursiv t Q");
    }

    #[test]
    fn crlf_and_empty() {
        let t = Theme::mono();
        let r = render("", 20, &t, &|_| None);
        assert!(r.lines.is_empty());
        let r = render("Zeile\r\n\r\nZwei\r\n", 20, &t, &|_| None);
        assert_eq!(plain(&r), vec!["Zeile", "", "Zwei"]);
    }
}
