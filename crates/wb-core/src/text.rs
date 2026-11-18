//! Textwerkzeuge: Normalisierung, Faltung, Wortzerlegung, Stämme DE/EN, Tokenschätzung, Sätze.

use rust_stemmers::{Algorithm, Stemmer};
use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::is_combining_mark;

/// Unicode-NFC.
pub fn nfc(s: &str) -> String {
    if s.is_ascii() { s.to_string() } else { s.nfc().collect() }
}

/// Faltung für Vergleich und Index: klein, diakritische Zeichen weg (ä→a, é→e), ß→ss,
/// und die deutschen Umschreibungen ae/oe/ue → a/o/u. Wird auf Index und Anfrage gleich
/// angewandt, deshalb finden `Größe`, `groesse` und `Grosse` dasselbe.
pub fn fold(s: &str) -> String {
    let mut tmp = String::with_capacity(s.len());
    if s.is_ascii() {
        tmp.push_str(&s.to_ascii_lowercase());
    } else {
        for c in s.nfd() {
            if is_combining_mark(c) {
                continue;
            }
            match c {
                'ß' | 'ẞ' => tmp.push_str("ss"),
                'æ' | 'Æ' => tmp.push_str("ae"),
                'ø' | 'Ø' => tmp.push('o'),
                _ => tmp.extend(c.to_lowercase()),
            }
        }
    }
    // ae/oe/ue → a/o/u
    let mut out = String::with_capacity(tmp.len());
    let mut prev: Option<char> = None;
    for c in tmp.chars() {
        if c == 'e' && matches!(prev, Some('a' | 'o' | 'u')) {
            prev = Some('e');
            continue;
        }
        out.push(c);
        prev = Some(c);
    }
    out
}

/// Wortzeichen: Buchstaben und Ziffern.
#[inline]
pub fn is_word_char(c: char) -> bool {
    c.is_alphanumeric()
}

/// Wörter mit Byte-Anfang. Trennt an allem, was kein Buchstabe und keine Ziffer ist.
pub fn words(text: &str) -> impl Iterator<Item = (usize, &str)> {
    let mut iter = text.char_indices().peekable();
    std::iter::from_fn(move || {
        loop {
            let (start, c) = iter.next()?;
            if !is_word_char(c) {
                continue;
            }
            let mut end = start + c.len_utf8();
            while let Some(&(i, c2)) = iter.peek() {
                if is_word_char(c2) {
                    end = i + c2.len_utf8();
                    iter.next();
                } else {
                    break;
                }
            }
            return Some((start, &text[start..end]));
        }
    })
}

/// Zerlegt Text in Indexformen: je Wort die gefaltete Form und die gefalteten Stämme
/// (deutsch und englisch), ohne Doppelte.
pub struct Analyzer {
    de: Stemmer,
    en: Stemmer,
}

impl Default for Analyzer {
    fn default() -> Self {
        Analyzer::new()
    }
}

impl Analyzer {
    pub fn new() -> Self {
        Analyzer { de: Stemmer::create(Algorithm::German), en: Stemmer::create(Algorithm::English) }
    }

    /// Formen eines Wortes: [gefaltet, Stamm DE, Stamm EN], Doppelte entfernt, Reihenfolge fest.
    pub fn forms(&self, word: &str) -> Vec<String> {
        let lower = word.to_lowercase();
        let f = fold(&lower);
        let mut out = vec![f];
        if lower.chars().any(|c| c.is_alphabetic()) && lower.chars().count() > 2 {
            let d = fold(&self.de.stem(&lower));
            let e = fold(&self.en.stem(&lower));
            if !out.contains(&d) && !d.is_empty() {
                out.push(d);
            }
            if !out.contains(&e) && !e.is_empty() {
                out.push(e);
            }
        }
        out
    }

    /// Indextext: alle Formen aller Wörter, durch Leerzeichen getrennt.
    pub fn index_text(&self, text: &str) -> String {
        let mut out = String::with_capacity(text.len() * 2);
        for (_, w) in words(text) {
            for f in self.forms(w) {
                if !out.is_empty() {
                    out.push(' ');
                }
                out.push_str(&f);
            }
        }
        out
    }
}

/// Geschätzte Token: Zeichen / 3,5, aufgerundet (Formel steht in `wb --help`).
pub fn estimate_tokens(s: &str) -> u32 {
    let chars = s.chars().count() as u64;
    (chars * 2).div_ceil(7) as u32
}

/// Füllwörter (deutsch und englisch, gefaltet), die in Anfragen ignoriert werden,
/// solange andere Begriffe übrig bleiben.
pub fn is_stopword(folded: &str) -> bool {
    const SW: &[&str] = &[
        "der", "die", "das", "den", "dem", "des", "ein", "eine", "einen", "einem", "einer", "und", "oder", "aber",
        "ist", "sind", "war", "wie", "was", "wo", "wer", "wann", "warum", "mit", "von", "vom", "zu", "zum", "zur",
        "im", "in", "an", "am", "auf", "aus", "bei", "fur", "uber", "nach", "nicht", "kein", "keine", "auch", "als",
        "es", "ich", "du", "wir", "sie", "er", "man", "sich", "so", "noch", "nur", "wird", "werden", "hat", "haben",
        "dass", "da", "dann", "denn", "the", "a", "an", "and", "or", "of", "to", "is", "are", "was", "for", "on",
        "with", "by", "at", "from", "how", "what", "where", "which", "who", "why", "it", "this", "that", "be", "as",
        "not", "do", "does", "can", "into", "via",
    ];
    SW.contains(&folded)
}

