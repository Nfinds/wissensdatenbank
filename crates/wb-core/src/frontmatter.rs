//! Frontmatter: flache YAML-Teilmenge (Skalare, Inline-Listen, Block-Listen).
//!
//! Lesen ist nachsichtig und meldet Probleme, statt abzubrechen. Unbekannte Schlüssel,
//! Kommentare und nicht verstandene Zeilen bleiben beim Schreiben wörtlich erhalten:
//! unveränderte Einträge werden aus ihrem Originaltext ausgegeben.

use serde::{Deserialize, Serialize};

/// Wert eines Frontmatter-Eintrags.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "lowercase")]
pub enum FmValue {
    Scalar(String),
    List(Vec<String>),
    /// Nicht verstandener Inhalt (verschachtelte Abbildung, Blockskalar `|`/`>`), wörtlich.
    Raw(String),
}

impl FmValue {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            FmValue::Scalar(s) => Some(s),
            _ => None,
        }
    }
    /// Liste; ein Skalar gilt als Liste mit einem Element (leer → leer).
    pub fn to_list(&self) -> Vec<String> {
        match self {
            FmValue::List(v) => v.clone(),
            FmValue::Scalar(s) if !s.is_empty() => vec![s.clone()],
            _ => Vec::new(),
        }
    }
}

/// Ein Eintrag. `key` ist leer für Kommentare und nicht verstandene Zeilen (nur zum Erhalten).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FmEntry {
    pub key: String,
    pub value: FmValue,
    /// 1-basierte Zeile in der Datei.
    pub line: u32,
    /// Originaltext (eine oder mehrere Zeilen ohne Zeilenende). `None` nach Änderung.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FmProblem {
    pub line: u32,
    /// `fm-duplicate-key`, `fm-invalid-line`, `fm-unclosed-quote`, `fm-bom`.
    pub code: String,
    pub message: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Frontmatter {
    pub entries: Vec<FmEntry>,
    pub problems: Vec<FmProblem>,
}

/// Zustand des Frontmatters einer Datei.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "lowercase")]
pub enum FrontmatterState {
    /// Datei beginnt nicht mit `---`.
    Missing,
    /// Block `---` … `---` (oder `...`) in den Zeilen `start_line..=end_line`.
    Present { fm: Frontmatter, start_line: u32, end_line: u32 },
    /// Beginnt mit `---`, aber ohne schließende Zeile; der ganze Text gilt als Inhalt.
    Unclosed,
}

impl FrontmatterState {
    pub fn frontmatter(&self) -> Option<&Frontmatter> {
        match self {
            FrontmatterState::Present { fm, .. } => Some(fm),
            _ => None,
        }
    }
    /// Probleme als Codes für Befunde (fehlend/ungeschlossen eingeschlossen).
    pub fn problem_codes(&self) -> Vec<String> {
        match self {
            FrontmatterState::Missing => vec!["fm-missing".into()],
            FrontmatterState::Unclosed => vec!["fm-unclosed".into()],
            FrontmatterState::Present { fm, .. } => fm.problems.iter().map(|p| p.code.clone()).collect(),
        }
    }
}

/// Ergebnis der Zerlegung: Frontmatter-Zustand, Byte-Anfang und erste Zeile des Inhalts.
#[derive(Clone, Debug)]
pub struct Split {
    pub state: FrontmatterState,
    pub body_offset: usize,
    /// 1-basiert.
    pub body_line: u32,
}

fn strip_cr(l: &str) -> &str {
    l.strip_suffix('\r').unwrap_or(l)
}

/// Zerlegt Text (ohne BOM) in Frontmatter und Inhalt.
pub fn split(text: &str) -> Split {
    let mut offset = 0usize;
    let mut lines: Vec<(usize, &str)> = Vec::new();
    for l in text.split_inclusive('\n') {
        lines.push((offset, l));
        offset += l.len();
    }
    let first = lines.first().map(|(_, l)| strip_cr(l.strip_suffix('\n').unwrap_or(l)).trim_end());
    if first != Some("---") {
        return Split { state: FrontmatterState::Missing, body_offset: 0, body_line: 1 };
    }
    let mut close = None;
    for (i, (_, l)) in lines.iter().enumerate().skip(1) {
        let t = strip_cr(l.strip_suffix('\n').unwrap_or(l)).trim_end();
        if t == "---" || t == "..." {
            close = Some(i);
            break;
        }
    }
    let Some(ci) = close else {
        return Split { state: FrontmatterState::Unclosed, body_offset: 0, body_line: 1 };
    };
    let inner: Vec<&str> = lines[1..ci].iter().map(|(_, l)| strip_cr(l.strip_suffix('\n').unwrap_or(l))).collect();
    let fm = parse_block(&inner, 2);
    let body_offset = lines.get(ci + 1).map(|(o, _)| *o).unwrap_or(text.len());
    Split {
        state: FrontmatterState::Present { fm, start_line: 1, end_line: ci as u32 + 1 },
        body_offset,
        body_line: ci as u32 + 2,
    }
}

