//! Ausgabeformate: Agent (Standard), JSON, Mensch. Alles wird in einen Puffer geschrieben
//! und erst am Ende ausgegeben (kein halber Text bei Fehlern).
//!
//! Budget (Vertrag §15.3): `--budget` ist eine harte Obergrenze für stdout, gemessen wie die
//! Preise (`~t` = Zeichen × 2/7 aufgerundet, +1 je Zeile), auch mit `--json` und `--links`.
//! Kopfzeile und die kleinste sinnvolle Einheit (erster Treffer, erster Textteil) erscheinen
//! immer; reicht das Budget dafür nicht oder liegt es unter dem Mindestwert, hebt `wb` es an und
//! nennt das angehobene Budget in der Kopfzeile (`budget=N(angehoben)`, JSON `budget_raised`).
//!
//! `more:` (§15.4): unverändert ausführbarer Befehl, der vorankommt und alle ergebnisbestimmenden
//! Schalter mitführt, in Git-Bash und PowerShell gleich zitiert. Erläuterungen stehen in `note:`.

use std::borrow::Cow;

use serde::Serialize;
use wb_core::text::{estimate_tokens, sentences, truncate_chars};
use wb_core::*;

use crate::{CheckArgs, ContextArgs, Mode, ReadArgs, SearchArgs};

type Res = std::result::Result<i32, Error>;

/// Mindestbudgets (in `wb --help` dokumentiert).
pub(crate) const MIN_SEARCH: u32 = 80;
pub(crate) const MIN_READ: u32 = 60;
pub(crate) const MIN_CONTEXT: u32 = 200;

/// Aufrufumgebung aller Ausgaben.
pub(crate) struct Ctx {
    pub mode: Mode,
    pub color: bool,
    /// Globale Schalter, die beim Aufruf auf der Kommandozeile standen (Schalter, Wert), für
    /// `more:` und `next:`; Umgebungsvariablen gelten ohnehin weiter.
    pub global: Vec<(String, Option<String>)>,
}

// ---------------- Puffer, Kosten, Steuerzeichen ----------------

/// Preis einer Ausgabezeile in Token (Zeile plus Zeilenende).
pub(crate) fn cost(s: &str) -> u32 {
    estimate_tokens(s).saturating_add(1)
}

/// Preis eines mehrzeiligen Textes.
fn text_cost(s: &str) -> u32 {
    s.lines().fold(0u32, |a, l| a.saturating_add(cost(l)))
}

fn is_ctrl(c: char) -> bool {
    (c < ' ' && c != '\t') || ('\u{7f}'..='\u{9f}').contains(&c) || matches!(c, '\u{2028}' | '\u{2029}') || wb_core::is_bidi_control(c)
}

/// Steuerzeichen aus Notizinhalten sichtbar ersetzen (§15.7, §19.5): C0 außer Tabulator als
/// Steuerzeichenbild (␛ für ESC), DEL als ␡, C1 und Richtungssteuerung (U+202E u. a.) als �,
/// Zeilen- und Absatztrenner U+2028/9 als ␤. Die Zeichenzahl bleibt gleich.
pub(crate) fn clean(s: &str) -> Cow<'_, str> {
    if !s.chars().any(is_ctrl) {
        return Cow::Borrowed(s);
    }
    Cow::Owned(
        s.chars()
            .map(|c| match c {
                '\u{0}'..='\u{1f}' if c != '\t' => char::from_u32(0x2400 + c as u32).unwrap_or('\u{fffd}'),
                '\u{7f}' => '\u{2421}',
                '\u{80}'..='\u{9f}' => '\u{fffd}',
                '\u{2028}' | '\u{2029}' => '\u{2424}',
                c if wb_core::is_bidi_control(c) => '\u{fffd}',
                _ => c,
            })
            .collect(),
    )
}

/// Ausgabepuffer mit Tokenzähler. Jede Zeile wird von Steuerzeichen befreit.
struct Buf<'a> {
    out: &'a mut Vec<u8>,
    used: u32,
    color: bool,
}

impl<'a> Buf<'a> {
    fn new(out: &'a mut Vec<u8>, color: bool) -> Self {
        Buf { out, used: 0, color }
    }
    fn line(&mut self, s: &str) {
        let s = clean(s);
        self.used = self.used.saturating_add(cost(&s));
        self.out.extend_from_slice(s.as_bytes());
        self.out.push(b'\n');
    }
    fn styled(&mut self, s: &str, code: &str) {
        let s = clean(s);
        self.used = self.used.saturating_add(cost(&s));
        if self.color {
            self.out.extend_from_slice(format!("\x1b[{code}m{s}\x1b[0m\n").as_bytes());
        } else {
            self.out.extend_from_slice(s.as_bytes());
            self.out.push(b'\n');
        }
    }
    fn bold(&mut self, s: &str) {
        self.styled(s, "1")
    }
    fn dim(&mut self, s: &str) {
        self.styled(s, "2")
    }
    /// Ausschnitt mit hervorgehobenen Trefferwörtern (nur mit Farbe).
    fn highlighted(&mut self, prefix: &str, s: &str, hl: &[(usize, usize)]) {
        if !self.color {
            return self.line(&format!("{prefix}{s}"));
        }
        let plain = clean(&format!("{prefix}{s}")).into_owned();
        self.used = self.used.saturating_add(cost(&plain));
        let mut o = String::from(prefix);
        let mut pos = 0;
        for &(a, b) in hl {
            if a < pos || b > s.len() || !s.is_char_boundary(a) || !s.is_char_boundary(b) {
                continue;
            }
            o.push_str(&clean(&s[pos..a]));
            o.push_str("\x1b[1;33m");
            o.push_str(&clean(&s[a..b]));
            o.push_str("\x1b[0m");
            pos = b;
        }
        o.push_str(&clean(&s[pos..]));
        self.out.extend_from_slice(o.as_bytes());
        self.out.push(b'\n');
    }
}

/// `p` Prozent von `x`, ohne Überlauf.
fn pct(x: u32, p: u32) -> u32 {
    (x as u64 * p as u64 / 100) as u32
}

/// Höchstzahl Zeichen, deren Zeile `room` Token kostet.
fn chars_for(room: u32) -> usize {
    (room.saturating_sub(1) as usize).saturating_mul(7) / 2
}

/// `s` auf höchstens `max` Zeichen kürzen, mit `…` am Ende, wenn gekürzt.
fn fit(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let keep = max.saturating_sub(1);
    let mut out: String = s.chars().take(keep).collect();
    out.push('…');
    out
}

/// Budget mit Mindestwert; `true`, wenn angehoben.
fn effective(requested: u32, min: u32) -> (u32, bool) {
    if requested < min { (min, true) } else { (requested, false) }
}

fn budget_note(budget: u32, raised: bool) -> String {
    if raised { format!(" budget={budget}(angehoben)") } else { String::new() }
}

// ---------------- Zitieren und Befehle ----------------

fn has_any(s: &str, set: &[char]) -> bool {
    s.chars().any(|c| set.contains(&c))
}

/// Ein Argument so zitieren, dass es in Git-Bash und PowerShell gleich ankommt (§15.4).
/// Zweiter Wert `false`: nur für Bash richtig (enthält `"`, oder einfache Anführungszeichen und
/// zugleich `$`, Backtick, `\` oder `!`). Windows-Dateinamen enthalten nie `"`.
pub(crate) fn shell_word(s: &str) -> (String, bool) {
    const SINGLE: &[char] = &['\'', '\u{2018}', '\u{2019}', '\u{201a}', '\u{201b}'];
    const DOUBLE_BAD: &[char] = &['"', '$', '`', '\\', '!', '\u{201c}', '\u{201d}', '\u{201e}'];
    let bare = !s.is_empty()
        && !s.starts_with(['-', '#', '~', '@'])
        && s.chars().all(|c| c.is_alphanumeric() || "/._:#-".contains(c));
    if bare {
        return (s.to_string(), true);
    }
    if !has_any(s, SINGLE) {
        // PowerShell 5.1 reicht `"` in Argumenten an Programme nicht unversehrt weiter.
        return (format!("'{s}'"), !s.contains('"'));
    }
    if !has_any(s, DOUBLE_BAD) {
        return (format!("\"{s}\""), true);
    }
    (format!("'{}'", s.replace('\'', "'\\''")), false)
}

/// Zitat für Kopfzeilen (`q="…"`); kein Befehl.
pub(crate) fn quote_arg(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\\\""))
}

/// Ausführbarer `wb`-Befehl für `more:` und `next:`.
pub(crate) struct CmdLine {
    s: String,
    portable: bool,
}

impl CmdLine {
    pub(crate) fn new(ctx: &Ctx, sub: &str) -> CmdLine {
        let mut c = CmdLine { s: "wb".into(), portable: true };
        for (flag, val) in &ctx.global {
            c.flag(flag);
            if let Some(v) = val {
                c.arg(v);
            }
        }
        match ctx.mode {
            Mode::Json => c.flag("--json"),
            Mode::Human => c.flag("--human"),
            Mode::Agent => &mut c,
        };
        c.flag(sub);
        c
    }
    pub(crate) fn flag(&mut self, f: &str) -> &mut Self {
        self.s.push(' ');
        self.s.push_str(f);
        self
    }
    pub(crate) fn arg(&mut self, a: &str) -> &mut Self {
        let (q, p) = shell_word(a);
        self.portable &= p;
        self.s.push(' ');
        self.s.push_str(&q);
        self
    }
    pub(crate) fn opt(&mut self, f: &str, v: &str) -> &mut Self {
        self.flag(f);
        self.arg(v)
    }
    /// Positionsargument, das mit `-` beginnen darf.
    pub(crate) fn positional(&mut self, a: &str) -> &mut Self {
        if a.starts_with('-') {
            self.flag("--");
        }
        self.arg(a)
    }
    pub(crate) fn as_str(&self) -> &str {
        &self.s
    }
    /// Zeilen `<prefix>: <befehl>` und, falls nötig, der Hinweis auf die Bash-Zitierung.
    pub(crate) fn lines(&self, prefix: &str) -> Vec<String> {
        let mut v = vec![format!("{prefix}: {}", self.s)];
        if !self.portable {
            v.push("note: Argument mit Anführungszeichen: Befehl ist für Bash zitiert".into());
        }
        v
    }
    fn cost(&self, prefix: &str) -> u32 {
        self.lines(prefix).iter().map(|l| cost(l)).sum()
    }
}

