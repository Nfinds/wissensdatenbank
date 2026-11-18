//! Ansicht 1: Übersicht. Frage: Wo liegt mein Wissen, was lebt, was verrottet?

use crate::app::*;
use crate::model::*;
use crate::text::{put, put_spans, truncate};
use crate::ui::*;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;

/// Aufzählungspunkte aus Abschnitten der Art `kind` (ohne Aufzählungszeichen).
pub(crate) fn section_bullets(doc: &NoteDoc, kind: SectionKind) -> Vec<String> {
    let lines: Vec<&str> = doc.text.lines().collect();
    let mut out = Vec::new();
    for s in doc.sections.iter().filter(|s| s.kind == kind) {
        let from = s.line_start as usize; // Zeile nach der Überschrift (0-basiert)
        let to = (s.line_end as usize).min(lines.len());
        for l in lines.iter().take(to).skip(from) {
            let t = l.trim_start();
            if let Some(r) = t.strip_prefix("- ").or_else(|| t.strip_prefix("* ")) {
                out.push(r.trim().to_string());
            }
        }
    }
    out
}

/// Erste Zeile eines Abschnitts der Art `kind`: der erste Aufzählungspunkt, sonst die erste
/// Textzeile (ohne Überschrift, Leerzeilen, Zitat- und Aufzählungszeichen).
pub(crate) fn section_first_line(doc: &NoteDoc, kind: SectionKind) -> Option<String> {
    if let Some(b) = section_bullets(doc, kind).into_iter().next() {
        return Some(b);
    }
    let lines: Vec<&str> = doc.text.lines().collect();
    for s in doc.sections.iter().filter(|s| s.kind == kind) {
        let from = s.line_start as usize;
        let to = (s.line_end as usize).min(lines.len());
        for l in lines.iter().take(to).skip(from) {
            let t = l.trim().trim_start_matches('>').trim();
            if t.is_empty() || t.starts_with('#') || t.starts_with("---") {
                continue;
            }
            return Some(t.to_string());
        }
    }
    None
}

struct Cols {
    name: usize,
    spark: bool,
    act: bool,
    notes: bool,
    bar: bool,
    kb: bool,
    age: bool,
    open: bool,
    /// Breite der Spalte „Status“ (nur die Aufzählung), 0 = keine.
    status: usize,
}

const STATUS_W: usize = 9;

/// Spaltenwahl nach Breite: bei Platzmangel fallen zuerst Größenangabe, Balken, Σ4W, Offen,
/// Sparkline, Alter und Notizen weg; Name, Punkt und Befunde bleiben immer.
fn columns(w: usize, show_status: bool) -> Cols {
    let avail = w.saturating_sub(1);
    let mut c = Cols { name: 0, spark: true, act: true, notes: true, bar: true, kb: true, age: true, open: true, status: 0 };
    let sum = |c: &Cols| -> usize {
        let mut t = 0;
        for (on, wi) in [(c.spark, 12), (c.act, 4), (c.notes, 4), (c.bar, 6), (c.kb, 5), (c.age, 5), (c.open, 5)] {
            if on {
                t += 1 + wi;
            }
        }
        t
    };
    // Punkt und Lücke (2), Lücke und Befunde (8)
    let fixed = 2 + 8;
    let name_min = 16usize;
    let drops: [fn(&mut Cols); 7] = [
        |c| c.kb = false,
        |c| c.bar = false,
        |c| c.act = false,
        |c| c.open = false,
        |c| c.spark = false,
        |c| c.age = false,
        |c| c.notes = false,
    ];
    let mut i = 0;
    while fixed + name_min + sum(&c) > avail && i < drops.len() {
        drops[i](&mut c);
        i += 1;
    }
    let rest = avail.saturating_sub(fixed + sum(&c));
    if show_status && rest >= name_min + 1 + STATUS_W {
        c.status = STATUS_W;
        c.name = (rest - 1 - STATUS_W).min(40);
    } else {
        c.name = rest.clamp(1, 40);
    }
    c
}