/// Liest die Zeilen zwischen den Trennern. `first_line` ist die Dateizeile der ersten Zeile.
pub fn parse_block(lines: &[&str], first_line: u32) -> Frontmatter {
    let mut fm = Frontmatter::default();
    let mut i = 0usize;
    while i < lines.len() {
        let line = lines[i];
        let lno = first_line + i as u32;
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') || line.starts_with(' ') || line.starts_with('\t') && !t.starts_with('-') {
            // Kommentar, Leerzeile oder verwaiste Einrückung: wörtlich erhalten
            if !t.is_empty() && !t.starts_with('#') {
                fm.problems.push(FmProblem {
                    line: lno,
                    code: "fm-invalid-line".into(),
                    message: format!("eingerückte Zeile ohne Schlüssel: {}", short(t)),
                });
            }
            fm.entries.push(FmEntry { key: String::new(), value: FmValue::Raw(line.to_string()), line: lno, raw: Some(line.to_string()) });
            i += 1;
            continue;
        }
        let Some((key, rest)) = split_key(line) else {
            fm.problems.push(FmProblem {
                line: lno,
                code: "fm-invalid-line".into(),
                message: format!("Zeile ohne `schlüssel: wert`: {}", short(t)),
            });
            fm.entries.push(FmEntry { key: String::new(), value: FmValue::Raw(line.to_string()), line: lno, raw: Some(line.to_string()) });
            i += 1;
            continue;
        };
        let mut raw_lines = vec![line.to_string()];
        let rest_t = strip_comment(rest.trim());
        let value = if rest_t.is_empty() {
            // Blockliste oder verschachtelter Inhalt in den folgenden Zeilen
            let mut items = Vec::new();
            let mut nested = Vec::new();
            let mut j = i + 1;
            while j < lines.len() {
                let l = lines[j];
                let lt = l.trim_start();
                if lt.starts_with("- ") || lt == "-" {
                    items.push(parse_scalar(strip_comment(lt[1..].trim()), lno + (j - i) as u32, &mut fm.problems));
                } else if (l.starts_with(' ') || l.starts_with('\t')) && !lt.is_empty() {
                    nested.push(l.to_string());
                } else {
                    break;
                }
                raw_lines.push(l.to_string());
                j += 1;
            }
            i = j;
            if !nested.is_empty() {
                FmValue::Raw(raw_lines[1..].join("\n"))
            } else if !items.is_empty() {
                FmValue::List(items)
            } else {
                FmValue::Scalar(String::new())
            }
        } else if rest_t.starts_with('|') || rest_t.starts_with('>') {
            let mut j = i + 1;
            while j < lines.len() && (lines[j].starts_with(' ') || lines[j].starts_with('\t') || lines[j].trim().is_empty()) {
                if lines[j].trim().is_empty() && j + 1 < lines.len() && !lines[j + 1].starts_with(' ') {
                    break;
                }
                raw_lines.push(lines[j].to_string());
                j += 1;
            }
            i = j;
            FmValue::Raw(raw_lines.join("\n"))
        } else if rest_t.starts_with('[') {
            i += 1;
            FmValue::List(parse_inline_list(rest_t, lno, &mut fm.problems))
        } else {
            i += 1;
            FmValue::Scalar(parse_scalar(rest_t, lno, &mut fm.problems))
        };
        if fm.entries.iter().any(|e| e.key == key) {
            fm.problems.push(FmProblem {
                line: lno,
                code: "fm-duplicate-key".into(),
                message: format!("Schlüssel `{key}` doppelt; der letzte gilt"),
            });
        }
        fm.entries.push(FmEntry { key: key.to_string(), value, line: lno, raw: Some(raw_lines.join("\n")) });
    }
    fm
}

fn short(s: &str) -> String {
    crate::text::truncate_chars(s, 60)
}

/// `schlüssel: rest` — Schlüssel endet am ersten `:` vor Leerzeichen oder Zeilenende.
fn split_key(line: &str) -> Option<(&str, &str)> {
    let bytes = line.as_bytes();
    let mut in_quote: Option<u8> = None;
    for (i, &b) in bytes.iter().enumerate() {
        match in_quote {
            Some(q) if b == q => in_quote = None,
            Some(_) => {}
            None if (b == b'"' || b == b'\'') && i == 0 => in_quote = Some(b),
            None if b == b':' && (i + 1 == bytes.len() || bytes[i + 1] == b' ' || bytes[i + 1] == b'\t') => {
                let key = line[..i].trim().trim_matches(|c| c == '"' || c == '\'');
                if key.is_empty() || key.starts_with('-') {
                    return None;
                }
                return Some((key, &line[i + 1..]));
            }
            None => {}
        }
    }
    None
}

