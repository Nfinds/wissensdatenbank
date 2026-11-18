//! Ansicht 2: Projektkarte. Frage: Was steckt in diesem Projekt, was ist frisch, groß, fehlt?

use crate::app::*;
use crate::model::*;
use crate::text::{put, put_spans, truncate, wrap};
use crate::ui::*;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;

impl App<'_> {
    /// Übersicht zuerst, dann nach gewählter Sortierung.
    pub(crate) fn pm_order(&self) -> Vec<&NoteInfo> {
        let mut v: Vec<&NoteInfo> = self.pm.notes.iter().collect();
        let today = self.today;
        let age = |n: &NoteInfo| n.modified.map(|d| today.since(d)).unwrap_or(i32::MAX);
        match self.pm.sort {
            NoteSort::Recent => v.sort_by(|a, b| age(a).cmp(&age(b)).then_with(|| a.path.cmp(&b.path))),
            NoteSort::Size => v.sort_by(|a, b| b.tokens.cmp(&a.tokens).then_with(|| a.path.cmp(&b.path))),
            NoteSort::Name => v.sort_by_key(|a| a.title.to_lowercase()),
            NoteSort::Findings => v.sort_by(|a, b| {
                b.findings.total().cmp(&a.findings.total()).then_with(|| a.path.cmp(&b.path))
            }),
        }
        // stabil: Übersicht an den Anfang
        v.sort_by_key(|n| if n.ntype == NoteType::Overview { 0 } else { 1 });
        v
    }

    pub(crate) fn pm_sel_pos(&self, order: &[&NoteInfo]) -> usize {
        self.pm.sel.as_ref().and_then(|s| order.iter().position(|n| &n.path == s)).unwrap_or(0)
    }

    pub(crate) fn project_headline(&self) -> String {
        let Some(name) = self.pm.name.as_deref().filter(|n| !n.is_empty()) else {
            return "Kein Projekt gewählt – in der Übersicht mit Enter öffnen".to_string();
        };
        match self.project(name) {
            Some(p) => {
                let age = p.last_change.map(|d| self.today.since(d).max(0));
                let ago = match age {
                    Some(0) => "heute".to_string(),
                    Some(a) => age_label(Some(a)),
                    None => "unbekannt".to_string(),
                };
                let bef = match (p.findings.total(), p.findings.other) {
                    (0, _) => String::new(),
                    (t, 0) => format!(" · {t} {}", if t == 1 { "Befund" } else { "Befunde" }),
                    (t, o) => format!(" · {t} {}, {o} andere", if t == 1 { "Befund" } else { "Befunde" }),
                };
                format!(
                    "{name} · {} {} · {} · {ago} · {} offen{bef}",
                    p.notes,
                    if p.notes == 1 { "Notiz" } else { "Notizen" },
                    bytes_short(p.bytes),
                    p.open_points
                )
            }
            None => format!("{name} · {} Notizen", self.pm.notes.len()),
        }
    }

    pub(crate) fn project_hints(&self) -> Vec<(&'static str, &'static str)> {
        vec![("↑↓", "wählen"), ("Enter", "lesen"), ("g", "Graph"), ("s", "Sortierung"), ("Tab", "nächstes Projekt"), ("Esc", "zurück")]
    }

    pub(crate) fn project_key(&mut self, key: KeyEvent) {
        let order = self.pm_order();
        if order.is_empty() {
            if key.code == KeyCode::Esc {
                self.go_back();
            }
            return;
        }
        let pos = self.pm_sel_pos(&order);
        let page = (self.area.get().height as usize).saturating_sub(10).max(3);
        let paths: Vec<String> = order.iter().map(|n| n.path.clone()).collect();
        let set = |app: &mut Self, p: usize| app.pm.sel = Some(paths[p.min(paths.len() - 1)].clone());
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => set(self, pos + 1),
            KeyCode::Up | KeyCode::Char('k') => set(self, pos.saturating_sub(1)),
            KeyCode::PageDown => set(self, pos + page),
            KeyCode::PageUp => set(self, pos.saturating_sub(page)),
            KeyCode::Home => set(self, 0),
            KeyCode::End => set(self, paths.len() - 1),
            KeyCode::Char('s') => self.pm.sort = self.pm.sort.next(),
            KeyCode::Enter => self.open_reader(&paths[pos]),
            KeyCode::Char('g') => self.open_graph(Level::Notes, &paths[pos], 1),
            KeyCode::Tab | KeyCode::BackTab => {
                let ord = self.ov_order();
                if ord.is_empty() {
                    return;
                }
                let cur = self.pm.name.as_deref().unwrap_or("");
                let at = ord.iter().position(|&i| self.projects[i].name == cur).unwrap_or(0);
                let next = if key.code == KeyCode::Tab { (at + 1) % ord.len() } else { (at + ord.len() - 1) % ord.len() };
                let name = self.projects[ord[next]].name.clone();
                self.ov.sel = Some(name.clone());
                self.load_project(&name);
            }
            KeyCode::Esc => self.go_back(),
            _ => {}
        }
    }

    pub(crate) fn project_draw(&self, body: Rect, buf: &mut Buffer) {
        let theme = self.theme;
        let g = theme.glyphs();
        let order = self.pm_order();
        if order.is_empty() {
            put(buf, body.x + 1, body.y + 1, "Keine Notizen.", 14, theme.dim());
            return;
        }
        let pos = self.pm_sel_pos(&order);
        let wide = body.width >= 120;
        let listw = if wide { 78u16 } else { body.width };
        let detail_h = 3u16;
        let proj = self.pm.name.as_deref().and_then(|n| self.project(n));
        let top = if proj.is_some_and(|p| p.state.is_some() || p.status.is_some()) { 1 } else { 0 };
        let list_h = body.height.saturating_sub(detail_h + top);
        let rows = list_h.saturating_sub(1) as usize;
        let max_tok = order.iter().map(|n| n.tokens).max().unwrap_or(1).max(1);

        // Stand des Projekts: Freitext aus `state`, davor die Aufzählung aus `status`
        if let Some(p) = proj.filter(|_| top == 1) {
            let mut line = String::from("Stand");
            if let Some(st) = p.status {
                line.push_str(&format!(" [{}]", st.label()));
            }
            if let Some(t) = &p.state {
                line.push_str(&format!("  {t}"));
            }
            put(buf, body.x + 1, body.y, &truncate(&line, listw as usize - 2, g.ellipsis), listw as usize - 2, theme.dim());
        }
        let y0 = body.y + top;

        // Kopfzeile
        {
            let mut rw = RowWriter::new(buf, body.x, y0, listw);
            let sorted = |k: NoteSort| self.pm.sort == k;
            let hs = |k: NoteSort| if sorted(k) { theme.accent() } else { theme.dim() };
            rw.cell("", 1, Align::Left, theme.dim(), g.ellipsis);
            rw.gap();
            rw.cell("", 1, Align::Left, theme.dim(), g.ellipsis);
            rw.gap();
            let title_w = (listw as usize).saturating_sub(41).max(10);
            rw.cell("Thema", title_w, Align::Left, hs(NoteSort::Name), g.ellipsis);
            rw.gap();
            rw.cell("Größe", 8, Align::Left, hs(NoteSort::Size), g.ellipsis);
            rw.gap();
            rw.cell("Token", 5, Align::Right, theme.dim(), g.ellipsis);
            rw.gap();
            rw.cell("Alter", 5, Align::Right, hs(NoteSort::Recent), g.ellipsis);
            rw.gap();
            rw.cell("SOELPQ", 6, Align::Left, theme.dim(), g.ellipsis);
            rw.gap();
            rw.cell(FINDING_HEADER, 7, Align::Left, hs(NoteSort::Findings), g.ellipsis);
        }

        let off = scroll_offset(pos, order.len(), rows);
        let title_w = (listw as usize).saturating_sub(41).max(10);
        for r in 0..rows {
            let Some(n) = order.get(off + r) else { break };
            let y = y0 + 1 + r as u16;
            let selected = off + r == pos;
            if selected {
                crate::text::fill(buf, Rect::new(body.x, y, listw, 1), theme.selected());
            }
            let fr = Freshness::of(n.modified, self.today);
            let age = n.modified.map(|d| self.today.since(d).max(0));
            let mut rw = RowWriter::new(buf, body.x, y, listw);
            rw.cell(&g.freshness(fr).to_string(), 1, Align::Left, theme.fresh(fr), g.ellipsis);
            rw.gap();
            let old = n.superseded();
            let tm = if old { g.superseded } else { n.ntype.mark() };
            rw.cell(&tm.to_string(), 1, Align::Left, if old { theme.dim() } else { theme.accent() }, g.ellipsis);
            rw.gap();
            let ts = if old {
                theme.dim()
            } else if selected {
                theme.title()
            } else {
                theme.text()
            };
            rw.cell(&n.title, title_w, Align::Left, ts, g.ellipsis);
            rw.gap();
            let b = size_bar(sqrt_frac(n.tokens as u64, max_tok as u64), 8, &theme);
            rw.cell(&b, 8, Align::Left, theme.bar(), g.ellipsis);
            rw.gap();
            rw.cell(&tokens_short(n.tokens), 5, Align::Right, theme.dim(), g.ellipsis);
            rw.gap();
            rw.cell(&age_label(age), 5, Align::Right, theme.fresh(fr), g.ellipsis);
            rw.gap();
            let marks: Vec<(String, ratatui::style::Style)> = SectionKind::MARKS
                .iter()
                .map(|(k, c)| {
                    if n.has(*k) {
                        (c.to_string(), theme.title())
                    } else {
                        (g.dot_none.to_string(), theme.dim())
                    }
                })
                .collect();
            rw.spans(&marks, 6);
            rw.gap();
            rw.spans(&finding_cells(&n.findings, &theme), 7);
        }
        
        // Detailzeilen zur Auswahl
        let sel = order[pos];
        let dy = body.y + body.height - detail_h;
        let dw = listw as usize - 2;
        let sum = if sel.summary.is_empty() { sel.title.clone() } else { sel.summary.clone() };
        let line1 = vec![(sel.title.clone(), theme.title()), ("  ".to_string(), theme.text()), (sum, theme.dim())];
        let wrapped = wrap(&line1, dw, 0, 0);
        if let Some(l) = wrapped.first() {
            let mut l = l.clone();
            if wrapped.len() > 1 {
                // die Zusammenfassung läuft in die nächste Zeile: Ende der ersten mit „…“ kennzeichnen
                let used: usize = l.iter().map(|(t, _)| crate::text::width(t)).sum();
                if used + 1 > dw {
                    if let Some(last) = l.last_mut() {
                        last.0 = truncate(&last.0, crate::text::width(&last.0).saturating_sub(used + 1 - dw).max(2), g.ellipsis);
                    }
                } else {
                    l.push((g.ellipsis.to_string(), theme.dim()));
                }
            }
            put_spans(buf, body.x + 1, dy, &l, dw);
        }
        let kinds: Vec<&str> = SectionKind::MARKS
            .iter()
            .filter(|(k, _)| sel.has(*k))
            .map(|(k, _)| k.label())
            .collect();
        let info = format!(
            "{}{}← {} eingehend · → {} ausgehend · {}",
            if sel.superseded() { "überholt · " } else { "" },
            if sel.findings.other > 0 { format!("{} andere Befunde · ", sel.findings.other) } else { String::new() },
            sel.links_in,
            sel.links_out,
            if kinds.is_empty() { "keine bekannten Abschnitte".to_string() } else { kinds.join(" · ") }
        );
        put(buf, body.x + 1, dy + 1, &truncate(&info, dw, g.ellipsis), dw, theme.dim());
        // Legende der Abschnittsspalte, damit niemand die Hilfe öffnen muss
        let legend = format!(
            "S Stand · O Offen · E Entscheidungen · L Lehren · P Prüfungen · Q Quellen · {} überholt",
            g.superseded
        );
        let short = format!("S Stand O Offen E Entsch. L Lehren P Prüf. Q Quellen {} überholt", g.superseded);
        let legend = if legend.chars().count() <= dw { legend } else { short };
        put(buf, body.x + 1, dy + 2, &truncate(&legend, dw, g.ellipsis), dw, theme.dim());

        // Nachbarschaft der Auswahl (breit)
        if wide {
            let px = body.x + listw + 1;
            let pw = body.width - listw - 2;
            for y in body.y..body.y + body.height {
                put(buf, px - 1, y, &g.sep.to_string(), 1, theme.dim());
            }
            put(buf, px + 1, body.y, "Nachbarschaft", 13, theme.title());
            let gr = Rect::new(px + 1, body.y + 2, pw - 1, body.height.saturating_sub(2));
            self.draw_ego_notes(buf, gr, &sel.path, 1, None);
        }
    }
}