fn parse_kind(s: &Option<String>) -> std::result::Result<Option<SectionKind>, Error> {
    match s {
        None => Ok(None),
        Some(k) => SectionKind::parse(k).map(Some).ok_or_else(|| Error::Usage {
            message: format!("unbekannte Abschnittsart: {k}"),
            hint: Some("state | open | decisions | lessons | evidence | sources | paths | topics | other".into()),
        }),
    }
}

// ---------------- JSON ----------------

/// JSON nur aus ASCII: Nicht-ASCII-Zeichen stehen als `\uXXXX`. So bleibt `--json` auch dort
/// heil, wo die Shell eingefangene Ausgabe mit einer OEM-Codepage liest (PowerShell 5.1).
/// Außerhalb von Zeichenketten kommt in JSON kein Nicht-ASCII vor, das Ersetzen ist daher sicher.
fn json_string<T: Serialize>(v: &T) -> String {
    let mut s = ascii_json(&serde_json::to_string_pretty(v).expect("json"));
    s.push('\n');
    s
}

/// Zeilenform für die budgetierten Befehle (search, read, context; Vertrag §17.2): ohne
/// Einrückung, jedes Element einer Liste der obersten Ebene auf einer eigenen Zeile, alles andere
/// kompakt. Gleiche Felder wie bisher, aber ein Treffer kostet nur noch seinen Inhalt.
fn json_lines<T: Serialize>(v: &T) -> String {
    let mut buf = Vec::new();
    let mut ser = serde_json::Serializer::with_formatter(&mut buf, LineFormatter::default());
    v.serialize(&mut ser).expect("json");
    let mut s = ascii_json(std::str::from_utf8(&buf).expect("utf8"));
    s.push('\n');
    s
}

#[derive(Default)]
struct LineFormatter {
    depth: usize,
    nonempty: Vec<bool>,
}

impl serde_json::ser::Formatter for LineFormatter {
    fn begin_array<W: ?Sized + std::io::Write>(&mut self, w: &mut W) -> std::io::Result<()> {
        self.depth += 1;
        self.nonempty.push(false);
        w.write_all(b"[")
    }
    fn end_array<W: ?Sized + std::io::Write>(&mut self, w: &mut W) -> std::io::Result<()> {
        let nonempty = self.nonempty.pop().unwrap_or(false);
        if self.depth == 2 && nonempty {
            w.write_all(b"\n")?;
        }
        self.depth -= 1;
        w.write_all(b"]")
    }
    fn begin_array_value<W: ?Sized + std::io::Write>(&mut self, w: &mut W, first: bool) -> std::io::Result<()> {
        if let Some(x) = self.nonempty.last_mut() {
            *x = true;
        }
        if !first {
            w.write_all(b",")?;
        }
        if self.depth == 2 {
            w.write_all(b"\n")?;
        }
        Ok(())
    }
    fn begin_object<W: ?Sized + std::io::Write>(&mut self, w: &mut W) -> std::io::Result<()> {
        self.depth += 1;
        self.nonempty.push(false);
        w.write_all(b"{")
    }
    fn end_object<W: ?Sized + std::io::Write>(&mut self, w: &mut W) -> std::io::Result<()> {
        self.nonempty.pop();
        self.depth -= 1;
        w.write_all(b"}")
    }
}

/// Ersetzt Nicht-ASCII in fertigem JSON durch `\uXXXX`.
fn ascii_json(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 1);
    let mut buf = [0u16; 2];
    for c in s.chars() {
        if c.is_ascii() {
            out.push(c);
        } else {
            for u in c.encode_utf16(&mut buf) {
                out.push_str(&format!("\\u{:04x}", u));
            }
        }
    }
    out
}

pub(crate) fn json<T: Serialize>(out: &mut Vec<u8>, v: &T) {
    out.extend_from_slice(json_string(v).as_bytes());
}

/// Größtes `n` in `lo..=hi`, für das `fits(n)` gilt (monoton fallend); sonst `lo`.
fn largest_fitting(lo: usize, hi: usize, fits: impl Fn(usize) -> bool) -> usize {
    if hi <= lo || fits(hi) {
        return hi.max(lo);
    }
    let (mut a, mut b) = (lo, hi); // fits(a) angenommen oder a = lo, !fits(b)
    while b - a > 1 {
        let m = a + (b - a) / 2;
        if fits(m) { a = m } else { b = m }
    }
    a
}

// ---------------- sync ----------------

pub(crate) fn sync(out: &mut Vec<u8>, mode: Mode, r: &SyncReport) {
    match mode {
        Mode::Json => json(out, r),
        _ => {
            let mut b = Buf::new(out, false);
            b.line(&format!(
                "sync scanned={} added={} changed={} removed={} touched={} full={} errors={}",
                r.scanned,
                r.added,
                r.changed,
                r.removed,
                r.touched,
                r.full_rebuild,
                r.errors.len()
            ));
        }
    }
}

// ---------------- search ----------------