/// Entfernt einen Kommentar ` # …` außerhalb von Anführungszeichen.
fn strip_comment(s: &str) -> &str {
    let bytes = s.as_bytes();
    let mut q: Option<u8> = None;
    for (i, &b) in bytes.iter().enumerate() {
        match q {
            Some(c) if b == c => q = None,
            Some(_) => {}
            None if b == b'"' || b == b'\'' => q = Some(b),
            None if b == b'#' && i > 0 && (bytes[i - 1] == b' ' || bytes[i - 1] == b'\t') => return s[..i].trim_end(),
            None => {}
        }
    }
    s
}

fn parse_scalar(s: &str, line: u32, problems: &mut Vec<FmProblem>) -> String {
    let s = s.trim();
    if let Some(rest) = s.strip_prefix('"') {
        let mut out = String::new();
        let mut chars = rest.chars();
        while let Some(c) = chars.next() {
            match c {
                '"' => return out,
                '\\' => match chars.next() {
                    Some('n') => out.push('\n'),
                    Some('t') => out.push('\t'),
                    Some(o) => out.push(o),
                    None => {}
                },
                c => out.push(c),
            }
        }
        problems.push(FmProblem { line, code: "fm-unclosed-quote".into(), message: "Anführungszeichen nicht geschlossen".into() });
        return out;
    }
    if let Some(rest) = s.strip_prefix('\'') {
        let mut out = String::new();
        let mut chars = rest.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '\'' {
                if chars.peek() == Some(&'\'') {
                    out.push('\'');
                    chars.next();
                } else {
                    return out;
                }
            } else {
                out.push(c);
            }
        }
        problems.push(FmProblem { line, code: "fm-unclosed-quote".into(), message: "Anführungszeichen nicht geschlossen".into() });
        return out;
    }
    s.to_string()
}

fn parse_inline_list(s: &str, line: u32, problems: &mut Vec<FmProblem>) -> Vec<String> {
    let inner = s.trim().strip_prefix('[').unwrap_or(s);
    let inner = match inner.trim_end().strip_suffix(']') {
        Some(x) => x,
        None => {
            problems.push(FmProblem { line, code: "fm-invalid-line".into(), message: "Inline-Liste ohne `]`".into() });
            inner
        }
    };
    let mut items = Vec::new();
    let mut cur = String::new();
    let mut q: Option<char> = None;
    for c in inner.chars() {
        match q {
            Some(qc) if c == qc => {
                q = None;
                cur.push(c);
            }
            Some(_) => cur.push(c),
            None if c == '"' || c == '\'' => {
                q = Some(c);
                cur.push(c);
            }
            None if c == ',' => {
                let t = cur.trim();
                if !t.is_empty() {
                    items.push(parse_scalar(t, line, problems));
                }
                cur.clear();
            }
            None => cur.push(c),
        }
    }
    let t = cur.trim();
    if !t.is_empty() {
        items.push(parse_scalar(t, line, problems));
    }
    items
}

fn needs_quotes(s: &str) -> bool {
    s.is_empty()
        || s.starts_with(|c: char| "[]{}#&*!|>'\"%@`,-?:".contains(c) || c.is_whitespace())
        || s.ends_with(|c: char| c.is_whitespace() || c == ':')
        || s.contains(": ")
        || s.contains(" #")
        || s.contains('\n')
        || s.contains('"')
}

fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn render_scalar(s: &str) -> String {
    if needs_quotes(s) { quote(s) } else { s.to_string() }
}

fn render_list_item(s: &str) -> String {
    if needs_quotes(s) || s.contains(',') || s.contains('[') || s.contains(']') || s.contains(':') || s.contains('\'') {
        quote(s)
    } else {
        s.to_string()
    }
}