impl App<'_> {
    pub(crate) fn ov_order(&self) -> Vec<usize> {
        let mut idx: Vec<usize> = (0..self.projects.len()).collect();
        if self.ov.hide_inactive {
            idx.retain(|&i| {
                let p = &self.projects[i];
                p.activity(4) > 0 || p.last_change.map(|d| self.today.since(d) <= 30).unwrap_or(false)
            });
        }
        let p = &self.projects;
        let today = self.today;
        let age = |i: usize| p[i].last_change.map(|d| today.since(d)).unwrap_or(i32::MAX);
        match self.ov.sort {
            OvSort::Activity => idx.sort_by(|&a, &b| {
                p[b].activity(4)
                    .cmp(&p[a].activity(4))
                    .then_with(|| age(a).cmp(&age(b)))
                    .then_with(|| p[a].name.cmp(&p[b].name))
            }),
            OvSort::Size => idx.sort_by(|&a, &b| p[b].bytes.cmp(&p[a].bytes).then_with(|| p[a].name.cmp(&p[b].name))),
            OvSort::Age => idx.sort_by(|&a, &b| age(b).cmp(&age(a)).then_with(|| p[a].name.cmp(&p[b].name))),
            OvSort::Name => idx.sort_by(|&a, &b| p[a].name.to_lowercase().cmp(&p[b].name.to_lowercase())),
            OvSort::Findings => idx.sort_by(|&a, &b| {
                p[b].findings.total().cmp(&p[a].findings.total()).then_with(|| p[a].name.cmp(&p[b].name))
            }),
        }
        if self.ov.reverse {
            idx.reverse();
        }
        idx
    }

    pub(crate) fn ov_sel_pos(&self, order: &[usize]) -> usize {
        self.ov
            .sel
            .as_ref()
            .and_then(|n| order.iter().position(|&i| &self.projects[i].name == n))
            .unwrap_or(0)
    }

    /// Kopfsatz in der längsten Fassung, die in `avail` Spalten passt. „aktiv“ heißt nur
    /// `status: active`; die zweite Zahl nennt ihren Zeitraum (Änderung in den letzten 7 Tagen).
    pub(crate) fn overview_headline_fit(&self, avail: usize) -> String {
        let n = self.projects.len();
        let notes: u32 = self.projects.iter().map(|p| p.notes).sum();
        let bytes: u64 = self.projects.iter().map(|p| p.bytes).sum();
        let active = self.projects.iter().filter(|p| p.status == Some(Status::Active)).count();
        let week = self
            .projects
            .iter()
            .filter(|p| p.last_change.map(|d| self.today.since(d) <= Freshness::FRESH_DAYS).unwrap_or(false))
            .count();
        let bad = self.projects.iter().filter(|p| p.findings.total() > 0).count();
        let sources = self.src.sources();
        let pl = |k: usize, one: &str, many: &str| format!("{k} {}", if k == 1 { one } else { many });
        let size = bytes_short(bytes);
        let tail = if self.ov.hide_inactive { " · nur Änderung in 30 Tagen" } else { "" };
        let proj = pl(n, "Projekt", "Projekte");
        let nt = pl(notes as usize, "Notiz", "Notizen");
        let src = pl(sources as usize, "Quelle", "Quellen");
        let variants = [
            format!("{proj} · {nt} · {src} · {size} · {active} aktiv · {week} in 7 Tagen geändert · Befunde in {bad}{tail}"),
            format!("{proj} · {nt} · {src} · {active} aktiv · {week} in 7 Tagen geändert · Befunde in {bad}{tail}"),
            format!("{proj} · {nt} · {active} aktiv · {week} in 7 T. geändert · Befunde in {bad}{tail}"),
            format!("{proj} · {active} aktiv · {week} in 7 T. geändert · Befunde in {bad}{tail}"),
            format!("{proj} · {active} aktiv · {week} in 7 T. · {bad} mit Befund{tail}"),
        ];
        let last = variants.len() - 1;
        variants.into_iter().enumerate().find(|(i, v)| *i == last || crate::text::width(v) <= avail).map(|(_, v)| v).unwrap_or_default()
    }

    pub(crate) fn overview_hints(&self) -> Vec<(&'static str, &'static str)> {
        vec![("↑↓", "wählen"), ("Enter", "Projekt"), ("s", "Sortierung"), ("f", "30 Tage"), ("g", "Graph"), ("/", "Suche")]
    }

    pub(crate) fn overview_key(&mut self, key: KeyEvent) {
        let order = self.ov_order();
        if order.is_empty() {
            return;
        }
        let pos = self.ov_sel_pos(&order);
        let page = (self.area.get().height as usize).saturating_sub(14).max(3);
        let set = |app: &mut Self, p: usize| {
            app.ov.sel = Some(app.projects[order[p.min(order.len() - 1)]].name.clone());
        };
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => set(self, pos + 1),
            KeyCode::Up | KeyCode::Char('k') => set(self, pos.saturating_sub(1)),
            KeyCode::PageDown => set(self, pos + page),
            KeyCode::PageUp => set(self, pos.saturating_sub(page)),
            KeyCode::Home => set(self, 0),
            KeyCode::End => set(self, order.len() - 1),
            KeyCode::Char('s') => {
                self.ov.sort = self.ov.sort.next();
                self.ov.reverse = false;
            }
            KeyCode::Char('S') => self.ov.reverse = !self.ov.reverse,
            KeyCode::Char('f') => {
                self.ov.hide_inactive = !self.ov.hide_inactive;
                let o = self.ov_order();
                if !o.iter().any(|&i| Some(&self.projects[i].name) == self.ov.sel.as_ref()) {
                    self.ov.sel = o.first().map(|&i| self.projects[i].name.clone());
                }
            }
            KeyCode::Enter => {
                let name = self.projects[order[pos]].name.clone();
                self.open_project(&name);
            }
            KeyCode::Char('g') => {
                let name = self.projects[order[pos]].name.clone();
                self.open_graph(Level::Projects, &name, 1);
            }
            KeyCode::Esc => {}
            _ => {}
        }
    }

    pub(crate) fn overview_draw(&self, body: Rect, buf: &mut Buffer) {
        let theme = self.theme;
        let g = theme.glyphs();
        let order = self.ov_order();
        let pos = if self.plain { 0 } else { self.ov_sel_pos(&order) };
        // Statische Ausgabe (`wb stats`): höchstens STATS_ROWS Projekte, darunter „n von m“, dann immer
        // die Heatmap (Vertrag §16.4). Interaktiv: Heatmap ab 20 Zeilen.
        let shown_plain = order.len().min(crate::stats::STATS_ROWS);
        let hint_row = self.plain && order.len() > shown_plain;
        // unter 26 Spalten neben der Heatmap steht die Legende in einer eigenen Zeile darunter (auch interaktiv)
        let legend_below = heat_side_width(body.width) < 26;
        let heat_h = if self.plain || body.height >= 20 { (HEATMAP_ROWS + legend_below as u16).min(body.height) } else { 0 };
        let table_h = if self.plain {
            (2 + shown_plain.max(1) as u16 + hint_row as u16).min(body.height.saturating_sub(heat_h))
        } else {
            body.height - heat_h
        };
        let rows = if self.plain { shown_plain } else { table_h.saturating_sub(1) as usize };
        let show_status = self.projects.iter().any(|p| p.status.is_some());
        let cols = columns(body.width as usize, show_status);
        let max_bytes = self.projects.iter().map(|p| p.bytes).max().unwrap_or(0);

        // Kopfzeile der Tabelle
        {
            let mut rw = RowWriter::new(buf, body.x, body.y, body.width);
            let sorted = |k: OvSort| self.ov.sort == k;
            let hs = |k: OvSort| if sorted(k) { theme.accent() } else { theme.dim() };
            let marker = if self.ov.reverse { g.sort_up } else { g.sort_down };
            let mut mark_at: Vec<(u16, OvSort)> = Vec::new();
            rw.cell("", 1, Align::Left, theme.dim(), g.ellipsis);
            rw.gap();
            mark_at.push((rw.x, OvSort::Name));
            rw.cell("Projekt", cols.name, Align::Left, hs(OvSort::Name), g.ellipsis);
            if cols.spark {
                rw.gap();
                mark_at.push((rw.x, OvSort::Activity));
                rw.cell("12 Wochen", 12, Align::Left, hs(OvSort::Activity), g.ellipsis);
            }
            if cols.act {
                rw.gap();
                rw.cell("Σ4W", 4, Align::Right, hs(OvSort::Activity), g.ellipsis);
            }
            if cols.notes {
                rw.gap();
                rw.cell("Not.", 4, Align::Right, theme.dim(), g.ellipsis);
            }
            if cols.bar {
                rw.gap();
                mark_at.push((rw.x, OvSort::Size));
                rw.cell("Größe", 6, Align::Left, hs(OvSort::Size), g.ellipsis);
            }
            if cols.kb {
                rw.gap();
                rw.cell("", 5, Align::Right, theme.dim(), g.ellipsis);
            }
            if cols.age {
                rw.gap();
                mark_at.push((rw.x, OvSort::Age));
                rw.cell("Alter", 5, Align::Right, hs(OvSort::Age), g.ellipsis);
            }
            if cols.open {
                rw.gap();
                rw.cell("Offen", 5, Align::Right, theme.dim(), g.ellipsis);
            }
            rw.gap();
            mark_at.push((rw.x, OvSort::Findings));
            rw.cell(FINDING_HEADER, 7, Align::Left, hs(OvSort::Findings), g.ellipsis);
            if cols.status > 0 {
                rw.gap();
                rw.cell("Status", cols.status, Align::Left, theme.dim(), g.ellipsis);
            }
            for (x, k) in mark_at {
                if sorted(k) && x > body.x {
                    put(buf, x - 1, body.y, &marker.to_string(), 1, theme.accent());
                }
            }
        }

        if order.is_empty() && table_h > 1 {
            let msg = if self.projects.is_empty() {
                "Noch keine Projekte. Notizen unter einem Projektordner anlegen oder `wb migrate` ausführen."
            } else {
                "Keine Projekte mit Änderung in den letzten 30 Tagen. `f` zeigt alle."
            };
            put(buf, body.x + 1, body.y + 1, &truncate(msg, (body.width as usize).saturating_sub(2), g.ellipsis), (body.width as usize).saturating_sub(2), theme.dim());
        }

        // Zeilen
        let off = scroll_offset(pos, order.len(), rows);
        for r in 0..rows {
            let Some(&pi) = order.get(off + r) else { break };
            let p = &self.projects[pi];
            let y = body.y + 1 + r as u16;
            let selected = !self.plain && off + r == pos;
            if selected {
                crate::text::fill(buf, Rect::new(body.x, y, body.width, 1), theme.selected());
            }
            let old = p.status == Some(Status::Superseded);
            let fr = Freshness::of(p.last_change, self.today);
            let age = p.last_change.map(|d| self.today.since(d).max(0));
            let mut rw = RowWriter::new(buf, body.x, y, body.width);
            rw.cell(&g.freshness(fr).to_string(), 1, Align::Left, theme.fresh(fr), g.ellipsis);
            rw.gap();
            let name_style = if old {
                theme.dim()
            } else if selected {
                theme.title()
            } else {
                theme.text()
            };
            let name = if old { format!("{} {}", g.superseded, p.name) } else { p.name.clone() };
            rw.cell(&name, cols.name, Align::Left, name_style, g.ellipsis);
            if cols.spark {
                rw.gap();
                let sp = sparkline(&p.weekly, &theme);
                rw.spans(&sp, 12);
            }
            if cols.act {
                rw.gap();
                let a = p.activity(4);
                let (t, st) = if a == 0 { ("·".to_string(), theme.dim()) } else { (a.to_string(), theme.text()) };
                rw.cell(&t, 4, Align::Right, st, g.ellipsis);
            }
            if cols.notes {
                rw.gap();
                rw.cell(&p.notes.to_string(), 4, Align::Right, theme.text(), g.ellipsis);
            }
            if cols.bar {
                rw.gap();
                let b = size_bar(sqrt_frac(p.bytes, max_bytes), 6, &theme);
                rw.cell(&b, 6, Align::Left, theme.bar(), g.ellipsis);
            }
            if cols.kb {
                rw.gap();
                rw.cell(&bytes_short(p.bytes), 5, Align::Right, theme.dim(), g.ellipsis);
            }
            if cols.age {
                rw.gap();
                rw.cell(&age_label(age), 5, Align::Right, theme.fresh(fr), g.ellipsis);
            }
            if cols.open {
                rw.gap();
                let (ot, os) = if p.open_points == 0 { ("·".to_string(), theme.dim()) } else { (p.open_points.to_string(), theme.text()) };
                rw.cell(&ot, 5, Align::Right, os, g.ellipsis);
            }
            rw.gap();
            let fc = finding_cells(&p.findings, &theme);
            rw.spans(&fc, 7);
            if cols.status > 0 {
                rw.gap();
                let label = p.status.map(|s| s.label()).unwrap_or("");
                rw.cell(label, cols.status, Align::Left, theme.dim(), g.ellipsis);
            }
        }

        if hint_row {
            let t = format!("{} von {} Projekten (nach Aktivität); `wb ui` zeigt alle", shown_plain, order.len());
            let y = body.y + 1 + shown_plain as u16;
            put(buf, body.x + 1, y, &truncate(&t, (body.width as usize).saturating_sub(2), g.ellipsis), (body.width as usize).saturating_sub(2), theme.dim());
        }
        if heat_h == 0 {
            return;
        }
        let used = if self.plain { 1 + shown_plain.max(1) as u16 + hint_row as u16 } else { order.len() as u16 + 1 };
        let gap = table_h.saturating_sub(used).min(1);
        let hy = body.y + (used + gap).min(table_h);
        let hr = Rect::new(body.x + 1, hy, body.width.saturating_sub(2), HEATMAP_ROWS.min(body.height.saturating_sub(hy - body.y)));
        let weeks = heat_weeks(body.width);
        let (sum, right) = draw_heatmap(buf, hr, &self.series, &self.series, self.today, weeks, &theme);
        put_spans(buf, hr.x, hr.y, &heat_caption(&sum, weeks, &theme, hr.width as usize), hr.width as usize);
        // rechts daneben: Legende und Auswahl
        let px = right + 3;
        let pw = (hr.x + hr.width).saturating_sub(px) as usize;
        if pw < 26 {
            // schmal: Legende in einer Zeile unter der Karte
            let y = hr.y + HEATMAP_ROWS;
            if y < body.y + body.height {
                draw_legend_row(buf, body.x + 1, y, hr.width as usize, &theme);
            }
        }
        if pw >= 26 {
            put(buf, px, hr.y + 1, "Befunde", 7, theme.title());
            let legend = finding_legend();
            let mut x = px;
            for (i, (k, d)) in legend.iter().enumerate() {
                let (lx, ly) = if i < 2 { (px, hr.y + 2) } else { (px, hr.y + 3) };
                if i == 0 || i == 2 {
                    x = lx;
                } else {
                    x += 2;
                }
                x = put(buf, x, ly, k, 1, theme.finding(i));
                x = put(buf, x + 1, ly, d, pw.saturating_sub((x - px) as usize + 1), theme.dim());
            }
            put(buf, px, hr.y + 4, &truncate(PLUS_LEGEND, pw, g.ellipsis), pw, theme.dim());
            if let Some(p) = self.ov.sel.as_deref().filter(|_| !self.plain).and_then(|n| self.project(n)) {
                let bef = match (p.findings.total(), p.findings.other) {
                    (0, _) => String::new(),
                    (t, 0) => format!("  ·  {t} {}", if t == 1 { "Befund" } else { "Befunde" }),
                    (t, o) => format!("  ·  {t} {}, {o} andere", if t == 1 { "Befund" } else { "Befunde" }),
                };
                put(buf, px, hr.y + 5, &truncate(&format!("{} {}{bef}", g.arrow, p.name), pw, g.ellipsis), pw, theme.title());
                if let Some(st) = &p.state {
                    put(buf, px, hr.y + 6, &truncate(&format!("Stand   {st}"), pw, g.ellipsis), pw, theme.dim());
                }
                if let Some(path) = p.paths.first() {
                    put(buf, px, hr.y + 7, &truncate(&format!("Pfad    {path}"), pw, g.ellipsis), pw, theme.dim());
                }
                // dieselbe Zahl wie die Spalte „Offen“ (alle offenen Punkte des Projekts), dahinter die
                // erste Zeile des Offen-Abschnitts der Übersicht, auch ohne Aufzählung
                let first = p.overview.as_deref().and_then(|o| self.doc(o)).and_then(|d| section_first_line(&d, SectionKind::Open));
                let line = match (p.open_points, first) {
                    (0, None) => "Offen   –".to_string(),
                    (n, None) => format!("Offen   {n}"),
                    (0, Some(f)) => format!("Offen   {f}"),
                    (n, Some(f)) => format!("Offen   {n} · {f}"),
                };
                put(buf, px, hr.y + 8, &truncate(&line, pw, g.ellipsis), pw, theme.text());
            }
        }
    }
}