pub(crate) fn search(out: &mut Vec<u8>, ctx: &Ctx, v: &Vault, a: &SearchArgs) -> Res {
    if a.limit == 0 {
        return Err(Error::Usage { message: "--limit muss mindestens 1 sein".into(), hint: Some("--limit 8 (Standard)".into()) });
    }
    if a.per_note == 0 {
        return Err(Error::Usage { message: "--per-note muss mindestens 1 sein".into(), hint: Some("--per-note 2 (Standard)".into()) });
    }
    let text = a.terms.join(" ");
    let note_type = match &a.note_type {
        None => None,
        Some(t) => Some(NoteType::parse(t).ok_or_else(|| Error::Usage {
            message: format!("unbekannte Notizart: {t}"),
            hint: Some("overview | topic | source | other".into()),
        })?),
    };
    let status = match &a.status {
        None => None,
        Some(s) => Some(NoteStatus::parse(s).ok_or_else(|| Error::Usage {
            message: format!("unbekannter status: {s}"),
            hint: Some("active | paused | done | superseded".into()),
        })?),
    };
    let q = SearchQuery {
        text: text.clone(),
        project: a.project.clone(),
        note_type,
        kind: parse_kind(&a.kind)?,
        prefix: a.prefix.clone(),
        status,
        offset: a.offset,
        limit: a.limit,
        per_note: a.per_note,
    };
    let r = v.search(&q)?;
    let code = if r.total == 0 { exit::NOT_FOUND } else { exit::OK };
    let total = r.total as u64;
    let more_cmd = |next: u64| -> CmdLine {
        let mut c = CmdLine::new(ctx, "search");
        if let Some(p) = &a.project {
            c.opt("-p", p);
        }
        if let Some(t) = &a.note_type {
            c.opt("-t", t);
        }
        if let Some(k) = &a.kind {
            c.opt("-k", k);
        }
        if let Some(p) = &a.prefix {
            c.opt("--prefix", p);
        }
        if let Some(s) = &a.status {
            c.opt("-s", s);
        }
        if a.per_note != crate::SEARCH_PER_NOTE {
            c.opt("--per-note", &a.per_note.to_string());
        }
        if a.limit != crate::SEARCH_LIMIT {
            c.opt("--limit", &a.limit.to_string());
        }
        if a.budget != crate::SEARCH_BUDGET {
            c.opt("--budget", &a.budget.to_string());
        }
        c.opt("--offset", &next.to_string());
        c.positional(&text);
        c
    };
    let next_of = |shown: usize| r.offset as u64 + shown as u64;

    if ctx.mode == Mode::Json {
        // Schlanke Form (Vertrag §17.2): kompakt, eine Zeile je Treffer, keine Felder, die ein
        // anderes wiederholen (`path` und `section_title` stehen in `address`, `rank` ordnet,
        // `highlights` und `score` dienen nur der Anzeige), leere Felder fallen weg, `terms` nur
        // mit Begriffen, zu denen der Text eine `note:` schriebe.
        #[derive(Clone, Copy, Serialize)]
        struct JTerm<'a> {
            raw: &'a str,
            #[serde(skip_serializing_if = "Option::is_none")]
            corrected: Option<&'a str>,
            #[serde(skip_serializing_if = "std::ops::Not::not")]
            stopword: bool,
            sections: u32,
        }
        #[derive(Serialize)]
        struct J<'a> {
            query: &'a str,
            #[serde(skip_serializing_if = "Vec::is_empty")]
            terms: Vec<JTerm<'a>>,
            total: u32,
            offset: usize,
            budget: u32,
            budget_raised: bool,
            shown: usize,
            more: Option<String>,
            hits: Vec<JHit<'a>>,
        }
        #[derive(Serialize)]
        struct JHit<'a> {
            rank: u32,
            address: &'a str,
            line_start: u32,
            line_end: u32,
            #[serde(skip_serializing_if = "is_other")]
            kind: SectionKind,
            note_type: NoteType,
            #[serde(skip_serializing_if = "Option::is_none")]
            status: Option<NoteStatus>,
            #[serde(skip_serializing_if = "Option::is_none")]
            project: Option<&'a str>,
            #[serde(skip_serializing_if = "Option::is_none")]
            date: Option<&'a str>,
            tokens: u32,
            snippet: Cow<'a, str>,
            #[serde(skip_serializing_if = "is_zero")]
            more_in_note: u32,
        }
        fn is_other(k: &SectionKind) -> bool {
            *k == SectionKind::Other
        }
        fn is_zero(n: &u32) -> bool {
            *n == 0
        }
        let terms: Vec<JTerm> = r
            .terms
            .iter()
            .filter(|t| t.corrected.is_some() || t.stopword || t.sections == 0)
            .map(|t| JTerm { raw: &t.raw, corrected: t.corrected.as_deref(), stopword: t.stopword, sections: t.sections })
            .collect();
        fn jhit(h: &SearchHit) -> JHit<'_> {
            JHit {
                rank: h.rank,
                address: &h.address,
                line_start: h.line_start,
                line_end: h.line_end,
                kind: h.kind,
                note_type: h.note_type,
                status: h.status,
                project: h.project.as_deref(),
                date: h.date.as_deref(),
                tokens: h.tokens,
                snippet: Cow::Borrowed(&h.snippet),
                more_in_note: h.more_in_note,
            }
        }
        // `first`: gekürzter Ausschnitt des ersten Treffers.
        let render = |n: usize, budget: u32, raised: bool, first: Option<&str>| -> String {
            let more = (next_of(n) < total).then(|| more_cmd(next_of(n)).as_str().to_string());
            let mut hits: Vec<JHit> = r.hits.iter().take(n).map(jhit).collect();
            if let (Some(f), Some(h)) = (first, hits.first_mut()) {
                h.snippet = Cow::Borrowed(f);
            }
            json_lines(&J {
                query: &r.query,
                terms: terms.clone(),
                total: r.total,
                offset: r.offset,
                budget,
                budget_raised: raised,
                shown: n,
                more,
                hits,
            })
        };
        let (budget, raised) = effective(a.budget, MIN_SEARCH);
        let min_n = r.hits.len().min(1);
        let n = largest_fitting(min_n, r.hits.len(), |n| text_cost(&render(n, budget, raised, None)) <= budget);
        let mut s = render(n, budget, raised, None);
        const MIN_SNIPPET: usize = 40;
        let mut first: Option<String> = None;
        if text_cost(&s) > budget && n == 1 {
            // Wie im Text: der erste Ausschnitt wird gekürzt, bevor das Budget steigt.
            let full = r.hits[0].snippet.chars().count();
            let (mut lo, mut hi) = (MIN_SNIPPET, full);
            while hi - lo > 1 {
                let m = lo + (hi - lo) / 2;
                if text_cost(&render(1, budget, raised, Some(&fit(&r.hits[0].snippet, m)))) <= budget { lo = m } else { hi = m }
            }
            let f = fit(&r.hits[0].snippet, lo);
            s = render(1, budget, raised, Some(&f));
            first = Some(f);
        }
        if text_cost(&s) > budget {
            // Selbst der gekürzte erste Treffer passt nicht: Budget anheben und das sagen.
            let need = text_cost(&render(n, 99_999, true, first.as_deref()));
            s = render(n, need, true, first.as_deref());
        }
        out.extend_from_slice(s.as_bytes());
        return Ok(code);
    }

    // Zeilen vorbereiten, dann nach Budget packen
    let mut blocks: Vec<(String, &SearchHit)> = Vec::new();
    for h in &r.hits {
        let mut head = format!(
            "{} {} L{}-{} {} {} ~{}t",
            h.rank,
            h.address,
            h.line_start,
            h.line_end,
            h.note_type,
            h.date.as_deref().unwrap_or("-"),
            h.tokens
        );
        if h.kind != SectionKind::Other {
            head.push_str(&format!(" [{}]", h.kind));
        }
        if h.status == Some(NoteStatus::Superseded) {
            head.push_str(" superseded");
        }
        if h.more_in_note > 0 {
            head.push_str(&format!(" +{}sec", h.more_in_note));
        }
        blocks.push((head, h));
    }
    let mut notes = Vec::new();
    for t in &r.terms {
        if let Some(cor) = &t.corrected {
            notes.push(format!("{}→{} (Tippfehler?)", t.raw, cor));
        }
        if t.stopword {
            notes.push(format!("{} ignoriert", t.raw));
        }
        if t.sections == 0 && !t.stopword {
            notes.push(format!("{} ohne Treffer", t.raw));
        }
    }
    let filtered = q.project.is_some() || q.note_type.is_some() || q.kind.is_some() || q.prefix.is_some() || q.status.is_some();
    if r.total == 0 && filtered {
        let all = v.search(&SearchQuery { text: text.clone(), offset: 0, limit: 1, per_note: a.per_note, ..Default::default() })?;
        if all.total > 0 {
            notes.push(format!("{} Treffer ohne Filter", all.total));
        }
    }
    let note_line = (!notes.is_empty()).then(|| format!("note: {}", notes.join("; ")));
    let header = |shown: usize, price: u32, budget: u32, raised: bool| -> String {
        let range = if shown == 0 { "0".to_string() } else { format!("{}-{}", r.offset as u64 + 1, next_of(shown)) };
        format!("q={} hits={}/{} ~{}t{}", quote_arg(&text), range, r.total, price, budget_note(budget, raised))
    };
    let (mut budget, mut raised) = effective(a.budget, MIN_SEARCH);
    let price_all = r.hits.iter().fold(0u32, |s, h| s.saturating_add(h.tokens));
    // Obere Schranken für Kopfzeile, Hinweis und `more:` (längste denkbare Zahlen).
    let fixed = cost(&clean(&header(r.hits.len(), price_all, 99_999, true)))
        + note_line.as_deref().map(|l| cost(&clean(l))).unwrap_or(0)
        + if next_of(0) < total { more_cmd(next_of(r.hits.len()).max(next_of(0))).cost("more") } else { 0 };
    const MIN_SNIPPET: usize = 40;
    let unit = blocks.first().map(|(h, hit)| cost(&clean(h)) + cost(&fit(&format!("  {}", hit.snippet), MIN_SNIPPET))).unwrap_or(0);
    if fixed + unit > budget {
        budget = fixed + unit;
        raised = true;
    }
    let mut used = fixed;
    let mut shown = 0usize;
    let mut first_snip: Option<String> = None;
    for (i, (h, hit)) in blocks.iter().enumerate() {
        let snip = clean(&format!("  {}", hit.snippet)).into_owned();
        let cst = cost(&clean(h)) + cost(&snip);
        if used + cst <= budget {
            used += cst;
            shown += 1;
        } else if i == 0 {
            let room = budget.saturating_sub(used).saturating_sub(cost(&clean(h)));
            first_snip = Some(fit(&snip, chars_for(room).max(MIN_SNIPPET)));
            shown = 1;
            break;
        } else {
            break;
        }
    }
    let price: u32 = blocks.iter().take(shown).fold(0u32, |s, b| s.saturating_add(b.1.tokens));
    let mut b = Buf::new(out, ctx.color);
    b.bold(&header(shown, price, budget, raised));
    if let Some(l) = &note_line {
        b.line(l);
    }
    for (i, (h, hit)) in blocks.iter().take(shown).enumerate() {
        b.bold(h);
        match (&first_snip, i) {
            (Some(s), 0) => b.line(s),
            _ => b.highlighted("  ", &hit.snippet, &hit.highlights),
        }
    }
    if next_of(shown) < total {
        for l in more_cmd(next_of(shown)).lines("more") {
            b.dim(&l);
        }
    }
    Ok(code)
}

// ---------------- read ----------------

/// Ausgewählter Text: Zeile `from` ab Spalte `col` (1-basiert, Zeichen) bis Zeile `to`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Range {
    from: u32,
    col: u32,
    to: u32,
}

impl Range {
    fn spec(&self) -> String {
        let a = if self.col > 1 { format!("{}:{}", self.from, self.col) } else { self.from.to_string() };
        if self.to == self.from { a } else { format!("{a}-{}", self.to) }
    }
}

fn ranges_spec(r: &[Range]) -> String {
    r.iter().map(Range::spec).collect::<Vec<_>>().join(",")
}

/// `--lines`: `A`, `A-B`, `A-` oder `A:S-B` (ab Zeichen S der Zeile A), mehrere mit Komma.
fn parse_lines(s: &str, total: u32) -> std::result::Result<Vec<Range>, Error> {
    let bad = || Error::Usage {
        message: format!("ungültiger Zeilenbereich: {s}"),
        hint: Some("--lines 10-20, --lines 10, --lines 10-, --lines 8:1201-10 (ab Zeichen 1201), mehrere mit Komma".into()),
    };
    let num = |x: &str| x.trim().parse::<u32>().map_err(|_| bad());
    let mut out = Vec::new();
    for part in s.split(',') {
        let (a, b) = match part.split_once('-') {
            Some((a, b)) => (a, Some(b)),
            None => (part, None),
        };
        let (from, col) = match a.split_once(':') {
            Some((l, c)) => (num(l)?, num(c)?),
            None => (num(a)?, 1),
        };
        let to = match b {
            None => from,
            Some(b) if b.trim().is_empty() => total.max(from),
            Some(b) => num(b)?,
        };
        if from == 0 || col == 0 || to < from {
            return Err(bad());
        }
        if from > total {
            return Err(Error::NotFound {
                what: "Zeile",
                query: format!("{from} (Notiz hat {total} Zeilen)"),
                suggestions: vec![format!("--lines 1-{total}")],
            });
        }
        out.push(Range { from, col, to: to.min(total) });
    }
    Ok(out)
}