impl Frontmatter {
    /// Wert eines Schlüssels (bei Doppelten der letzte).
    pub fn get(&self, key: &str) -> Option<&FmValue> {
        self.entries.iter().rev().find(|e| !e.key.is_empty() && e.key == key).map(|e| &e.value)
    }
    /// Skalarwert, leer → `None`.
    pub fn get_str(&self, key: &str) -> Option<&str> {
        self.get(key).and_then(|v| v.as_str()).map(str::trim).filter(|s| !s.is_empty())
    }
    pub fn get_list(&self, key: &str) -> Vec<String> {
        self.get(key).map(|v| v.to_list()).unwrap_or_default()
    }
    /// Setzt einen Wert. Der erste vorhandene Eintrag wird ersetzt (an seiner Stelle),
    /// weitere Doppelte entfernt; sonst wird angehängt.
    pub fn set(&mut self, key: &str, value: FmValue) {
        let mut first = true;
        let mut line = 0;
        self.entries.retain_mut(|e| {
            if e.key == key {
                if first {
                    first = false;
                    e.value = value.clone();
                    e.raw = None;
                    line = e.line;
                    true
                } else {
                    false
                }
            } else {
                true
            }
        });
        if first {
            self.entries.push(FmEntry { key: key.to_string(), value, line: 0, raw: None });
        }
    }
    pub fn remove(&mut self, key: &str) -> bool {
        let n = self.entries.len();
        self.entries.retain(|e| e.key != key);
        n != self.entries.len()
    }
    /// Schlüssel in Dateireihenfolge (ohne Kommentare, ohne Doppelte).
    pub fn keys(&self) -> Vec<&str> {
        let mut out: Vec<&str> = Vec::new();
        for e in &self.entries {
            if !e.key.is_empty() && !out.contains(&e.key.as_str()) {
                out.push(&e.key);
            }
        }
        out
    }
    /// Block mit Trennern `---` und LF-Zeilenenden. Unveränderte Einträge wörtlich.
    pub fn to_yaml(&self) -> String {
        let mut out = String::from("---\n");
        for e in &self.entries {
            if let Some(raw) = &e.raw {
                out.push_str(raw);
                out.push('\n');
                continue;
            }
            match &e.value {
                FmValue::Scalar(s) => {
                    out.push_str(&e.key);
                    out.push_str(": ");
                    out.push_str(&render_scalar(s));
                }
                FmValue::List(items) => {
                    out.push_str(&e.key);
                    out.push_str(": [");
                    out.push_str(&items.iter().map(|s| render_list_item(s)).collect::<Vec<_>>().join(", "));
                    out.push(']');
                }
                FmValue::Raw(r) => {
                    if e.key.is_empty() {
                        out.push_str(r);
                    } else {
                        out.push_str(&e.key);
                        out.push_str(":\n");
                        out.push_str(r);
                    }
                }
            }
            out.push('\n');
        }
        out.push_str("---\n");
        out
    }
}

/// Ersetzt (oder ergänzt) das Frontmatter eines Textes; der Inhalt bleibt bytegleich.
pub fn replace_in_text(text: &str, fm: &Frontmatter) -> String {
    let sp = split(text);
    let body = &text[sp.body_offset..];
    format!("{}{}", fm.to_yaml(), body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_scalars_and_lists() {
        let t = "---\ntype: topic\nchecked_on: 2026-11-09\npaths: [\"G:/Beispiel\", other]\naliases:\n  - Eins\n  - 'Zwei'\nsummary: \"a: b\"\n---\n# Titel\n";
        let sp = split(t);
        let FrontmatterState::Present { fm, end_line, .. } = &sp.state else { panic!() };
        assert_eq!(*end_line, 9);
        assert_eq!(sp.body_line, 10);
        assert_eq!(&t[sp.body_offset..], "# Titel\n");
        assert_eq!(fm.get_str("type"), Some("topic"));
        assert_eq!(fm.get_list("paths"), ["G:/Beispiel", "other"]);
        assert_eq!(fm.get_list("aliases"), ["Eins", "Zwei"]);
        assert_eq!(fm.get_str("summary"), Some("a: b"));
        assert!(fm.problems.is_empty());
    }

    #[test]
    fn roundtrip_keeps_unknown() {
        let t = "---\ntype: project\n# Kommentar\nweird:\n  nested: 1\nstatus: rc7-installed\n---\nText\n";
        let sp = split(t);
        let mut fm = sp.state.frontmatter().unwrap().clone();
        assert_eq!(replace_in_text(t, &fm), t);
        fm.set("type", FmValue::Scalar("overview".into()));
        fm.set("paths", FmValue::List(vec!["G:/X Y".into()]));
        let out = replace_in_text(t, &fm);
        assert_eq!(out, "---\ntype: overview\n# Kommentar\nweird:\n  nested: 1\nstatus: rc7-installed\npaths: [\"G:/X Y\"]\n---\nText\n");
    }

    #[test]
    fn broken_cases() {
        assert!(matches!(split("kein frontmatter").state, FrontmatterState::Missing));
        assert!(matches!(split("---\ntype: x\nohne ende").state, FrontmatterState::Unclosed));
        let sp = split("---\r\ntype: a\r\ntype: b\r\nnur text\r\n---\r\nBody");
        let fm = sp.state.frontmatter().unwrap();
        assert_eq!(fm.get_str("type"), Some("b"));
        let codes: Vec<_> = fm.problems.iter().map(|p| p.code.as_str()).collect();
        assert_eq!(codes, ["fm-duplicate-key", "fm-invalid-line"]);
        assert_eq!(sp.body_line, 6);
    }
}
