//! Textwerkzeuge: Anzeigebreite, Kürzen, Umbrechen von gestylten Textstücken, Zeichnen.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

pub type Spans = Vec<(String, Style)>;

pub fn width(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

/// Kürzt auf `max` Spalten; hängt `ell` an, wenn gekürzt wurde.
pub fn truncate(s: &str, max: usize, ell: char) -> String {
    if width(s) <= max {
        return s.to_string();
    }
    if max == 0 {
        return String::new();
    }
    let mut out = String::new();
    let mut w = 0;
    for ch in s.chars() {
        let cw = UnicodeWidthChar::width(ch).unwrap_or(0);
        if w + cw + 1 > max {
            break;
        }
        out.push(ch);
        w += cw;
    }
    out.push(ell);
    out
}

pub fn pad_right(s: &str, w: usize) -> String {
    let cur = width(s);
    if cur >= w {
        s.to_string()
    } else {
        format!("{s}{}", " ".repeat(w - cur))
    }
}

pub fn pad_left(s: &str, w: usize) -> String {
    let cur = width(s);
    if cur >= w {
        s.to_string()
    } else {
        format!("{}{s}", " ".repeat(w - cur))
    }
}

/// Schreibt `s` an (x, y), höchstens `max_w` Spalten, nicht über den Puffer hinaus.
pub fn put(buf: &mut Buffer, x: u16, y: u16, s: &str, max_w: usize, style: Style) -> u16 {
    let a = buf.area;
    if y < a.y || y >= a.y + a.height || x >= a.x + a.width || max_w == 0 {
        return x;
    }
    let avail = (a.x + a.width - x) as usize;
    let (nx, _) = buf.set_stringn(x, y, s, max_w.min(avail), style);
    nx
}

/// Schreibt gestylte Stücke hintereinander; gibt die neue x-Position zurück.
pub fn put_spans(buf: &mut Buffer, mut x: u16, y: u16, spans: &[(String, Style)], max_w: usize) -> u16 {
    let end = x as usize + max_w;
    for (t, st) in spans {
        if x as usize >= end {
            break;
        }
        x = put(buf, x, y, t, end - x as usize, *st);
    }
    x
}

pub fn fill(buf: &mut Buffer, r: Rect, style: Style) {
    let a = buf.area;
    let r = r.intersection(a);
    if r.width > 0 && r.height > 0 {
        buf.set_style(r, style);
    }
}

/// Zeile aus Stücken: Gesamtbreite in Spalten.
pub fn spans_width(spans: &[(String, Style)]) -> usize {
    spans.iter().map(|(t, _)| width(t)).sum()
}

fn push_span(line: &mut Spans, text: &str, style: Style) {
    if text.is_empty() {
        return;
    }
    if let Some((last, st)) = line.last_mut() {
        if *st == style {
            last.push_str(text);
            return;
        }
    }
    line.push((text.to_string(), style));
}

/// Umbruch nach Anzeigebreite. `first_indent` vor der ersten, `hang_indent` vor jeder weiteren
/// Zeile (Leerzeichen); Wörter, die länger sind als eine Zeile, werden hart getrennt.
/// Zeilenumbrüche (`\n`) im Text werden erhalten.
pub fn wrap(spans: &[(String, Style)], w: usize, first_indent: usize, hang_indent: usize) -> Vec<Spans> {
    let w = w.max(1);
    let mut lines: Vec<Spans> = Vec::new();
    let mut cur: Spans = Vec::new();
    let mut cur_w = 0usize;
    let mut indent = first_indent.min(w.saturating_sub(1));
    if indent > 0 {
        cur.push((" ".repeat(indent), Style::default()));
        cur_w = indent;
    }
    let mut line_has_word = false;

    // Wörter als Folge (Text, Stil) ohne Leerraum zerlegen
    let mut pending_space: Option<Style> = None;
    let mut word: Vec<(String, Style)> = Vec::new();
    let mut word_w = 0usize;

    macro_rules! newline {
        () => {{
            lines.push(std::mem::take(&mut cur));
            indent = hang_indent.min(w.saturating_sub(1));
            cur_w = indent;
            if indent > 0 {
                cur.push((" ".repeat(indent), Style::default()));
            }
            line_has_word = false;
        }};
    }

    let flush_word = |word: &mut Vec<(String, Style)>,
                          word_w: &mut usize,
                          pending_space: &mut Option<Style>,
                          lines: &mut Vec<Spans>,
                          cur: &mut Spans,
                          cur_w: &mut usize,
                          indent: &mut usize,
                          line_has_word: &mut bool| {
        if word.is_empty() {
            return;
        }
        let sp = if *line_has_word { 1 } else { 0 };
        if *cur_w + sp + *word_w > w && *line_has_word {
            lines.push(std::mem::take(cur));
            *indent = hang_indent.min(w.saturating_sub(1));
            *cur_w = *indent;
            if *indent > 0 {
                cur.push((" ".repeat(*indent), Style::default()));
            }
            *line_has_word = false;
        } else if sp == 1 {
            push_span(cur, " ", pending_space.unwrap_or_default());
            *cur_w += 1;
        }
        // Wort ggf. hart trennen
        for (t, st) in word.drain(..) {
            let mut piece = String::new();
            let mut pw = 0;
            for ch in t.chars() {
                let cw = UnicodeWidthChar::width(ch).unwrap_or(0);
                if *cur_w + pw + cw > w && (pw > 0 || *cur_w > *indent) {
                    push_span(cur, &piece, st);
                    lines.push(std::mem::take(cur));
                    *indent = hang_indent.min(w.saturating_sub(1));
                    *cur_w = *indent;
                    if *indent > 0 {
                        cur.push((" ".repeat(*indent), Style::default()));
                    }
                    piece.clear();
                    pw = 0;
                }
                piece.push(ch);
                pw += cw;
            }
            push_span(cur, &piece, st);
            *cur_w += pw;
        }
        *word_w = 0;
        *line_has_word = true;
        *pending_space = None;
    };

    for (text, style) in spans {
        let mut piece = String::new();
        for ch in text.chars() {
            if ch == '\n' {
                if !piece.is_empty() {
                    word_w += width(&piece);
                    word.push((std::mem::take(&mut piece), *style));
                }
                flush_word(&mut word, &mut word_w, &mut pending_space, &mut lines, &mut cur, &mut cur_w, &mut indent, &mut line_has_word);
                newline!();
                continue;
            }
            if ch.is_whitespace() {
                if !piece.is_empty() {
                    word_w += width(&piece);
                    word.push((std::mem::take(&mut piece), *style));
                }
                flush_word(&mut word, &mut word_w, &mut pending_space, &mut lines, &mut cur, &mut cur_w, &mut indent, &mut line_has_word);
                pending_space = Some(*style);
            } else {
                piece.push(ch);
            }
        }
        if !piece.is_empty() {
            word_w += width(&piece);
            word.push((piece, *style));
        }
    }
    flush_word(&mut word, &mut word_w, &mut pending_space, &mut lines, &mut cur, &mut cur_w, &mut indent, &mut line_has_word);
    if !cur.is_empty() || lines.is_empty() {
        lines.push(cur);
    }
    lines
}

/// Teilt `text` an den Byte-Bereichen `marks` in Stücke mit `base` und `hit`.
pub fn highlight(text: &str, marks: &[(usize, usize)], base: Style, hit: Style) -> Spans {
    let mut out: Spans = Vec::new();
    let mut pos = 0;
    let mut sorted: Vec<(usize, usize)> = marks.to_vec();
    sorted.sort();
    for (s, e) in sorted {
        if s < pos || e > text.len() || s >= e || !text.is_char_boundary(s) || !text.is_char_boundary(e) {
            continue;
        }
        if s > pos {
            out.push((text[pos..s].to_string(), base));
        }
        out.push((text[s..e].to_string(), hit));
        pos = e;
    }
    if pos < text.len() {
        out.push((text[pos..].to_string(), base));
    }
    out
}

/// Faltet ein Zeichen für den Treffervergleich (Klein, ä→a, ö→o, ü→u, ß→s), immer eins zu eins.
fn fold_char(c: char) -> char {
    let l = c.to_lowercase().next().unwrap_or(c);
    match l {
        'ä' => 'a',
        'ö' => 'o',
        'ü' => 'u',
        'ß' => 's',
        o => o,
    }
}

/// Suchwörter aus der Eingabe: gefaltet, ohne Anführungszeichen und Minus, mindestens zwei Zeichen.
pub fn query_terms(q: &str) -> Vec<Vec<char>> {
    q.split_whitespace()
        .map(|w| w.trim_matches(|c: char| c == '"' || c == '-' || c == '*').chars().map(fold_char).collect::<Vec<_>>())
        .filter(|w| w.len() >= 2)
        .collect()
}

/// Byte-Bereiche in `text`, an denen ein Suchwort (gefaltet, als Teilstring) vorkommt.
pub fn term_marks(text: &str, terms: &[Vec<char>]) -> Vec<(usize, usize)> {
    if terms.is_empty() {
        return Vec::new();
    }
    let chars: Vec<(usize, char)> = text.char_indices().map(|(i, c)| (i, fold_char(c))).collect();
    let mut marks: Vec<(usize, usize)> = Vec::new();
    for t in terms {
        let n = t.len();
        if n == 0 || chars.len() < n {
            continue;
        }
        let mut i = 0;
        while i + n <= chars.len() {
            if (0..n).all(|k| chars[i + k].1 == t[k]) {
                let s = chars[i].0;
                let e = chars.get(i + n).map(|x| x.0).unwrap_or(text.len());
                marks.push((s, e));
                i += n;
            } else {
                i += 1;
            }
        }
    }
    marks.sort();
    // überlappende Bereiche verschmelzen
    let mut out: Vec<(usize, usize)> = Vec::new();
    for (s, e) in marks {
        match out.last_mut() {
            Some(l) if s <= l.1 => l.1 = l.1.max(e),
            _ => out.push((s, e)),
        }
    }
    out
}

/// Ein Zeichen als ASCII (Vertrag §16.7): Umlaute und Akzente verlieren Tupfen, bekannte Zeichen
/// bekommen ein Gegenstück, alles andere wird `?`. Immer ein Zeichen für ein Zeichen, damit die
/// Zellen eines Bildes ihre Breite behalten.
pub fn ascii_char(c: char) -> char {
    if c.is_ascii() {
        return c;
    }
    match c {
        'ä' | 'à' | 'á' | 'â' | 'ã' | 'å' => 'a',
        'Ä' | 'À' | 'Á' | 'Â' | 'Ã' | 'Å' => 'A',
        'ö' | 'ò' | 'ó' | 'ô' | 'õ' => 'o',
        'Ö' | 'Ò' | 'Ó' | 'Ô' | 'Õ' => 'O',
        'ü' | 'ù' | 'ú' | 'û' => 'u',
        'Ü' | 'Ù' | 'Ú' | 'Û' => 'U',
        'é' | 'è' | 'ê' | 'ë' => 'e',
        'É' | 'È' | 'Ê' | 'Ë' => 'E',
        'í' | 'ì' | 'î' | 'ï' => 'i',
        'ç' => 'c',
        'ñ' => 'n',
        'ß' => 's',
        '×' => 'x',
        '…' => '~',
        '·' | '•' | '∙' | '⋅' => '.',
        '–' | '—' | '‑' | '−' | '─' | '━' | '‒' => '-',
        '│' | '┃' | '▏' | '▕' | '▎' | '▍' | '▌' | '║' => '|',
        '┌' | '┐' | '└' | '┘' | '├' | '┤' | '┬' | '┴' | '┼' | '╭' | '╮' | '╰' | '╯' => '+',
        '→' | '▶' | '►' | '›' => '>',
        '←' | '◀' | '◄' | '‹' => '<',
        '↑' | '▲' => '^',
        '↓' | '▼' => 'v',
        'Σ' => 'S',
        '„' | '“' | '”' | '«' | '»' => '"',
        '’' | '‘' | '‚' | '′' => '\'',
        '€' => 'E',
        '█' | '▉' | '▊' | '▇' | '▆' | '■' => '#',
        '▅' | '▄' | '▓' => '=',
        '▃' | '▂' | '▁' | '▒' => '-',
        '░' => ':',
        '●' | '◉' | '⬤' => '*',
        '◕' | '◑' | '◐' | '◔' => '+',
        '○' | '◯' | '◌' => 'o',
        '✓' | '✔' => 'v',
        '\u{a0}' | '\u{2009}' | '\u{200a}' | '\u{202f}' | '\u{2007}' | '\u{2002}' | '\u{2003}' => ' ',
        '\u{200b}' | '\u{200c}' | '\u{200d}' | '\u{feff}' | '\u{fe0f}' => ' ',
        _ => '?',
    }
}

/// Ganze Zeichenkette in ASCII, Zeichen für Zeichen.
pub fn asciify(s: &str) -> String {
    s.chars().map(ascii_char).collect()
}

/// Setzt jede Zelle eines fertigen Bildes auf reines ASCII. Das erfasst auch feste Texte und
/// Inhalte der Notizen, nicht nur die Glyphen der Oberfläche.
pub fn asciify_buffer(buf: &mut Buffer, area: Rect) {
    for y in area.y..area.y.saturating_add(area.height) {
        for x in area.x..area.x.saturating_add(area.width) {
            if let Some(c) = buf.cell_mut((x, y)) {
                if !c.symbol().is_ascii() {
                    let s = asciify(c.symbol());
                    // Breite Zeichen belegten zwei Zellen; die Folgezelle ist leer (" ")
                    let s: String = if s.is_empty() { " ".into() } else { s.chars().take(1).collect() };
                    c.set_symbol(&s);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(lines: &[Spans]) -> Vec<String> {
        lines
            .iter()
            .map(|l| l.iter().map(|(t, _)| t.as_str()).collect::<String>())
            .collect()
    }

    #[test]
    fn truncate_adds_ellipsis() {
        assert_eq!(truncate("Hallo Welt", 20, '…'), "Hallo Welt");
        assert_eq!(truncate("Hallo Welt", 6, '…'), "Hallo…");
        assert_eq!(width(&truncate("日本語のテキスト", 7, '…')), 7);
        assert_eq!(truncate("x", 0, '…'), "");
    }

    #[test]
    fn wrap_basic_and_hang() {
        let s = vec![("eins zwei drei vier fünf sechs".to_string(), Style::default())];
        let l = plain(&wrap(&s, 10, 0, 2));
        assert_eq!(l, vec!["eins zwei", "  drei", "  vier", "  fünf", "  sechs"]);
        for line in &l {
            assert!(width(line) <= 10);
        }
    }

    #[test]
    fn wrap_hard_splits_long_words() {
        let s = vec![("Donaudampfschifffahrtsgesellschaft".to_string(), Style::default())];
        let l = plain(&wrap(&s, 10, 0, 0));
        assert!(l.len() >= 4);
        for line in &l {
            assert!(width(line) <= 10, "{line:?}");
        }
        assert_eq!(l.concat(), "Donaudampfschifffahrtsgesellschaft");
    }

    #[test]
    fn wrap_keeps_newlines_and_wide_chars() {
        let s = vec![("a\nb 日本語日本語".to_string(), Style::default())];
        let l = plain(&wrap(&s, 6, 0, 0));
        assert_eq!(l[0], "a");
        for line in &l {
            assert!(width(line) <= 6, "{line:?}");
        }
    }

    #[test]
    fn highlight_splits() {
        let sp = highlight("ein Test hier", &[(4, 8)], Style::default(), Style::default().bold_hit());
        assert_eq!(sp.len(), 3);
        assert_eq!(sp[1].0, "Test");
    }

    trait BoldHit {
        fn bold_hit(self) -> Self;
    }
    impl BoldHit for Style {
        fn bold_hit(self) -> Self {
            self.add_modifier(ratatui::style::Modifier::BOLD)
        }
    }
}
