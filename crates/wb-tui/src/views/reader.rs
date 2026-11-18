//! Leser: Markdown lesbar, Gliederung, Rück- und Vorwärtslinks. Gehört zur Suchansicht.

use crate::app::*;
use crate::md;
use crate::model::*;
use crate::text::{put, put_spans, truncate};
use crate::ui::*;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;

struct Geometry {
    text: Rect,
    side: Option<Rect>,
}

fn geometry(body: Rect) -> Geometry {
    if body.width >= 100 {
        let sw = 36u16;
        Geometry {
            text: Rect::new(body.x + 1, body.y, body.width - sw - 3, body.height),
            side: Some(Rect::new(body.x + body.width - sw - 1, body.y, sw, body.height)),
        }
    } else {
        // schmal: die letzte Zeile gehört dem Linkzähler, der Text endet eine Zeile davor
        Geometry { text: Rect::new(body.x + 1, body.y, body.width.saturating_sub(2), body.height.saturating_sub(1)), side: None }
    }
}

impl App<'_> {
    pub(crate) fn body_rect(&self) -> Rect {
        let a = self.area.get();
        Rect::new(a.x, a.y + 2, a.width, a.height.saturating_sub(3))
    }

    /// Verknüpfungen in Anzeigereihenfolge: erst ausgehend, dann eingehend.
    pub(crate) fn reader_link_list(&self) -> Vec<(bool, Link)> {
        let Some(p) = &self.rd.path else { return Vec::new() };
        let l = self.links_of(p);
        let mut v: Vec<(bool, Link)> = l.out.iter().cloned().map(|x| (true, x)).collect();
        v.extend(l.inc.iter().cloned().map(|x| (false, x)));
        v
    }

    fn reader_render(&self, width: usize) -> Option<(md::Rendered, std::rc::Rc<NoteDoc>)> {
        let path = self.rd.path.as_deref()?;
        let doc = self.doc(path)?;
        let links = self.links_of(path);
        let lookup = |t: &str| links.out.iter().position(|l| l.path == t).map(|i| i + 1);
        let r = md::render(&doc.text, width, &self.theme, &lookup);
        Some((r, doc))
    }

    /// Öffnet eine Notiz und springt zur Quellzeile (1-basiert).
    pub(crate) fn open_reader_at(&mut self, path: &str, line: Option<u32>) {
        self.go(Loc::Reader { path: path.to_string(), scroll: 0 });
        if let Some(l) = line {
            if l > 1 {
                let g = geometry(self.body_rect());
                if let Some((r, _)) = self.reader_render(g.text.width as usize) {
                    if let Some(&at) = r.src_to_out.get(l as usize - 1) {
                        self.rd.scroll = at.saturating_sub(1);
                    }
                }
            }
        }
    }

    pub(crate) fn reader_headline(&self) -> String {
        let Some(path) = self.rd.path.as_deref() else { return "Keine Notiz".into() };
        match self.doc(path) {
            Some(d) => {
                let i = &d.info;
                let age = i.modified.map(|m| self.today.since(m).max(0));
                let proj = i.project.as_deref().map(|p| format!("{p} › ")).unwrap_or_default();
                format!(
                    "{proj}{} · {} · {} · geändert {} · {}",
                    i.title,
                    i.ntype.label(),
                    tokens_short(i.tokens) + " T",
                    i.modified.map(|d| d.to_string()).unwrap_or_else(|| "?".into()),
                    match age {
                        Some(0) => "heute".to_string(),
                        Some(a) => format!("vor {}", age_label(Some(a))),
                        None => String::new(),
                    }
                )
            }
            None => format!("Notiz nicht gefunden: {path}"),
        }
    }

    pub(crate) fn reader_hints(&self) -> Vec<(&'static str, &'static str)> {
        if self.rd.focus_links {
            vec![("↑↓", "Link wählen"), ("Enter", "folgen"), ("Tab", "zurück zum Text"), ("Esc", "zurück")]
        } else {
            vec![("↑↓", "blättern"), ("Space", "Seite"), ("[ ]", "Abschnitt"), ("Tab", "Links"), ("g", "Graph"), ("e", "Editor"), ("Esc", "zurück")]
        }
    }

    pub(crate) fn reader_key(&mut self, key: KeyEvent) {
        let body = self.body_rect();
        let g = geometry(body);
        let page = (g.text.height as usize).saturating_sub(2).max(1);
        if self.rd.focus_links {
            let links = self.reader_link_list();
            let n = links.len();
            match key.code {
                KeyCode::Down | KeyCode::Char('j') => self.rd.link_sel = (self.rd.link_sel + 1).min(n.saturating_sub(1)),
                KeyCode::Up | KeyCode::Char('k') => self.rd.link_sel = self.rd.link_sel.saturating_sub(1),
                KeyCode::Tab | KeyCode::BackTab => self.rd.focus_links = false,
                KeyCode::Enter => {
                    if let Some((_, l)) = links.get(self.rd.link_sel) {
                        let p = l.path.clone();
                        self.open_reader(&p);
                    }
                }
                KeyCode::Esc | KeyCode::Backspace => self.rd.focus_links = false,
                _ => {}
            }
            return;
        }
        let total = self.reader_render(g.text.width as usize).map(|(r, _)| r.lines.len()).unwrap_or(0);
        let max_scroll = total.saturating_sub(g.text.height as usize);
        let sc = self.rd.scroll;
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => self.rd.scroll = (sc + 1).min(max_scroll),
            KeyCode::Up | KeyCode::Char('k') => self.rd.scroll = sc.saturating_sub(1),
            KeyCode::PageDown | KeyCode::Char(' ') => self.rd.scroll = (sc + page).min(max_scroll),
            KeyCode::PageUp => self.rd.scroll = sc.saturating_sub(page),
            KeyCode::Home => self.rd.scroll = 0,
            KeyCode::End => self.rd.scroll = max_scroll,
            KeyCode::Char(']') | KeyCode::Char('[') => {
                if let Some((r, _)) = self.reader_render(g.text.width as usize) {
                    let target = if key.code == KeyCode::Char(']') {
                        r.headings.iter().copied().find(|&h| h > sc).unwrap_or(sc)
                    } else {
                        r.headings.iter().copied().rev().find(|&h| h < sc).unwrap_or(0)
                    };
                    self.rd.scroll = target.min(max_scroll);
                }
            }
            KeyCode::Tab | KeyCode::BackTab | KeyCode::Char('l') => {
                if !self.reader_link_list().is_empty() {
                    self.rd.focus_links = true;
                }
            }
            KeyCode::Char('g') => {
                if let Some(p) = self.rd.path.clone() {
                    self.open_graph(Level::Notes, &p, 1);
                }
            }
            KeyCode::Esc | KeyCode::Backspace => self.go_back(),
            _ => {}
        }
    }

    pub(crate) fn reader_draw(&self, body: Rect, buf: &mut Buffer) {
        let theme = self.theme;
        let g = theme.glyphs();
        let geo = geometry(body);
        let Some((r, doc)) = self.reader_render(geo.text.width as usize) else {
            put(buf, body.x + 2, body.y + 1, "Notiz nicht gefunden.", 21, theme.dim());
            return;
        };
        let h = geo.text.height as usize;
        let max_scroll = r.lines.len().saturating_sub(h);
        let sc = self.rd.scroll.min(max_scroll);
        for (i, l) in r.lines.iter().skip(sc).take(h).enumerate() {
            put_spans(buf, geo.text.x, geo.text.y + i as u16, l, geo.text.width as usize);
        }
        // Rollposition rechts außen im Textfeld
        if r.lines.len() > h && h > 2 {
            let frac = sc as f64 / max_scroll.max(1) as f64;
            let y = geo.text.y + (frac * (h - 1) as f64).round() as u16;
            put(buf, geo.text.x + geo.text.width, y, &g.eighths[3].to_string(), 1, theme.dim());
        }

        let Some(side) = geo.side else {
            // schmal: Linkzähler in der eigenen letzten Zeile unter dem Text
            let links = self.reader_link_list();
            if !links.is_empty() {
                let out = links.iter().filter(|(o, _)| *o).count();
                let t = format!("Tab: Links ({out} aus, {} ein)", links.len() - out);
                let tw = t.chars().count() as u16;
                put(buf, body.x + body.width - tw - 1, body.y + body.height - 1, &t, tw as usize, theme.dim());
            }
            if self.rd.focus_links {
                self.draw_link_overlay(buf, body);
            }
            return;
        };
        for y in body.y..body.y + body.height {
            put(buf, side.x - 1, y, &g.sep.to_string(), 1, theme.dim());
        }
        let mut y = side.y;
        put(buf, side.x + 1, y, "Gliederung", 10, theme.title());
        y += 1;
        let max_outline = (side.height as usize / 2).max(4);
        // aktuell = der letzte Abschnitt, dessen Überschrift schon erreicht ist; mit Pfeil markiert,
        // damit der Unterschied auch ohne Farbe und Abdunkelung sichtbar bleibt
        let current = (0..doc.sections.len().min(r.headings.len())).rev().find(|&i| r.headings[i] <= sc + 1);
        for (idx, s) in doc.sections.iter().enumerate().take(max_outline) {
            if y >= side.y + side.height {
                break;
            }
            let mark = if s.kind == SectionKind::Other { g.dot_none } else { s.kind.mark() };
            let active = r.headings.get(idx).map(|&hl| hl <= sc + 1).unwrap_or(false);
            if Some(idx) == current {
                put(buf, side.x, y, &g.arrow.to_string(), 1, theme.accent());
            }
            put(buf, side.x + 1, y, &mark.to_string(), 1, theme.accent());
            put(buf, side.x + 3, y, &truncate(&s.title, side.width as usize - 11, g.ellipsis), side.width as usize - 11, if active { theme.text() } else { theme.dim() });
            let t = tokens_short(s.tokens);
            put(buf, side.x + side.width - t.chars().count() as u16, y, &t, t.chars().count(), theme.dim());
            y += 1;
        }
        y += 1;
        let links = self.reader_link_list();
        let out_n = links.iter().filter(|(o, _)| *o).count();
        if y < side.y + side.height {
            put(buf, side.x + 1, y, &format!("Links  → {out_n}  ← {}", links.len() - out_n), side.width as usize - 1, theme.title());
            y += 1;
        }
        let room = (side.y + side.height).saturating_sub(y) as usize;
        let off = scroll_offset(self.rd.link_sel, links.len(), room);
        for (shown, (i, (out, l))) in links.iter().enumerate().skip(off).take(room).enumerate() {
            let yy = y + shown as u16;
            let sel = self.rd.focus_links && i == self.rd.link_sel;
            if sel {
                crate::text::fill(buf, Rect::new(side.x, yy, side.width, 1), theme.selected());
            }
            let arrow = if *out { "→" } else { "←" };
            let label = match (&l.project, l.ntype) {
                (_, NoteType::Source) => format!("Q {}", l.title),
                (Some(p), _) if Some(p) != self.doc(self.rd.path.as_deref().unwrap_or("")).and_then(|d| d.info.project.clone()).as_ref() => format!("{p} {} {}", g.arrow, l.title),
                _ => l.title.clone(),
            };
            let num = if *out { format!("{:>2}", i + 1) } else { "  ".to_string() };
            put(buf, side.x + 1, yy, &num, 2, theme.dim());
            put(buf, side.x + 4, yy, arrow, 1, theme.dim());
            put(buf, side.x + 6, yy, &truncate(&label, side.width as usize - 7, g.ellipsis), side.width as usize - 7, if sel { theme.title() } else { theme.link() });
        }
        if links.is_empty() && room > 0 {
            put(buf, side.x + 1, y, "keine", 5, theme.dim());
        }
    }

    fn draw_link_overlay(&self, buf: &mut Buffer, body: Rect) {
        let theme = self.theme;
        let g = theme.glyphs();
        let links = self.reader_link_list();
        let h = (links.len() as u16 + 2).min(body.height.saturating_sub(2)).max(3);
        let r = Rect::new(body.x, body.y + body.height - h, body.width, h);
        crate::text::fill(buf, r, theme.selected().remove_modifier(ratatui::style::Modifier::BOLD));
        for y in r.y..r.y + r.height {
            for x in r.x..r.x + r.width {
                if let Some(c) = buf.cell_mut((x, y)) {
                    c.set_symbol(" ");
                }
            }
        }
        for x in r.x..r.x + r.width {
            put(buf, x, r.y, &g.rule.to_string(), 1, theme.dim());
        }
        put(buf, r.x + 1, r.y, " Links ", 7, theme.title());
        let room = (h as usize).saturating_sub(2);
        let off = scroll_offset(self.rd.link_sel, links.len(), room);
        for (i, (out, l)) in links.iter().enumerate().skip(off).take(room) {
            let y = r.y + 1 + (i - off) as u16;
            let sel = i == self.rd.link_sel;
            if sel {
                crate::text::fill(buf, Rect::new(r.x, y, r.width, 1), theme.tab_active());
            }
            let t = format!("{} {}", if *out { "→" } else { "←" }, l.title);
            put(buf, r.x + 1, y, &truncate(&t, r.width as usize - 2, g.ellipsis), r.width as usize - 2, if sel { theme.tab_active() } else { theme.link() });
        }
    }
}
