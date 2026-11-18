//! Wikilinks finden und umschreiben. Links in Codeblöcken und Inline-Code zählen nicht
//! (wie in `wiki_lint.py`).

use crate::model::WikiLink;

/// Findet alle Wikilinks. Byte-Bereiche beziehen sich auf `text`.
pub fn parse_wikilinks(text: &str) -> Vec<WikiLink> {
    let mut out = Vec::new();
    let mut fence: Option<(u8, usize)> = None;
    let mut offset = 0usize;
    for (idx, raw_line) in text.split_inclusive('\n').enumerate() {
        let line_no = idx as u32 + 1;
        let line = raw_line.trim_end_matches(['\n', '\r']);
        let base = offset;
        offset += raw_line.len();
        if let Some((ch, n)) = fence_marker(line) {
            match fence {
                None => fence = Some((ch, n)),
                Some((fc, fnn)) if fc == ch && n >= fnn => fence = None,
                _ => {}
            }
            continue;
        }
        if fence.is_some() {
            continue;
        }
        let code = inline_code_ranges(line);
        let b = line.as_bytes();
        let mut i = 0usize;
        while i + 1 < b.len() {
            if b[i] == b'[' && b[i + 1] == b'[' && !(i > 0 && b[i - 1] == b'\\') && !in_ranges(&code, i)
                && let Some(rel_end) = line[i + 2..].find("]]") {
                    let inner = &line[i + 2..i + 2 + rel_end];
                    if !inner.is_empty() && !inner.contains('[') {
                        let embed = i > 0 && b[i - 1] == b'!';
                        let start = if embed { i - 1 } else { i };
                        let end = i + 2 + rel_end + 2;
                        let (target_part, alias, escaped_pipe) = match inner.find('|') {
                            Some(p) => {
                                let (t, esc) = match inner[..p].strip_suffix('\\') {
                                    Some(t) => (t, true),
                                    None => (&inner[..p], false),
                                };
                                (t, Some(inner[p + 1..].trim().to_string()), esc)
                            }
                            None => (inner, None, false),
                        };
                        let (target, heading) = match target_part.find('#') {
                            Some(h) => (&target_part[..h], Some(target_part[h + 1..].trim().to_string())),
                            None => (target_part, None),
                        };
                        out.push(WikiLink {
                            target: target.trim().to_string(),
                            heading: heading.filter(|h| !h.is_empty()),
                            alias: alias.filter(|a| !a.is_empty()),
                            embed,
                            escaped_pipe,
                            line: line_no,
                            start: base + start,
                            end: base + end,
                        });
                        i = end;
                        continue;
                    }
                }
            i += 1;
        }
    }
    out
}

/// Codezaun ``` oder ~~~ (bis drei Leerzeichen Einrückung): Zeichen und Länge.
pub(crate) fn fence_marker(line: &str) -> Option<(u8, usize)> {
    let t = line.trim_start_matches(' ');
    if line.len() - t.len() > 3 {
        return None;
    }
    let b = t.as_bytes();
    let ch = *b.first()?;
    if ch != b'`' && ch != b'~' {
        return None;
    }
    let n = b.iter().take_while(|&&c| c == ch).count();
    if n >= 3 { Some((ch, n)) } else { None }
}

fn inline_code_ranges(line: &str) -> Vec<(usize, usize)> {
    let b = line.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'`' {
            let n = b[i..].iter().take_while(|&&c| c == b'`').count();
            let open = i;
            let mut j = i + n;
            let mut closed = None;
            while j < b.len() {
                if b[j] == b'`' {
                    let m = b[j..].iter().take_while(|&&c| c == b'`').count();
                    if m == n {
                        closed = Some(j + m);
                        break;
                    }
                    j += m;
                } else {
                    j += 1;
                }
            }
            match closed {
                Some(e) => {
                    out.push((open, e));
                    i = e;
                }
                None => i += n,
            }
        } else {
            i += 1;
        }
    }
    out
}

fn in_ranges(r: &[(usize, usize)], i: usize) -> bool {
    r.iter().any(|&(a, b)| i >= a && i < b)
}

impl WikiLink {
    /// Link als Text mit neuem Ziel; Überschrift, Alias, Embed und Tabellen-Escape bleiben.
    pub fn render_with_target(&self, target: &str) -> String {
        let mut s = String::new();
        if self.embed {
            s.push('!');
        }
        s.push_str("[[");
        s.push_str(target);
        if let Some(h) = &self.heading {
            s.push('#');
            s.push_str(h);
        }
        if let Some(a) = &self.alias {
            s.push_str(if self.escaped_pipe { "\\|" } else { "|" });
            s.push_str(a);
        }
        s.push_str("]]");
        s
    }
}

/// Schreibt Links um. `f` liefert für einen Link den neuen Linktext (ganz, mit `[[ ]]`)
/// oder `None` (unverändert). Alles andere bleibt bytegleich.
pub fn rewrite_wikilinks(text: &str, mut f: impl FnMut(&WikiLink) -> Option<String>) -> String {
    let links = parse_wikilinks(text);
    let mut out = String::with_capacity(text.len());
    let mut pos = 0;
    for l in &links {
        if let Some(rep) = f(l) {
            out.push_str(&text[pos..l.start]);
            out.push_str(&rep);
            pos = l.end;
        }
    }
    out.push_str(&text[pos..]);
    out
}

/// Ziel eines Links ohne `.md`, mit `/`, NFC (zum Auflösen).
pub fn normalize_target(target: &str) -> String {
    crate::model::path_key(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_links_with_alias_heading_embed() {
        let t = "a [[Projects/X/00 Overview|X]] b ![[Bild.png]]\n| [[T\\|Alias]] |\n[[N#Stand (2026)]]";
        let l = parse_wikilinks(t);
        assert_eq!(l.len(), 4);
        assert_eq!(l[0].target, "Projects/X/00 Overview");
        assert_eq!(l[0].alias.as_deref(), Some("X"));
        assert!(l[1].embed);
        assert!(l[2].escaped_pipe);
        assert_eq!(l[2].target, "T");
        assert_eq!(l[3].heading.as_deref(), Some("Stand (2026)"));
        assert_eq!(l[3].line, 3);
        assert_eq!(&t[l[0].start..l[0].end], "[[Projects/X/00 Overview|X]]");
    }

    #[test]
    fn skips_code() {
        let t = "`[[nein]]` ja [[ja]]\n```\n[[nein2]]\n```\n``x [[nein3]] ``";
        let l = parse_wikilinks(t);
        assert_eq!(l.iter().map(|x| x.target.as_str()).collect::<Vec<_>>(), ["ja"]);
    }

    #[test]
    fn rewrite_keeps_rest() {
        let t = "vor [[Alt|A]] mitte [[Bleibt]] ende\n| [[Alt\\|B]] |";
        let out = rewrite_wikilinks(t, |l| (l.target == "Alt").then(|| l.render_with_target("Neu/Ort")));
        assert_eq!(out, "vor [[Neu/Ort|A]] mitte [[Bleibt]] ende\n| [[Neu/Ort\\|B]] |");
    }
}