/// Erstes Zeichen ab Spalte `col` (1-basiert).
fn from_col(s: &str, col: u32) -> &str {
    match s.char_indices().nth(col.saturating_sub(1) as usize) {
        Some((i, _)) => &s[i..],
        None => "",
    }
}

/// Wie viele Zeichen von `s` in `room` Token passen (mit `…`): bevorzugt Satzende, sonst
/// Leerzeichen in der zweiten Hälfte, sonst Zeichengrenze. Mindestens 1, wenn `s` nicht leer ist.
fn cut_chars(s: &str, room: u32) -> usize {
    let n = s.chars().count();
    let max = chars_for(room).saturating_sub(1).max(1).min(n);
    if max >= n {
        return n;
    }
    let byte_max = s.char_indices().nth(max).map(|(i, _)| i).unwrap_or(s.len());
    let mut best = 0usize;
    for (_, e) in sentences(s) {
        if e <= byte_max {
            best = e;
        }
    }
    let to_chars = |b: usize| s[..b].chars().count();
    if best > 0 && to_chars(best) * 2 >= max {
        return to_chars(best);
    }
    if let Some(sp) = s[..byte_max].rfind(' ')
        && to_chars(sp + 1) * 2 >= max
    {
        return to_chars(sp + 1);
    }
    max
}

/// Text der Bereiche ausgeben, solange er in `cap` passt. Liefert die nicht ausgegebenen
/// Bereiche (leer, wenn alles kam). `force`: den ersten Teil auch über `cap` hinaus zeigen.
fn emit_ranges(b: &mut Buf, lines: &[&str], ranges: &[Range], cap: u32, mut force: bool) -> Vec<Range> {
    for (ri, r) in ranges.iter().enumerate() {
        for n in r.from..=r.to {
            let full = lines.get(n as usize - 1).copied().unwrap_or("");
            let col = if n == r.from { r.col } else { 1 };
            let text = clean(from_col(full, col)).into_owned();
            if b.used.saturating_add(cost(&text)) <= cap {
                b.line(&text);
                force = false;
                continue;
            }
            let room = cap.saturating_sub(b.used);
            let room = if force { room.max(12) } else { room };
            if room >= 6 && !text.is_empty() {
                let k = cut_chars(&text, room);
                if k < text.chars().count() {
                    let part: String = text.chars().take(k).collect();
                    b.line(&format!("{part}…"));
                    let mut rest = vec![Range { from: n, col: col + k as u32, to: r.to }];
                    rest.extend_from_slice(&ranges[ri + 1..]);
                    return rest;
                }
            }
            let mut rest = vec![Range { from: n, col, to: r.to }];
            rest.extend_from_slice(&ranges[ri + 1..]);
            return rest;
        }
    }
    Vec::new()
}

/// Kopf von `read --json`: was die Textkopfzeile zeigt, dazu Titel und Auffälliges; ohne
/// `type_raw`, `summary`, `mtime`, `bytes` (Vertrag §17.2).
#[derive(Serialize)]
struct ReadMeta<'a> {
    path: &'a NotePath,
    title: &'a str,
    note_type: NoteType,
    #[serde(skip_serializing_if = "Option::is_none")]
    project: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    status: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    state: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    checked_on: Option<&'a str>,
    #[serde(skip_serializing_if = "<[String]>::is_empty")]
    aliases: &'a [String],
    lines: u32,
    tokens: u32,
    links_out: u32,
    links_in: u32,
    #[serde(skip_serializing_if = "<[String]>::is_empty")]
    frontmatter_problems: &'a [String],
}

impl<'a> ReadMeta<'a> {
    fn of(m: &'a NoteMeta) -> Self {
        ReadMeta {
            path: &m.path,
            title: &m.title,
            note_type: m.note_type,
            project: m.project.as_deref(),
            status: m.status.as_deref(),
            state: m.state.as_deref(),
            checked_on: m.checked_on.as_deref(),
            aliases: &m.aliases,
            lines: m.lines,
            tokens: m.tokens,
            links_out: m.links_out,
            links_in: m.links_in,
            frontmatter_problems: &m.frontmatter_problems,
        }
    }
}

#[derive(Serialize)]
struct ReadJson<'a> {
    meta: ReadMeta<'a>,
    /// Gliederung nur beim Lesen der ganzen Notiz (wie `--outline`); bei einem Abschnitt oder
    /// `--lines` steht sie nicht dabei (`wb read <notiz> --outline --json`).
    #[serde(skip_serializing_if = "Option::is_none")]
    sections: Option<&'a [Section]>,
    line_start: u32,
    line_end: u32,
    /// Ausgewählter Text (Zeilen mit `\n`), bei Budget gekürzt.
    text: String,
    /// Erste nicht gezeigte Stelle als `Zeile` oder `Zeile:Zeichen`; `null`, wenn alles kam.
    truncated_at: Option<String>,
    sections_shown: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    links: Option<NoteLinks>,
    links_truncated: bool,
    budget: u32,
    budget_raised: bool,
    more: Vec<String>,
}

