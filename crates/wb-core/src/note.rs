//! Eine Notiz aus Bytes zerlegen (reine Funktion, ohne Index): Frontmatter, Titel,
//! Abschnitte, Links, Kennzahlen. Grundlage für Index, TUI-Fixtures und Migration.

use serde::{Deserialize, Serialize};

use crate::frontmatter::{self, FrontmatterState};
use crate::markdown;
use crate::model::{NotePath, NoteType, Section, WikiLink};
use crate::text::estimate_tokens;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ParsedNote {
    pub path: NotePath,
    pub frontmatter: FrontmatterState,
    /// Erste Inhaltszeile (1-basiert) nach dem Frontmatter.
    pub body_line: u32,
    /// Byte-Anfang des Inhalts im dekodierten Text.
    pub body_offset: usize,
    pub title: String,
    pub note_type: NoteType,
    pub type_raw: Option<String>,
    pub status: Option<String>,
    pub state: Option<String>,
    pub checked_on: Option<String>,
    pub summary: Option<String>,
    pub aliases: Vec<String>,
    /// Frontmatter `paths` (Übersichten).
    pub paths: Vec<String>,
    pub sections: Vec<Section>,
    pub links: Vec<WikiLink>,
    pub lines: u32,
    pub bytes: u64,
    pub tokens: u32,
    pub bom: bool,
    pub crlf: bool,
    pub invalid_utf8: bool,
}

/// Bytes → Text: BOM entfernen, ungültiges UTF-8 ersetzen. Liefert (Text, BOM, ungültig).
pub fn decode(bytes: &[u8]) -> (String, bool, bool) {
    let (b, bom) = match bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        Some(rest) => (rest, true),
        None => (bytes, false),
    };
    match std::str::from_utf8(b) {
        Ok(s) => (s.to_string(), bom, false),
        Err(_) => (String::from_utf8_lossy(b).into_owned(), bom, true),
    }
}

/// Zerlegt eine Notiz. `bytes` ist der rohe Dateiinhalt.
pub fn parse_note(path: &NotePath, bytes: &[u8]) -> ParsedNote {
    let (text, bom, invalid) = decode(bytes);
    let mut p = parse_text(path, &text);
    p.bom = bom;
    p.invalid_utf8 = invalid;
    p.bytes = bytes.len() as u64;
    p
}

/// Wie [`parse_note`], aber für bereits dekodierten Text (ohne BOM).
pub fn parse_text(path: &NotePath, text: &str) -> ParsedNote {
    let sp = frontmatter::split(text);
    let all = markdown::lines(text);
    let sections = markdown::sections(&all, sp.body_line);
    let links = crate::links::parse_wikilinks(text);
    let fm = sp.state.frontmatter();
    let get = |k: &str| fm.and_then(|f| f.get_str(k)).map(str::to_string);
    let type_raw = get("type");
    let title = sections
        .iter()
        .find(|s| s.level == 1)
        .map(|s| crate::markdown::plain_inline(&s.title))
        .filter(|t| !t.is_empty())
        .or_else(|| get("title"))
        .unwrap_or_else(|| path.stem().to_string());
    let summary = get("summary").or_else(|| markdown::first_sentence(&all, sp.body_line, 160));
    let mut aliases = fm.map(|f| f.get_list("aliases")).unwrap_or_default();
    aliases.extend(fm.map(|f| f.get_list("alias")).unwrap_or_default());
    let paths = fm.map(|f| f.get_list("paths")).unwrap_or_default();
    ParsedNote {
        path: path.clone(),
        note_type: NoteType::classify(type_raw.as_deref(), path),
        type_raw,
        status: get("status"),
        state: get("state"),
        checked_on: get("checked_on").map(|d| d.chars().take(10).collect()),
        summary,
        aliases,
        paths,
        title,
        body_line: sp.body_line,
        body_offset: sp.body_offset,
        frontmatter: sp.state,
        sections,
        links,
        lines: all.len() as u32,
        bytes: text.len() as u64,
        tokens: estimate_tokens(text),
        bom: false,
        crlf: text.contains("\r\n"),
        invalid_utf8: false,
    }
}

/// Absolute Pfade auf der Platte, die eine Übersicht in Backticks nennt (`G:/X/…`), als
/// Kandidaten für Projektwurzeln. Dateipfade werden auf ihren Ordner gekürzt.
pub fn infer_paths(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut rest = text;
    while let Some(a) = rest.find('`') {
        let after = &rest[a + 1..];
        let Some(b) = after.find('`') else { break };
        let span = &after[..b];
        rest = &after[b + 1..];
        let s = span.trim();
        let bytes = s.as_bytes();
        if bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && (bytes[2] == b'/' || bytes[2] == b'\\')
            && let Some(p) = normalize_disk_path(s)
                && !out.contains(&p) {
                    out.push(p);
                }
    }
    out.sort();
    out
}

/// Plattenpfad vereinheitlichen: `/`, Laufwerk groß, ohne abschließenden `/`, Dateien → Ordner.
pub fn normalize_disk_path(s: &str) -> Option<String> {
    // `G:/x.exe --flag` → nur der Pfad; Leerzeichen in Ordnernamen bleiben
    let s = match s.find(" -").or_else(|| s.find(" \"")) {
        Some(i) => &s[..i],
        None => s,
    };
    let mut p = s.replace('\\', "/");
    while p.ends_with('/') && p.len() > 3 {
        p.pop();
    }
    let p = p.trim_end_matches(['.', ',', ';', ')']).to_string();
    let mut chars: Vec<char> = p.chars().collect();
    if chars.len() < 2 {
        return None;
    }
    chars[0] = chars[0].to_ascii_uppercase();
    let mut p: String = chars.into_iter().collect();
    if p.contains('*') || p.contains('<') || p.contains('?') {
        return None;
    }
    let last = p.rsplit('/').next().unwrap_or("");
    let looks_file = !last.starts_with('.') && last.rfind('.').is_some_and(|i| i > 0 && last.len() - i <= 6);
    if looks_file
        && let Some(i) = p.rfind('/') {
            p.truncate(i.max(3));
        }
    if p.len() <= 3 {
        return None; // nur Laufwerk
    }
    Some(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_basic_note() {
        let t = "---\ntype: project-topic\nstatus: rc7-pending\nchecked_on: 2026-11-09\n---\n# Werkzeug *Pro*\n\nNative Dateien über ein Prüfgerät. Danach mehr.\n## Offen\n- a\n";
        let n = parse_note(&NotePath::new("Projects/Beispiel/Werkzeug.md"), t.as_bytes());
        assert_eq!(n.note_type, NoteType::Topic);
        assert_eq!(n.title, "Werkzeug *Pro*");
        assert_eq!(n.summary.as_deref(), Some("Native Dateien über ein Prüfgerät."));
        assert_eq!(n.checked_on.as_deref(), Some("2026-11-09"));
        assert_eq!(n.sections.len(), 2);
    }

    #[test]
    fn infer_paths_from_overview() {
        let t = "Vault `G:/Beispiel/wissen`, Datei `C:\\Nutzer\\demo\\.konfig\\einstellungen.json`, `npm run`, `G:/X Y/z.md` und `C:/`";
        assert_eq!(infer_paths(t), ["C:/Nutzer/demo/.konfig", "G:/Beispiel/wissen", "G:/X Y"]);
    }
}