/// Satzgrenzen als Byte-Bereiche. Trennt nach `.`, `!`, `?`, `:` gefolgt von Leerraum und
/// Großbuchstabe/Ziffer/Markdown-Zeichen, sowie an Zeilenumbrüchen. Kurze Abkürzungen
/// (`z. B.`, `u. a.`, `e.g.`) trennen nicht.
pub fn sentences(text: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut start = 0usize;
    let bytes = text.as_bytes();
    let mut i = 0usize;
    let push = |out: &mut Vec<(usize, usize)>, s: usize, e: usize| {
        let seg = &text[s..e];
        let trimmed_start = s + (seg.len() - seg.trim_start().len());
        let trimmed_end = e - (seg.len() - seg.trim_end().len());
        if trimmed_end > trimmed_start {
            out.push((trimmed_start, trimmed_end));
        }
    };
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'\n' {
            push(&mut out, start, i);
            start = i + 1;
        } else if matches!(b, b'.' | b'!' | b'?' | b';') && i + 1 < bytes.len() && bytes[i + 1] == b' ' {
            // Nächstes Nicht-Leerzeichen
            let mut j = i + 1;
            while j < bytes.len() && bytes[j] == b' ' {
                j += 1;
            }
            let next = text[j..].chars().next();
            let starts_new = match next {
                Some(c) => c.is_uppercase() || c.is_ascii_digit() || matches!(c, '-' | '*' | '[' | '`' | '(' | '„' | '"'),
                None => false,
            };
            // Abkürzung: Wort vor dem Punkt höchstens 2 Buchstaben (z. B., u. a., e.g., bzw. ausgenommen)
            let before = &text[start..i];
            let last_word = before.rsplit(|c: char| !c.is_alphanumeric()).next().unwrap_or("");
            let abbrev = b == b'.'
                && (last_word.chars().count() <= 2 && last_word.chars().all(|c| c.is_alphabetic())
                    || matches!(last_word, "bzw" | "ca" | "vgl" | "ggf" | "usw" | "etc" | "inkl" | "evtl" | "Nr" | "Abs" | "vs"));
            if starts_new && !abbrev {
                push(&mut out, start, i + 1);
                start = i + 1;
            }
        }
        i += 1;
    }
    push(&mut out, start, bytes.len());
    out
}

/// Levenshtein-Abstand (für Tippfehler gegen den Index-Wortschatz), mit Abbruch über `max`.
pub fn edit_distance(a: &str, b: &str, max: usize) -> Option<usize> {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.len().abs_diff(b.len()) > max {
        return None;
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0; b.len() + 1];
    for i in 1..=a.len() {
        cur[0] = i;
        let mut row_min = cur[0];
        for j in 1..=b.len() {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
            row_min = row_min.min(cur[j]);
        }
        if row_min > max {
            return None;
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    let d = prev[b.len()];
    if d <= max { Some(d) } else { None }
}

/// Kürzt auf höchstens `max` Zeichen an einer Wortgrenze und hängt `…` an.
pub fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut end = s.char_indices().nth(max).map(|(i, _)| i).unwrap_or(s.len());
    if let Some(sp) = s[..end].rfind(' ')
        && sp > end / 2 {
            end = sp;
        }
    format!("{}…", s[..end].trim_end())
}

/// Fasst Leerraum zu einem Leerzeichen zusammen.
pub fn squash_ws(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut ws = false;
    for c in s.chars() {
        if c.is_whitespace() {
            ws = true;
        } else {
            if ws && !out.is_empty() {
                out.push(' ');
            }
            ws = false;
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fold_umlauts_and_transliteration() {
        assert_eq!(fold("Größe"), "grosse");
        assert_eq!(fold("groesse"), "grosse");
        assert_eq!(fold("GRÜN"), "grun");
        assert_eq!(fold("gruen"), "grun");
        assert_eq!(fold("Café"), "cafe");
        // NFD-Eingabe faltet wie NFC
        assert_eq!(fold("Gro\u{0308}sse"), fold("Grösse"));
    }

    #[test]
    fn words_split() {
        let w: Vec<_> = words("karte-pro-mcp, gitter_treiber.py: Kartenansicht").map(|x| x.1).collect();
        assert_eq!(w, ["karte", "pro", "mcp", "gitter", "treiber", "py", "Kartenansicht"]);
    }

    #[test]
    fn forms_contain_stems() {
        let a = Analyzer::new();
        let f = a.forms("Installationen");
        assert_eq!(f[0], "installationen");
        assert!(f.len() >= 2);
        assert_eq!(a.forms("providers").last().unwrap(), "provid");
    }

    #[test]
    fn sentence_split_respects_abbrev() {
        let t = "Das ist z. B. gut. Danach kommt 2.1 nicht. Ende";
        let s: Vec<_> = sentences(t).into_iter().map(|(a, b)| &t[a..b]).collect();
        assert_eq!(s, ["Das ist z. B. gut.", "Danach kommt 2.1 nicht.", "Ende"]);
    }

    #[test]
    fn edit_distance_works() {
        assert_eq!(edit_distance("provder", "provider", 2), Some(1));
        assert_eq!(edit_distance("abc", "xyz", 1), None);
    }

    #[test]
    fn tokens_estimate() {
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("abcdefg"), 2);
    }
}