pub(crate) fn read(out: &mut Vec<u8>, ctx: &Ctx, v: &Vault, a: &ReadArgs) -> Res {
    let (path, sec_spec) = v.resolve_address(&a.address)?;
    let note = v.note(&path)?;
    let lines: Vec<&str> = note.text.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l)).collect();
    let total = note.meta.lines.max(lines.len() as u32).max(1);
    let addr = path.without_ext().to_string();
    let links = if a.links { Some(v.links(&path)?) } else { None };
    let body_start = match &note.frontmatter {
        FrontmatterState::Present { end_line, .. } => (end_line + 1).min(total),
        _ => 1,
    };
    // Auswahl bestimmen: Bereiche und Überschriften (nur zur Anzeige)
    let mut ranges: Vec<Range> = Vec::new();
    let mut titles: Vec<String> = Vec::new();
    let kind = parse_kind(&a.part)?;
    let explicit_lines = a.lines.is_some();
    if let Some(l) = &a.lines {
        ranges = parse_lines(l, total)?;
    } else if let Some(k) = kind {
        for s in note.sections.iter().filter(|s| s.kind == k) {
            ranges.push(section_range(&lines, s));
            titles.push(format!("§ {} L{}-{} ~{}t", s.title, s.line_start, s.line_end, s.tokens));
        }
        if ranges.is_empty() {
            return Err(Error::NotFound {
                what: "Abschnittsart",
                query: format!("{addr} --part {k}"),
                suggestions: note.sections.iter().filter(|s| s.kind != SectionKind::Other).map(|s| format!("{} [{}]", s.title, s.kind)).collect(),
            });
        }
    } else if let Some(spec) = &sec_spec {
        let s = v.find_section(&note, spec)?;
        ranges.push(section_range(&lines, s));
        titles.push(format!("§ {} L{}-{} ~{}t", s.title, s.line_start, s.line_end, s.tokens));
    }
    let whole = ranges.is_empty();
    let lines_cmd = |rest: &[Range]| -> CmdLine {
        let mut c = CmdLine::new(ctx, "read");
        c.positional(&addr);
        c.opt("--lines", &ranges_spec(rest));
        if a.budget != crate::READ_BUDGET {
            c.opt("--budget", &a.budget.to_string());
        }
        c
    };

    if ctx.mode == Mode::Json {
        return read_json(out, ctx, &note, &lines, &addr, a, whole, body_start, total, &ranges, links, &lines_cmd);
    }

    let (mut budget, mut raised) = effective(a.budget, MIN_READ);
    let m = &note.meta;
    let header = |budget: u32, raised: bool| -> String {
        let mut hdr = format!("{addr} {} L1-{total} ~{}t links={}/{}", m.note_type, m.tokens, m.links_out, m.links_in);
        if let Some(d) = &m.checked_on {
            hdr.push_str(&format!(" checked={d}"));
        }
        if let Some(s) = &m.status {
            hdr.push_str(&format!(" status={}", truncate_chars(s, 40)));
        }
        hdr.push_str(&budget_note(budget, raised));
        hdr
    };
    let warn = (!m.frontmatter_problems.is_empty()).then(|| {
        let codes: Vec<&str> = m.frontmatter_problems.iter().map(|p| p.split(':').next().unwrap_or(p)).collect();
        format!("warn: {}", codes.join(","))
    });
    // Links-Zeilen vorab, damit ihr Platz reserviert werden kann (höchstens 30 % des Budgets).
    let link_lines: Vec<String> = links
        .as_ref()
        .map(|l| {
            let mut v: Vec<String> = l
                .outgoing
                .iter()
                .map(|o| {
                    let t = match &o.status {
                        LinkStatus::Ok(p) => p.without_ext().to_string(),
                        LinkStatus::Dead => "DEAD".into(),
                        LinkStatus::Ambiguous(c) => format!("AMBIGUOUS({})", c.len()),
                    };
                    format!("out L{} {t}", o.line)
                })
                .collect();
            v.extend(l.incoming.iter().map(|i| format!("in {} L{}", i.source.without_ext(), i.line)));
            v
        })
        .unwrap_or_default();
    let links_cost: u32 = link_lines.iter().map(|l| cost(&clean(l))).sum();
    let links_cmd = || {
        let mut c = CmdLine::new(ctx, "read");
        c.positional(&addr);
        c.flag("--outline");
        c.flag("--links");
        let need = text_cost(&outline_text(&note)) + links_cost + cost(&header(99_999, true)) + 40;
        c.opt("--budget", &need.max(a.budget).to_string());
        c
    };
    // Obere Schranke für `more:`: längste denkbare Fortsetzung.
    let max_col = lines.iter().map(|l| l.chars().count() as u32).max().unwrap_or(1).max(1);
    // Nur für die Preisschranke: jede Zahl mindestens so lang wie jede echte.
    let wide = total.saturating_mul(10).saturating_add(9);
    let worst = Range { from: total, col: max_col, to: wide };
    let worst_rest: Vec<Range> = if whole { vec![worst] } else { ranges.iter().map(|_| worst).collect() };
    let links_note = |shown: usize| format!("note: {} von {} Links gezeigt", shown, link_lines.len());
    let outline_cmd = || {
        let mut c = CmdLine::new(ctx, "read");
        c.positional(&addr);
        c.flag("--outline");
        let need = text_cost(&outline_text(&note)) + cost(&header(99_999, true)) + 20;
        c.opt("--budget", &need.max(a.budget.saturating_add(1)).to_string());
        c
    };
    // Ganze Notiz zu groß: Gliederung gekürzt und Text als zwei Fortsetzungen möglich.
    let more_reserve = if whole { lines_cmd(&worst_rest).cost("more") + outline_cmd().cost("more") } else { lines_cmd(&worst_rest).cost("more") }
        + if links.is_some() { links_cmd().cost("more") + cost(&links_note(link_lines.len())) } else { 0 }
        + if whole { cost("note: Notiz größer als das Budget: Abschnitt per #Titel oder #L<Zeile> lesen") } else { 0 };
    // Die erste Abschnittszeile (`§ Titel L…`) kommt immer; ein langer Titel gehört in die Anhebung (§19.5).
    let first_title = if explicit_lines { 0 } else { titles.first().map(|t| cost(&clean(t))).unwrap_or(0) };
    let fixed = cost(&clean(&header(99_999, true))) + warn.as_deref().map(|w| cost(&clean(w))).unwrap_or(0) + more_reserve + first_title;
    let unit = 14; // erster Textteil bzw. erste Gliederungszeile
    if fixed + unit > budget {
        budget = fixed + unit;
        raised = true;
    }
    let mut b = Buf::new(out, ctx.color);
    b.bold(&header(budget, raised));
    if let Some(w) = &warn {
        b.line(w);
    }
    let avail = budget.saturating_sub(fixed);
    let link_share = links_cost.min(pct(avail, 30));
    let text_cap = budget.saturating_sub(more_reserve).saturating_sub(link_share);
    let fits = m.tokens.saturating_add(text_cost(&titles.join("\n"))) <= text_cap.saturating_sub(b.used);
    let mut more: Vec<CmdLine> = Vec::new();
    let mut notes: Vec<String> = Vec::new();
    if whole && (a.outline || !fits) {
        let mut cut = false;
        for (i, line) in outline_text(&note).lines().enumerate() {
            if b.used + cost(&clean(line)) > text_cap && i > 0 {
                cut = true;
                break;
            }
            b.line(line);
        }
        if cut {
            more.push(outline_cmd());
        }
        if !a.outline {
            notes.push("note: Notiz größer als das Budget: Abschnitt per #Titel oder #L<Zeile> lesen".into());
            more.push(lines_cmd(&[Range { from: body_start, col: 1, to: total }]));
        }
    } else {
        let sel = if whole { vec![Range { from: body_start, col: 1, to: total }] } else { ranges.clone() };
        let mut rest: Vec<Range> = Vec::new();
        let mut force = true;
        for (i, r) in sel.iter().enumerate() {
            if let Some(t) = titles.get(i).filter(|_| !explicit_lines) {
                if b.used + cost(&clean(t)) > text_cap && !force {
                    rest = sel[i..].to_vec();
                    break;
                }
                b.dim(t);
            }
            let left = emit_ranges(&mut b, &lines, std::slice::from_ref(r), text_cap, force);
            force = false;
            if !left.is_empty() {
                rest = left;
                rest.extend_from_slice(&sel[i + 1..]);
                break;
            }
        }
        if !rest.is_empty() {
            more.push(lines_cmd(&rest));
        }
    }
    if !link_lines.is_empty() {
        let cap = budget.saturating_sub(more_reserve);
        let mut shown = 0;
        for l in &link_lines {
            if b.used + cost(&clean(l)) > cap {
                break;
            }
            b.line(l);
            shown += 1;
        }
        if shown < link_lines.len() {
            notes.push(links_note(shown));
            more.push(links_cmd());
        }
    }
    for n in &notes {
        b.dim(n);
    }
    for c in &more {
        for l in c.lines("more") {
            b.dim(&l);
        }
    }
    Ok(exit::OK)
}

/// Bereich eines Abschnitts ohne die Überschriftszeile (die steht im Titel).
fn section_range(lines: &[&str], s: &Section) -> Range {
    let head = lines.get(s.line_start as usize - 1).is_some_and(|l| wb_core::markdown::heading(l).is_some());
    let from = if head && s.line_end > s.line_start { s.line_start + 1 } else { s.line_start };
    Range { from, col: 1, to: s.line_end.max(from) }
}

fn outline_text(note: &Note) -> String {
    let mut o = String::new();
    for s in &note.sections {
        let k = if s.kind != SectionKind::Other { format!(" [{}]", s.kind) } else { String::new() };
        let title = if s.level == 0 { "(Vorspann)".to_string() } else { format!("{} {}", "#".repeat(s.level as usize), s.title) };
        o.push_str(&format!("{} {title}{k} L{}-{} ~{}t\n", s.index, s.line_start, s.line_end, s.tokens));
    }
    o
}

/// Text der Bereiche bis höchstens `max_chars` Zeichen; dazu die erste nicht gezeigte Stelle.
fn ranges_text(lines: &[&str], ranges: &[Range], max_chars: usize) -> (String, Vec<Range>) {
    let mut out = String::new();
    let mut left = max_chars;
    for (ri, r) in ranges.iter().enumerate() {
        for n in r.from..=r.to {
            let full = lines.get(n as usize - 1).copied().unwrap_or("");
            let col = if n == r.from { r.col } else { 1 };
            let t = from_col(full, col);
            let len = t.chars().count();
            if len < left {
                out.push_str(t);
                out.push('\n');
                left -= len + 1;
                continue;
            }
            let k = left;
            out.extend(t.chars().take(k));
            let mut rest = vec![Range { from: n, col: col + k as u32, to: r.to }];
            rest.extend_from_slice(&ranges[ri + 1..]);
            return (out, rest);
        }
    }
    (out, Vec::new())
}

#[allow(clippy::too_many_arguments)]
fn read_json(
    out: &mut Vec<u8>,
    ctx: &Ctx,
    note: &Note,
    lines: &[&str],
    addr: &str,
    a: &ReadArgs,
    whole: bool,
    body_start: u32,
    total: u32,
    ranges: &[Range],
    links: Option<NoteLinks>,
    lines_cmd: &dyn Fn(&[Range]) -> CmdLine,
) -> Res {
    let sel: Vec<Range> = if whole { vec![Range { from: body_start, col: 1, to: total }] } else { ranges.to_vec() };
    let no_text = whole && a.outline;
    let (line_start, line_end) = (sel[0].from, sel.last().map(|r| r.to).unwrap_or(total));
    let full_chars: usize = if no_text { 0 } else { ranges_text(lines, &sel, usize::MAX).0.chars().count() };
    let (budget, raised) = effective(a.budget, MIN_READ);
    let links_cmd = || {
        let mut c = CmdLine::new(ctx, "read");
        c.positional(addr);
        c.flag("--outline");
        c.flag("--links");
        c
    };
    let render = |chars: usize, nsec: usize, nlinks: (usize, usize), budget: u32, raised: bool| -> String {
        let (text, rest) = if no_text { (String::new(), Vec::new()) } else { ranges_text(lines, &sel, chars) };
        let l = links.as_ref().map(|l| NoteLinks {
            outgoing: l.outgoing.iter().take(nlinks.0).cloned().collect(),
            incoming: l.incoming.iter().take(nlinks.1).cloned().collect(),
        });
        let links_truncated = links.as_ref().is_some_and(|l| nlinks.0 < l.outgoing.len() || nlinks.1 < l.incoming.len());
        let mut more = Vec::new();
        if !rest.is_empty() {
            more.push(lines_cmd(&rest).as_str().to_string());
        }
        if links_truncated {
            more.push(links_cmd().as_str().to_string());
        }
        json_lines(&ReadJson {
            meta: ReadMeta::of(&note.meta),
            sections: whole.then(|| &note.sections[..nsec]),
            line_start,
            line_end,
            text,
            truncated_at: rest.first().map(Range::spec).map(|s| s.split('-').next().unwrap_or("").to_string()),
            sections_shown: nsec,
            links: l,
            links_truncated,
            budget,
            budget_raised: raised,
            more,
        })
    };
    let nsec_all = if whole { note.sections.len() } else { 0 };
    let (lo, li) = links.as_ref().map(|l| (l.outgoing.len(), l.incoming.len())).unwrap_or((0, 0));
    let ok = |s: &str| text_cost(s) <= budget;
    let s_full = render(full_chars, nsec_all, (lo, li), budget, raised);
    let s = if ok(&s_full) {
        s_full
    } else {
        // Vorrang: Text (mindestens ein erster Teil), dann Gliederung, dann Links.
        let min_chars = full_chars.min(40);
        let chars = largest_fitting(min_chars, full_chars, |c| ok(&render(c, 0, (0, 0), budget, raised)));
        let nsec = largest_fitting(0, nsec_all, |n| ok(&render(chars, n, (0, 0), budget, raised)));
        let nout = largest_fitting(0, lo, |n| ok(&render(chars, nsec, (n, 0), budget, raised)));
        let nin = largest_fitting(0, li, |n| ok(&render(chars, nsec, (nout, n), budget, raised)));
        let s = render(chars, nsec, (nout, nin), budget, raised);
        if ok(&s) {
            s
        } else {
            let need = text_cost(&render(chars, nsec, (nout, nin), 99_999, true));
            render(chars, nsec, (nout, nin), need, true)
        }
    };
    out.extend_from_slice(s.as_bytes());
    Ok(exit::OK)
}

