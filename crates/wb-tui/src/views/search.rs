//! Ansicht 3: Suche mit Vorschau. Frage: Wo stand das noch?

use crate::app::*;
use crate::model::*;
use crate::text::{highlight, put, put_spans, query_terms, term_marks, truncate, wrap, Spans};
use crate::ui::*;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;

pub(crate) fn hit_from_note(n: NoteInfo) -> SearchHit {
    SearchHit {
        path: n.path,
        title: n.title,
        project: n.project,
        ntype: n.ntype,
        status: n.status,
        modified: n.modified,
        section: None,
        line_start: 1,
        line_end: 1,
        tokens: n.tokens,
        snippet: n.summary,
        marks: Vec::new(),
    }
}

const FILTERS: [&str; 3] = ["alle", "Themen", "Quellen"];
const LIMIT: usize = 60;

impl App<'_> {
    pub(crate) fn refresh_search(&mut self) {
        let prev = self.se.hits.get(self.se.sel).map(|h| (h.path.clone(), h.line_start));
        let q = self.se.query.trim().to_string();
        let mut hits: Vec<SearchHit> = if q.is_empty() {
            self.src.recent(40).into_iter().map(hit_from_note).collect()
        } else {
            self.src.search(&q, LIMIT)
        };
        self.se.total_unfiltered = hits.len();
        match self.se.filter {
            1 => hits.retain(|h| h.ntype != NoteType::Source),
            2 => hits.retain(|h| h.ntype == NoteType::Source),
            _ => {}
        }
        self.se.sel = prev
            .and_then(|(p, l)| hits.iter().position(|h| h.path == p && h.line_start == l))
            .unwrap_or(0);
        self.se.hits = hits;
    }

    pub(crate) fn search_headline(&self) -> String {
        let q = self.se.query.trim();
        if q.is_empty() {
            return format!("Zuletzt geändert · {} Notizen · tippen zum Suchen", self.se.hits.len());
        }
        let n = self.se.hits.len();
        if n == 0 {
            return format!("„{q}“ · keine Treffer");
        }
        let mut projects: Vec<&str> = self.se.hits.iter().filter_map(|h| h.project.as_deref()).collect();
        projects.sort();
        projects.dedup();
        let more = if self.se.total_unfiltered >= LIMIT { "+" } else { "" };
        let proj = if projects.len() == 1 { "1 Projekt".to_string() } else { format!("{} Projekten", projects.len()) };
        format!("„{q}“ · {n}{more} Treffer in {proj}")
    }

    pub(crate) fn search_hints(&self) -> Vec<(&'static str, &'static str)> {
        vec![("↑↓", "wählen"), ("Enter", "öffnen"), ("Tab", "Filter"), ("Esc", "leeren/zurück"), ("F1–F4", "Ansicht")]
    }

    pub(crate) fn search_key(&mut self, key: KeyEvent) {
        let ctrl = crate::app::is_ctrl(&key);
        let n = self.se.hits.len();
        let page = (self.area.get().height as usize).saturating_sub(12).max(3);
        match key.code {
            KeyCode::Char('n') if ctrl => self.se.sel = (self.se.sel + 1).min(n.saturating_sub(1)),
            KeyCode::Char('p') if ctrl => self.se.sel = self.se.sel.saturating_sub(1),
            KeyCode::Char('u') if ctrl => {
                self.se.query.clear();
                self.refresh_search();
            }
            KeyCode::Char(c) if !ctrl => {
                self.se.query.push(c);
                self.se.sel = 0;
                self.refresh_search();
            }
            KeyCode::Backspace => {
                self.se.query.pop();
                self.se.sel = 0;
                self.refresh_search();
            }
            KeyCode::Down => self.se.sel = (self.se.sel + 1).min(n.saturating_sub(1)),
            KeyCode::Up => self.se.sel = self.se.sel.saturating_sub(1),
            KeyCode::PageDown => self.se.sel = (self.se.sel + page).min(n.saturating_sub(1)),
            KeyCode::PageUp => self.se.sel = self.se.sel.saturating_sub(page),
            KeyCode::Home => self.se.sel = 0,
            KeyCode::End => self.se.sel = n.saturating_sub(1),
            KeyCode::Tab => {
                self.se.filter = (self.se.filter + 1) % 3;
                self.refresh_search();
            }
            KeyCode::BackTab => {
                self.se.filter = (self.se.filter + 2) % 3;
                self.refresh_search();
            }
            KeyCode::Enter => {
                if let Some(h) = self.se.hits.get(self.se.sel).cloned() {
                    self.open_reader_at(&h.path, Some(h.line_start));
                }
            }
            KeyCode::Esc => {
                if self.se.query.is_empty() {
                    self.go_back();
                } else {
                    self.se.query.clear();
                    self.refresh_search();
                }
            }
            _ => {}
        }
    }

    pub(crate) fn search_draw(&self, body: Rect, buf: &mut Buffer) {
        let theme = self.theme;
        let g = theme.glyphs();
        // Eingabezeile
        let prompt = format!("{} ", g.arrow);
        let x = put(buf, body.x + 1, body.y, &prompt, 2, theme.accent());
        let qw = body.width.saturating_sub(24) as usize;
        let q = truncate(&self.se.query, qw, g.ellipsis);
        let x = put(buf, x, body.y, &q, qw, theme.title());
        put(buf, x, body.y, &g.cursor.to_string(), 1, theme.accent());
        if self.se.query.is_empty() {
            put(buf, x + 1, body.y, "Begriffe eingeben …", 20, theme.dim());
        }
        // Filter rechts
        let mut fx = body.x + body.width;
        for (i, f) in FILTERS.iter().enumerate().rev() {
            let t = format!(" {f} ");
            let tw = t.chars().count() as u16;
            if fx < body.x + tw + 2 {
                break;
            }
            fx -= tw;
            put(buf, fx, body.y, &t, tw as usize, if i as u8 == self.se.filter { theme.tab_active() } else { theme.dim() });
        }

        let wide = body.width >= 100;
        let list_top = body.y + 2;
        let avail_h = body.height.saturating_sub(2);
        if self.se.hits.is_empty() {
            let msg = if self.se.query.trim().is_empty() { "Noch keine Notizen." } else { "Keine Treffer. Weniger oder andere Begriffe versuchen." };
            put(buf, body.x + 2, list_top, msg, body.width as usize - 4, theme.dim());
            return;
        }
        let (list_rect, prev_rect) = if wide {
            let lw = (body.width * 45 / 100).max(44);
            (
                Rect::new(body.x, list_top, lw, avail_h),
                Rect::new(body.x + lw + 1, list_top, body.width - lw - 2, avail_h),
            )
        } else {
            let lh = (avail_h * 45 / 100).max(5).min(avail_h.saturating_sub(5));
            (
                Rect::new(body.x, list_top, body.width, lh),
                Rect::new(body.x + 1, list_top + lh + 1, body.width - 2, avail_h.saturating_sub(lh + 1)),
            )
        };
        let per = if wide { 2 } else { 1 };
        let rows = (list_rect.height as usize) / per;
        let off = scroll_offset(self.se.sel, self.se.hits.len(), rows);
        for r in 0..rows {
            let Some(h) = self.se.hits.get(off + r) else { break };
            let y = list_rect.y + (r * per) as u16;
            let selected = off + r == self.se.sel;
            if selected {
                crate::text::fill(buf, Rect::new(list_rect.x, y, list_rect.width, per as u16), theme.selected());
            }
            let fr = Freshness::of(h.modified, self.today);
            let age = h.modified.map(|d| self.today.since(d).max(0));
            let crumb = match (&h.project, &h.section) {
                (Some(p), Some(s)) => format!("{p} {} {s}", g.arrow),
                (Some(p), None) => p.clone(),
                (None, Some(s)) => format!("Quelle {} {s}", g.arrow),
                (None, None) => "Quelle".to_string(),
            };
            let old = h.status == Some(Status::Superseded);
            let tstyle = if old {
                theme.dim()
            } else if selected {
                theme.title()
            } else {
                theme.text()
            };
            let ty = if old {
                format!("{} ", g.superseded)
            } else if h.ntype == NoteType::Source {
                "Q ".to_string()
            } else {
                String::new()
            };
            let crumb = if old && wide { format!("{crumb} · überholt") } else { crumb };
            if wide {
                let mut rw = RowWriter::new(buf, list_rect.x, y, list_rect.width);
                rw.cell(&(off + r + 1).to_string(), 2, Align::Right, theme.dim(), g.ellipsis);
                rw.gap();
                rw.cell(&g.freshness(fr).to_string(), 1, Align::Left, theme.fresh(fr), g.ellipsis);
                rw.gap();
                let tw = (list_rect.width as usize).saturating_sub(13);
                rw.cell(&format!("{ty}{}", h.title), tw, Align::Left, tstyle, g.ellipsis);
                rw.gap();
                rw.cell(&age_label(age), 5, Align::Right, theme.fresh(fr), g.ellipsis);
                put(buf, list_rect.x + 5, y + 1, &truncate(&crumb, list_rect.width as usize - 7, g.ellipsis), list_rect.width as usize - 7, theme.dim());
            } else {
                let mut rw = RowWriter::new(buf, list_rect.x, y, list_rect.width);
                rw.cell(&(off + r + 1).to_string(), 2, Align::Right, theme.dim(), g.ellipsis);
                rw.gap();
                rw.cell(&g.freshness(fr).to_string(), 1, Align::Left, theme.fresh(fr), g.ellipsis);
                rw.gap();
                let total = (list_rect.width as usize).saturating_sub(14);
                let tw = (total * 50 / 100).max(10);
                rw.cell(&format!("{ty}{}", h.title), tw, Align::Left, tstyle, g.ellipsis);
                rw.gap();
                rw.cell(&crumb, total.saturating_sub(tw + 1), Align::Left, theme.dim(), g.ellipsis);
                rw.gap();
                rw.cell(&age_label(age), 5, Align::Right, theme.fresh(fr), g.ellipsis);
            }
        }

        // Trennlinie bzw. senkrechter Strich
        if wide {
            for y in list_top..list_top + avail_h {
                put(buf, list_rect.x + list_rect.width, y, &g.sep.to_string(), 1, theme.dim());
            }
        } else {
            put(buf, body.x + 1, prev_rect.y.saturating_sub(1), &g.rule.to_string().repeat(body.width as usize - 2), body.width as usize - 2, theme.dim());
        }
        if let Some(h) = self.se.hits.get(self.se.sel) {
            self.draw_preview(buf, prev_rect, h);
        }
    }

    fn draw_preview(&self, buf: &mut Buffer, r: Rect, h: &SearchHit) {
        if r.height == 0 || r.width < 10 {
            return;
        }
        let theme = self.theme;
        let g = theme.glyphs();
        let w = r.width as usize;
        let mut y = r.y;
        if h.status == Some(Status::Superseded) {
            let t = format!("{} {}  (überholt)", g.superseded, h.title);
            put(buf, r.x, y, &truncate(&t, w, g.ellipsis), w, theme.dim());
        } else {
            put(buf, r.x, y, &truncate(&h.title, w, g.ellipsis), w, theme.title());
        }
        y += 1;
        let crumb = match (&h.project, &h.section) {
            (Some(p), Some(s)) => format!("{p} {a} {} {a} {s}", h.title, a = g.arrow),
            (Some(p), None) => format!("{p} {} {}", g.arrow, h.title),
            (None, Some(s)) => format!("{} {} {s}", h.ntype.label(), g.arrow),
            (None, None) => h.ntype.label().to_string(),
        };
        if r.height > 6 {
            put(buf, r.x, y, &truncate(&crumb, w, g.ellipsis), w, theme.dim());
            y += 1;
        }
        let ago = h.modified.map(|d| format!(" · geändert {d} ({})", age_label(Some(self.today.since(d).max(0))))).unwrap_or_default();
        let lines = if h.line_end > h.line_start { format!("L{}–{}", h.line_start, h.line_end) } else { "Notiz".to_string() };
        put(buf, r.x, y, &truncate(&format!("{lines} · {}{ago}", tokens_short(h.tokens) + " T"), w, g.ellipsis), w, theme.dim());
        y += 1;
        put(buf, r.x, y, &g.rule.to_string().repeat(w), w, theme.dim());
        y += 1;
        let max_lines = (r.y + r.height).saturating_sub(y + 1) as usize;
        let lines = self.passage_lines(h, max_lines, w);
        for (i, l) in lines.iter().take(max_lines).enumerate() {
            put_spans(buf, r.x, y + i as u16, l, w);
        }
        if r.y + r.height > y {
            put(buf, r.x, r.y + r.height - 1, "Enter öffnet die Notiz an dieser Stelle", w, theme.dim());
        }
    }

    /// Die Stelle der Notiz zur Vorschau: der Abschnitt des Treffers (ab der ersten Zeile mit Treffer),
    /// sonst der Notizanfang; Suchwörter hervorgehoben. Fehlt der Text, bleibt der Schnipsel.
    fn passage_lines(&self, h: &SearchHit, max_lines: usize, w: usize) -> Vec<Spans> {
        let theme = self.theme;
        let g = theme.glyphs();
        let terms = query_terms(&self.se.query);
        let fallback = || {
            let spans = highlight(&h.snippet, &h.marks, theme.text(), theme.hit());
            wrap(&spans, w, 0, 0)
        };
        let Some(doc) = self.doc(&h.path) else { return fallback() };
        let all: Vec<&str> = doc.text.lines().collect();
        let (from, to) = if h.line_end > h.line_start {
            (h.line_start as usize, (h.line_end as usize).min(all.len()))
        } else {
            (frontmatter_end(&all), all.len())
        };
        if from >= to {
            return fallback();
        }
        let body: Vec<&str> = all[from..to].to_vec();
        // erste Zeile mit Treffer, eine Zeile Vorlauf
        let first_hit = body.iter().position(|l| !term_marks(l, &terms).is_empty());
        let mut start = first_hit.map(|i| i.saturating_sub(1)).unwrap_or(0);
        // Leerzeilen und die Trennlinie des Frontmatters sind kein Vorlauf
        while start < body.len() && (body[start].trim().is_empty() || body[start].trim() == "---") {
            start += 1;
        }
        let mut out: Vec<Spans> = Vec::new();
        if start > 0 && first_hit.is_some() {
            out.push(vec![(g.ellipsis.to_string(), theme.dim())]);
        }
        for l in &body[start..] {
            if out.len() >= max_lines {
                break;
            }
            if l.trim().is_empty() {
                continue;
            }
            // Überschriften ohne rohe `#`: nur der Text, hervorgehoben wie ein Titel
            let t = l.trim_start();
            let hashes = t.chars().take_while(|c| *c == '#').count();
            let (line, base) = if (1..=6).contains(&hashes) && t[hashes..].starts_with(' ') {
                (t[hashes..].trim(), theme.title())
            } else {
                (*l, theme.text())
            };
            let marks = term_marks(line, &terms);
            let spans = highlight(line, &marks, base, theme.hit());
            out.extend(wrap(&spans, w, 0, 2));
        }
        if out.is_empty() {
            return fallback();
        }
        out
    }
}

/// Zeile nach dem Frontmatter (0-basiert); ohne Frontmatter 0.
fn frontmatter_end(lines: &[&str]) -> usize {
    if lines.first().map(|l| l.trim()) != Some("---") {
        return 0;
    }
    lines.iter().skip(1).position(|l| l.trim() == "---").map(|i| i + 2).unwrap_or(0)
}