/// Zahl der Wochen der Heatmap bei dieser Breite.
pub(crate) fn heat_weeks(body_width: u16) -> usize {
    let weeks = if body_width >= 110 { 26 } else { 16 };
    weeks.min((body_width as usize).saturating_sub(2 + 4) / 2).max(4)
}

/// Breite neben der Heatmap, die für die Legende bleibt; unter 26 steht sie stattdessen darunter.
pub(crate) fn heat_side_width(body_width: u16) -> usize {
    let inner = body_width.saturating_sub(2) as usize;
    let right = 1 + 3 + heat_weeks(body_width) * 2;
    (1 + inner).saturating_sub(right + 3)
}

/// Erklärung der Ziffer „+“ in den Befundspalten.
const PLUS_LEGEND: &str = "+ = 10 und mehr";

/// Legende der Befundspalten in einer Zeile: lange Fassung, wenn sie passt, sonst kurz.
fn draw_legend_row(buf: &mut Buffer, x: u16, y: u16, w: usize, theme: &crate::theme::Theme) {
    let legend = finding_legend();
    let short = ["veraltet", "groß", "tote", "Waise"];
    let full_w: usize = legend.iter().map(|(_, d)| 2 + crate::text::width(d)).sum::<usize>() + 3 * 3 + 3 + crate::text::width("+ ab 10");
    let use_full = full_w <= w;
    let mut cx = x;
    for (i, (k, d)) in legend.iter().enumerate() {
        if i > 0 {
            cx = put(buf, cx, y, " · ", 3, theme.dim());
        }
        let d = if use_full { *d } else { short[i] };
        cx = put(buf, cx, y, k, 1, theme.finding(i));
        cx = put(buf, cx + 1, y, d, (x as usize + w).saturating_sub(cx as usize + 1), theme.dim());
    }
    // „+“ ab 10 erklären, solange Platz bleibt
    let rest = (x as usize + w).saturating_sub(cx as usize);
    if rest >= 3 + crate::text::width("+ ab 10") {
        cx = put(buf, cx, y, " · ", 3, theme.dim());
        put(buf, cx, y, "+ ab 10", 7, theme.dim());
    }
}