// ---------------- context ----------------

pub(crate) fn context(out: &mut Vec<u8>, ctx: &Ctx, v: &Vault, a: &ContextArgs) -> Res {
    if a.journal > crate::CONTEXT_JOURNAL_MAX {
        return Err(Error::Usage {
            message: format!("--journal höchstens {}", crate::CONTEXT_JOURNAL_MAX),
            hint: Some("ältere Zeilen mit `wb search <begriffe> -p <projekt>` oder im Journal lesen".into()),
        });
    }
    let target = match (&a.project, &a.cwd) {
        (Some(p), _) => ContextTarget::Project(p.clone()),
        (None, Some(d)) => ContextTarget::Cwd(d.clone()),
        (None, None) => ContextTarget::Cwd(std::env::current_dir().map(|d| d.display().to_string()).unwrap_or_default()),
    };
    let mut cx = match v.context(&target) {
        // Ohne Angabe und ohne Projekt zum Arbeitsordner: knappe Projektliste (ersetzt `00 Home.md`, §13.12).
        Err(Error::NotFound { .. }) if a.project.is_none() && a.cwd.is_none() => return project_list(out, ctx, v, a.budget),
        // Derselbe Ordner gehört zu zwei Projekten: je Kandidat der ausführbare Befehl (§17.3).
        Err(Error::Ambiguous { what, query, candidates }) if a.project.is_none() => {
            let candidates = candidates
                .iter()
                .map(|name| {
                    let mut c = CmdLine::new(ctx, "context");
                    c.positional(name);
                    format!("`{}`", c.as_str())
                })
                .collect();
            return Err(Error::Ambiguous { what, query, candidates });
        }
        r => r?,
    };
    cx.journal.truncate(a.journal);
    let budget_cmd = |m: u32| {
        let mut c = CmdLine::new(ctx, "context");
        c.positional(&cx.project.name);
        if a.journal != crate::CONTEXT_JOURNAL {
            c.opt("--journal", &a.journal.to_string());
        }
        c.opt("--budget", &m.to_string());
        c
    };
    if ctx.mode == Mode::Json {
        return context_json(out, &cx, a.budget, &budget_cmd);
    }
    // Voller Preis ohne Budget: daraus das Budget für `more:`, das alles zeigt.
    let mut scratch = Vec::new();
    let full = context_text(&mut scratch, ctx, &cx, u32::MAX / 4, false, &budget_cmd, true);
    let (budget, raised) = effective(a.budget, MIN_CONTEXT);
    if full.used <= budget {
        if !raised {
            out.extend_from_slice(&scratch);
            return Ok(exit::OK);
        }
        // Angehoben, aber alles passt: dieselbe Ausgabe, die Kopfzeile nennt die Anhebung (§19.5).
        let mut again = Vec::new();
        if context_text(&mut again, ctx, &cx, budget, true, &budget_cmd, true).used <= budget {
            out.extend_from_slice(&again);
            return Ok(exit::OK);
        }
    }
    let _ = context_text(out, ctx, &cx, budget, raised, &|_| budget_cmd(full.used.saturating_add(10)), false);
    Ok(exit::OK)
}

struct Packed {
    used: u32,
}

/// `context` als Text; `unbounded` nur für die Preisermittlung.
fn context_text(
    out: &mut Vec<u8>,
    ctx: &Ctx,
    cx: &ProjectContext,
    budget: u32,
    raised: bool,
    budget_cmd: &dyn Fn(u32) -> CmdLine,
    unbounded: bool,
) -> Packed {
    let p = &cx.project;
    let header = |budget: u32, raised: bool| {
        let mut hdr = format!("project={} notes={} ~{}t", p.name, p.notes, p.tokens);
        if let Some(d) = &p.last_checked {
            hdr.push_str(&format!(" checked={d}"));
        }
        if let Some(s) = &p.status {
            hdr.push_str(&format!(" status={}", truncate_chars(s, 30)));
        }
        hdr.push_str(&format!(" open={} match={}", p.open_items, cx.matched_by));
        hdr.push_str(&budget_note(budget, raised));
        hdr
    };
    let overview = match &cx.overview {
        Some(o) => format!("overview: {} L1-{} ~{}t", o.path.without_ext(), o.lines, o.tokens),
        None => "overview: -".to_string(),
    };
    let paths = (!p.paths.is_empty()).then(|| truncate_chars(&format!("paths{}: {}", if p.paths_inferred { "~" } else { "" }, p.paths.join(" | ")), 240));
    let section_cmd = |s: &SectionText| {
        let mut c = CmdLine::new(ctx, "read");
        c.positional(&s.address);
        c
    };
    // Obere Schranke für alle `more:`-Zeilen, die entstehen können.
    let reserve: u32 = if unbounded {
        0
    } else {
        cx.state.iter().chain(&cx.open).map(|s| section_cmd(s).cost("more")).sum::<u32>() + budget_cmd(99_999).cost("more")
    };
    let mut b = Buf::new(out, ctx.color);
    b.bold(&header(budget, raised));
    b.line(&overview);
    if let Some(l) = &paths {
        b.line(l);
    }
    let cap = budget.saturating_sub(reserve);
    let avail = cap.saturating_sub(b.used);
    let journal_lines: Vec<String> = cx
        .journal
        .iter()
        .map(|j| {
            let t = wb_core::markdown::plain_inline(&j.text);
            format!("  {}{} {}", j.date, j.time.as_deref().map(|t| format!(" {t}")).unwrap_or_default(), truncate_chars(&t, 140))
        })
        .collect();
    let journal_cost: u32 =
        if journal_lines.is_empty() { 0 } else { cost(&format!("journal {}:", journal_lines.len())) + journal_lines.iter().map(|l| cost(&clean(l))).sum::<u32>() };
    let journal_share = journal_cost.min(avail / 4);
    let mut more_sec: Vec<CmdLine> = Vec::new();
    let mut cut_list = false;
    let state_cap = b.used.saturating_add(pct(avail, 35));
    for s in &cx.state {
        if !section_block(&mut b, s, state_cap.min(cap.saturating_sub(journal_share))) {
            more_sec.push(section_cmd(s));
        }
    }
    if cx.state.is_empty() {
        b.line("state: - (keine Abschnitt Stand/Status in der Übersicht)");
    }
    let open_cap = b.used.saturating_add(pct(avail, 20));
    for s in &cx.open {
        if !section_block(&mut b, s, open_cap.min(cap.saturating_sub(journal_share))) {
            more_sec.push(section_cmd(s));
        }
    }
    let folder = cx.overview.as_ref().map(|o| format!("{}/", o.path.folder())).unwrap_or_else(|| format!("Projects/{}/", p.name));
    let topics_head = format!("topics {} in {folder} (stem date ~t [kinds] summary):", cx.topics.len());
    if b.used + cost(&clean(&topics_head)) <= cap.saturating_sub(journal_share) {
        b.bold(&topics_head);
        for t in &cx.topics {
            let rel = t.path.without_ext().strip_prefix(folder.as_str()).unwrap_or(t.path.without_ext());
            let kinds = if t.kinds.is_empty() { String::new() } else { format!(" [{}]", t.kinds.iter().map(|k| k.as_str()).collect::<Vec<_>>().join(",")) };
            let sum = t.summary.as_deref().map(|s| format!(" | {}", truncate_chars(s, 110))).unwrap_or_default();
            let line = format!("  {rel} {} ~{}t{kinds}{sum}", t.date.as_deref().unwrap_or("-"), t.tokens);
            if b.used + cost(&clean(&line)) + journal_share > cap {
                cut_list = true;
                break;
            }
            b.line(&line);
        }
        if cx.superseded > 0 {
            let mut c = CmdLine::new(ctx, "search");
            c.opt("-p", &p.name).opt("-s", "superseded").flag("<begriffe>");
            let line = format!("  superseded: {} ({})", cx.superseded, c.as_str());
            if b.used + cost(&clean(&line)) + journal_share <= cap {
                b.line(&line);
            } else {
                cut_list = true;
            }
        }
    } else {
        cut_list = !cx.topics.is_empty() || cx.superseded > 0;
    }
    if !journal_lines.is_empty() {
        let head = format!("journal {}:", journal_lines.len());
        if b.used + cost(&head) <= cap {
            b.bold(&head);
            for l in &journal_lines {
                if b.used + cost(&clean(l)) > cap {
                    cut_list = true;
                    break;
                }
                b.line(l);
            }
        } else {
            cut_list = true;
        }
    }
    for c in &more_sec {
        for l in c.lines("more") {
            b.dim(&l);
        }
    }
    if cut_list {
        for l in budget_cmd(0).lines("more") {
            b.dim(&l);
        }
    }
    Packed { used: b.used }
}

fn context_json(out: &mut Vec<u8>, cx: &ProjectContext, requested: u32, budget_cmd: &dyn Fn(u32) -> CmdLine) -> Res {
    // Schlanke Form (Vertrag §17.2): was der Text zeigt, ohne Felder, die ein anderes wiederholen
    // (Titel neben Adresse oder Pfad, Projekt an jeder Journalzeile, Metadaten der Übersicht,
    // Wochenaktivität, die nur `wb stats` braucht).
    #[derive(Serialize)]
    struct JProject<'a> {
        name: &'a str,
        notes: u32,
        bytes: u64,
        tokens: u64,
        last_modified: i64,
        #[serde(skip_serializing_if = "Option::is_none")]
        last_checked: Option<&'a str>,
        #[serde(skip_serializing_if = "Option::is_none")]
        status: Option<&'a str>,
        open_items: u32,
        findings: &'a std::collections::BTreeMap<String, u32>,
        paths: &'a [String],
        paths_inferred: bool,
    }
    #[derive(Serialize)]
    struct JOverview<'a> {
        path: &'a NotePath,
        lines: u32,
        tokens: u32,
        #[serde(skip_serializing_if = "Option::is_none")]
        checked_on: Option<&'a str>,
    }
    #[derive(Serialize)]
    struct JSection<'a> {
        address: &'a str,
        kind: SectionKind,
        line_start: u32,
        line_end: u32,
        tokens: u32,
        text: &'a str,
    }
    #[derive(Serialize)]
    struct JTopic<'a> {
        path: &'a NotePath,
        #[serde(skip_serializing_if = "Option::is_none")]
        date: Option<&'a str>,
        tokens: u32,
        kinds: &'a [SectionKind],
        #[serde(skip_serializing_if = "Option::is_none")]
        summary: Option<&'a str>,
    }
    #[derive(Serialize)]
    struct JJournal<'a> {
        date: &'a str,
        #[serde(skip_serializing_if = "Option::is_none")]
        time: Option<&'a str>,
        #[serde(skip_serializing_if = "Option::is_none")]
        target: Option<&'a str>,
        text: &'a str,
        file: &'a NotePath,
        line: u32,
    }
    #[derive(Serialize)]
    struct J<'a> {
        project: JProject<'a>,
        matched_by: &'a str,
        overview: Option<JOverview<'a>>,
        state: Vec<JSection<'a>>,
        open: Vec<JSection<'a>>,
        topics: Vec<JTopic<'a>>,
        superseded: u32,
        journal: Vec<JJournal<'a>>,
        budget: u32,
        budget_raised: bool,
        /// Gekürzte Felder (`state`, `open`, `topics`, `journal`).
        truncated: Vec<&'static str>,
        more: Option<String>,
    }
    fn sections(v: &[SectionText]) -> Vec<JSection<'_>> {
        v.iter()
            .map(|s| JSection { address: &s.address, kind: s.kind, line_start: s.line_start, line_end: s.line_end, tokens: s.tokens, text: s.text.trim_start_matches('\n') })
            .collect()
    }
    fn view<'a>(c: &'a ProjectContext, budget: u32, budget_raised: bool, truncated: Vec<&'static str>, more: Option<String>) -> J<'a> {
        let p = &c.project;
        J {
            project: JProject {
                name: &p.name,
                notes: p.notes,
                bytes: p.bytes,
                tokens: p.tokens,
                last_modified: p.last_modified,
                last_checked: p.last_checked.as_deref(),
                status: p.status.as_deref(),
                open_items: p.open_items,
                findings: &p.findings,
                paths: &p.paths,
                paths_inferred: p.paths_inferred,
            },
            matched_by: &c.matched_by,
            overview: c.overview.as_ref().map(|m| JOverview { path: &m.path, lines: m.lines, tokens: m.tokens, checked_on: m.checked_on.as_deref() }),
            state: sections(&c.state),
            open: sections(&c.open),
            topics: c
                .topics
                .iter()
                .map(|t| JTopic { path: &t.path, date: t.date.as_deref(), tokens: t.tokens, kinds: &t.kinds, summary: t.summary.as_deref() })
                .collect(),
            superseded: c.superseded,
            journal: c
                .journal
                .iter()
                .map(|j| JJournal { date: &j.date, time: j.time.as_deref(), target: j.target.as_deref(), text: &j.text, file: &j.file, line: j.line })
                .collect(),
            budget,
            budget_raised,
            truncated,
            more,
        }
    }
    let (budget, raised) = effective(requested, MIN_CONTEXT);
    let full_cost = text_cost(&json_lines(&view(cx, 99_999, true, Vec::new(), None)));
    // Kürzen: Textmenge der Abschnitte (Zeichen), Themen, Journal.
    let max_text = cx.state.iter().chain(&cx.open).map(|s| s.text.chars().count()).max().unwrap_or(0);
    let render = |chars: usize, ntop: usize, nj: usize, budget: u32, raised: bool| -> String {
        let mut c = cx.clone();
        let mut tr: Vec<&'static str> = Vec::new();
        for (name, list) in [("state", &mut c.state), ("open", &mut c.open)] {
            let mut cut = false;
            for s in list.iter_mut() {
                if s.text.chars().count() > chars {
                    s.text = s.text.chars().take(chars).collect();
                    cut = true;
                }
            }
            if cut {
                tr.push(name);
            }
        }
        if ntop < c.topics.len() {
            c.topics.truncate(ntop);
            tr.push("topics");
        }
        if nj < c.journal.len() {
            c.journal.truncate(nj);
            tr.push("journal");
        }
        let more = (!tr.is_empty()).then(|| budget_cmd(full_cost.saturating_add(10)).as_str().to_string());
        json_lines(&view(&c, budget, raised, tr, more))
    };
    let ok = |s: &str| text_cost(s) <= budget;
    let s = render(max_text, cx.topics.len(), cx.journal.len(), budget, raised);
    let s = if ok(&s) {
        s
    } else {
        let chars = largest_fitting(max_text.min(80), max_text, |c| ok(&render(c, 0, 0, budget, raised)));
        let ntop = largest_fitting(0, cx.topics.len(), |n| ok(&render(chars, n, 0, budget, raised)));
        let nj = largest_fitting(0, cx.journal.len(), |n| ok(&render(chars, ntop, n, budget, raised)));
        let s = render(chars, ntop, nj, budget, raised);
        if ok(&s) {
            s
        } else {
            let need = text_cost(&render(chars, ntop, nj, 99_999, true));
            render(chars, ntop, nj, need, true)
        }
    };
    out.extend_from_slice(s.as_bytes());
    Ok(exit::OK)
}

/// Eine Zeile je Projekt, zuletzt geänderte zuerst.
fn project_list(out: &mut Vec<u8>, ctx: &Ctx, v: &Vault, requested: u32) -> Res {
    let mut ps = v.projects()?;
    ps.sort_by(|a, b| b.last_modified.cmp(&a.last_modified).then_with(|| a.name.cmp(&b.name)));
    let date_of = |p: &ProjectInfo| p.last_checked.clone().unwrap_or_else(|| wb_core::date_of_secs(p.last_modified));
    let more_cmd = |m: u32| {
        let mut c = CmdLine::new(ctx, "context");
        c.opt("--budget", &m.to_string());
        c
    };
    let (budget, raised) = effective(requested, MIN_CONTEXT);
    if ctx.mode == Mode::Json {
        #[derive(Serialize)]
        struct Row<'a> {
            name: &'a str,
            notes: u32,
            tokens: u64,
            date: String,
            open: u32,
            status: Option<&'a str>,
            overview: Option<String>,
        }
        #[derive(Serialize)]
        struct J<'a> {
            projects: Vec<Row<'a>>,
            total: usize,
            budget: u32,
            budget_raised: bool,
            more: Option<String>,
        }
        let rows = |n: usize| -> Vec<Row> {
            ps.iter()
                .take(n)
                .map(|p| Row {
                    name: &p.name,
                    notes: p.notes,
                    tokens: p.tokens,
                    date: date_of(p),
                    open: p.open_items,
                    status: p.status.as_deref().filter(|s| NoteStatus::parse(s).is_some()),
                    overview: p.overview.as_ref().map(|o| o.without_ext().to_string()),
                })
                .collect()
        };
        let full = text_cost(&json_lines(&J { projects: rows(ps.len()), total: ps.len(), budget: 99_999, budget_raised: true, more: None }));
        let render = |n: usize, budget: u32, raised: bool| {
            let more = (n < ps.len()).then(|| more_cmd(full.saturating_add(10)).as_str().to_string());
            json_lines(&J { projects: rows(n), total: ps.len(), budget, budget_raised: raised, more })
        };
        let n = largest_fitting(ps.len().min(1), ps.len(), |n| text_cost(&render(n, budget, raised)) <= budget);
        let mut s = render(n, budget, raised);
        if text_cost(&s) > budget {
            let need = text_cost(&render(n, 99_999, true));
            s = render(n, need, true);
        }
        out.extend_from_slice(s.as_bytes());
        return Ok(exit::OK);
    }
    let header = |b: u32, r: bool| {
        format!("projects={} (kein Projekt zum Arbeitsordner; name notes ~t date open status){}", ps.len(), budget_note(b, r))
    };
    let row = |p: &ProjectInfo| {
        let status = p.status.as_deref().and_then(NoteStatus::parse).map(|s| format!(" {s}")).unwrap_or_default();
        format!("  {} {} ~{}t {} open={}{status}", shell_word(&p.name).0, p.notes, p.tokens, date_of(p), p.open_items)
    };
    let note = "note: `wb context <projekt>` öffnet ein Projekt";
    let all: u32 = cost(&header(99_999, true)) + ps.iter().map(|p| cost(&clean(&row(p)))).sum::<u32>() + cost(note);
    let reserve = more_cmd(99_999).cost("more") + cost(note);
    let mut b = Buf::new(out, ctx.color);
    b.bold(&header(budget, raised));
    let mut shown = 0;
    for p in &ps {
        let line = row(p);
        if b.used + cost(&clean(&line)) + reserve > budget && shown > 0 {
            break;
        }
        b.line(&line);
        shown += 1;
    }
    if shown < ps.len() {
        for l in more_cmd(all.saturating_add(10)).lines("more") {
            b.dim(&l);
        }
    }
    b.dim(note);
    Ok(exit::OK)
}

/// Abschnitt mit Text bis `cap` ausgeben; `false`, wenn gekürzt.
fn section_block(b: &mut Buf, s: &SectionText, cap: u32) -> bool {
    let head = format!("{}: {} L{}-{} ~{}t", s.kind, s.address, s.line_start, s.line_end, s.tokens);
    if b.used + cost(&clean(&head)) > cap {
        return false;
    }
    b.bold(&head);
    for line in s.text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let l = clean(&format!("  {}", line.trim_end())).into_owned();
        if b.used + cost(&l) > cap {
            let room = cap.saturating_sub(b.used);
            if room >= 8 {
                let k = cut_chars(&l, room);
                if k > 2 {
                    let part: String = l.chars().take(k).collect();
                    b.line(&format!("{part}…"));
                }
            }
            return false;
        }
        b.line(&l);
    }
    true
}

// ---------------- check ----------------

pub(crate) fn check(out: &mut Vec<u8>, ctx: &Ctx, v: &Vault, a: &CheckArgs) -> Res {
    if a.limit == 0 {
        return Err(Error::Usage { message: "--limit muss mindestens 1 sein".into(), hint: Some("--limit 50 (Standard)".into()) });
    }
    // Ein Pfad, unter dem keine Notiz liegt (auch außerhalb des Vaults), ist ein leeres Ergebnis (§19.5).
    if let Some(p) = &a.prefix {
        let key = p.replace('\\', "/").to_lowercase();
        let any = v.notes(&NoteFilter { include_journal: true, ..Default::default() })?.iter().any(|n| n.path.as_str().to_lowercase().starts_with(&key));
        if !any {
            return Err(Error::NotFound { what: "Pfad im Vault", query: p.clone(), suggestions: Vec::new() });
        }
    }
    let mut f = v.check(&CheckOptions { prefix: a.prefix.clone(), today: a.today.clone(), stale_days: None })?;
    // §23.2: Fehler vor Warnungen vor Hinweisen, darin wie bisher nach Pfad und Zeile; so
    // schneidet `--limit` nie einen Fehler hinter Hinweisen ab. Gilt für Text und JSON.
    f.sort_by_key(|x| x.severity);
    // §15.6: gelaufen heißt 0, in Text und JSON gleich; --strict: 1 bei Befunden ab Warnung.
    let code = if a.strict && f.iter().any(|x| x.severity != Severity::Info) { exit::NOT_FOUND } else { exit::OK };
    let mut by_code: std::collections::BTreeMap<&str, u32> = Default::default();
    for x in &f {
        *by_code.entry(x.code.as_str()).or_default() += 1;
    }
    // §23.2: Über den ganzen Vault bündelt die Textausgabe Hinweise (info) je Regel zu einer
    // Zeile; einzeln stehen sie mit Pfadpräfix, mit --all und immer in JSON.
    let bundle = ctx.mode != Mode::Json && a.prefix.is_none() && !a.all;
    let listed = if bundle { f.iter().take_while(|x| x.severity != Severity::Info).count() } else { f.len() };
    let start = a.offset.min(listed);
    let end = start.saturating_add(a.limit).min(listed);
    // Fortsetzung, gleich für Text und JSON (§19.4).
    let more = (end < listed).then(|| {
        let mut c = CmdLine::new(ctx, "check");
        if let Some(p) = &a.prefix {
            c.positional(p);
        }
        if let Some(t) = &a.today {
            c.opt("--today", t);
        }
        if a.strict {
            c.flag("--strict");
        }
        if a.all {
            c.flag("--all");
        }
        if a.limit != crate::CHECK_LIMIT {
            c.opt("--limit", &a.limit.to_string());
        }
        c.opt("--offset", &end.to_string());
        c
    });
    if ctx.mode == Mode::Json {
        #[derive(Serialize)]
        struct J<'a> {
            total: usize,
            counts: &'a std::collections::BTreeMap<&'a str, u32>,
            offset: usize,
            shown: usize,
            #[serde(skip_serializing_if = "Option::is_none")]
            more: Option<&'a str>,
            findings: &'a [Finding],
        }
        let s = json_lines(&J { total: f.len(), counts: &by_code, offset: start, shown: end - start, more: more.as_ref().map(|c| c.as_str()), findings: &f[start..end] });
        out.extend_from_slice(s.as_bytes());
        return Ok(code);
    }
    let mut b = Buf::new(out, ctx.color);
    b.line(&format!(
        "findings={} {}",
        f.len(),
        by_code.iter().map(|(k, n)| format!("{k}={n}")).collect::<Vec<_>>().join(" ")
    ));
    for x in &f[start..end] {
        b.line(&format!("{}:{} {} {}: {} | hint: {}", x.path.without_ext(), x.line, severity(x.severity), x.code, x.message, x.hint));
    }
    if let Some(c) = &more {
        b.line(&format!("note: Befunde {}-{} von {}", start + 1, end, f.len()));
        for l in c.lines("more") {
            b.line(&l);
        }
    } else if listed < f.len() {
        // Gebündelte Hinweise: je Regel die Zahl und die Ordner mit den meisten Befunden.
        let mut rules: std::collections::BTreeMap<&str, (&str, std::collections::BTreeMap<String, u32>)> = Default::default();
        for x in &f[listed..] {
            let r = rules.entry(x.code.as_str()).or_insert((x.hint.as_str(), Default::default()));
            *r.1.entry(check_folder(x.path.as_str())).or_default() += 1;
        }
        for (rule, (hint, folders)) in &rules {
            let mut v: Vec<(&String, u32)> = folders.iter().map(|(k, n)| (k, *n)).collect();
            v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
            let mut parts: Vec<String> = v.iter().take(CHECK_FOLDERS).map(|(k, n)| format!("{k} {n}")).collect();
            if v.len() > CHECK_FOLDERS {
                let rest: u32 = v[CHECK_FOLDERS..].iter().map(|x| x.1).sum();
                parts.push(format!("+{rest} in {} weiteren", v.len() - CHECK_FOLDERS));
            }
            b.line(&format!("info {rule} {}: {} | hint: {hint}", by_code[rule], parts.join(", ")));
        }
        b.line("note: Hinweise (info) je Regel gebündelt; einzeln mit `wb check <Ordner>` oder `wb check --all`");
    }
    Ok(code)
}

/// So viele Ordner nennt eine gebündelte Hinweiszeile von `wb check`.
const CHECK_FOLDERS: usize = 6;

/// Ordner eines Befunds für die Bündelung, zugleich Präfix für `wb check <Ordner>`: die ersten
/// zwei Pfadteile (`Projects/<Projekt>/`, `Sources/Web/`), bei flacheren Pfaden einer.
fn check_folder(path: &str) -> String {
    let parts: Vec<&str> = path.split('/').collect();
    match parts.len() {
        0 | 1 => "(Vault-Wurzel)".to_string(),
        n => format!("{}/", parts[..(n - 1).min(2)].join("/")),
    }
}

fn severity(s: Severity) -> &'static str {
    match s {
        Severity::Error => "error",
        Severity::Warning => "warn",
        Severity::Info => "info",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoting_is_portable() {
        assert_eq!(shell_word("Projects/Demo/Messung#Stand"), ("Projects/Demo/Messung#Stand".into(), true));
        assert_eq!(shell_word("Größe neu"), ("'Größe neu'".into(), true));
        assert_eq!(shell_word("Preis $5 `x`"), ("'Preis $5 `x`'".into(), true));
        assert_eq!(shell_word("Peter's Notiz"), ("\"Peter's Notiz\"".into(), true));
        assert_eq!(shell_word("it's $x"), ("'it'\\''s $x'".into(), false));
        assert_eq!(shell_word("-foo").0, "'-foo'");
        assert_eq!(shell_word("").0, "''");
    }

    #[test]
    fn control_chars_are_visible() {
        assert_eq!(clean("a\u{1b}[31mb\u{7}c\td"), "a\u{241b}[31mb\u{2407}c\td");
        assert_eq!(clean("\u{9b}x\u{7f}"), "\u{fffd}x\u{2421}");
        assert_eq!(clean("a\u{202e}b\u{2028}c\u{2029}d\u{200f}"), "a\u{fffd}b\u{2424}c\u{2424}d\u{fffd}");
        assert!(matches!(clean("normal äöü"), Cow::Borrowed(_)));
    }

    #[test]
    fn lines_grammar() {
        let r = parse_lines("8:1201-10,12-,3", 20).unwrap();
        assert_eq!(ranges_spec(&r), "8:1201-10,12-20,3");
        assert!(parse_lines("0-3", 20).is_err());
        assert!(parse_lines("5-3", 20).is_err());
        assert_eq!(parse_lines("21", 20).unwrap_err().exit_code(), exit::NOT_FOUND);
        assert_eq!(parse_lines("4-999", 20).unwrap()[0].to, 20);
    }

    #[test]
    fn cut_prefers_sentence_then_space_then_char() {
        let s = "Erster Satz hier. Zweiter Satz folgt ohne Ende";
        assert_eq!(cut_chars(s, 7), "Erster Satz hier.".chars().count());
        let long = "x".repeat(500);
        let k = cut_chars(&long, 20);
        assert!(k > 0 && cost(&format!("{}…", "x".repeat(k))) <= 20);
    }
}
