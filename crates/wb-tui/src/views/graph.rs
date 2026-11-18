//! Ansicht 4: Graph. Frage: Wie hängt das zusammen?
//!
//! Kein freies Layout, sondern Sammelschienen: Spalten nach Abstand vom Fokus
//! (Eingehend | Fokus | Ausgehend), Kanten als Linienzeichen in den Zwischenräumen.
//! Das Ergebnis ist deterministisch und jede Beschriftung hat ihre eigene Zeile.

use crate::app::*;
use crate::model::*;
use crate::text::{put, truncate, width};
use crate::ui::*;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use std::collections::HashSet;

const GUTTER: u16 = 5;

#[derive(Clone, Debug)]
pub(crate) struct GNode {
    pub id: String,
    pub label: String,
    pub tag: String,
    pub mark: char,
    /// Notiz mit `status: superseded`: abgesetzt dargestellt.
    pub old: bool,
    pub fresh: Freshness,
    pub col: i32,
    pub row: usize,
    pub parent: Option<usize>,
    pub more: usize,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Ego {
    pub nodes: Vec<GNode>,
    /// (Spalte, Zeile, Anzahl) für „+N weitere“
    pub overflow: Vec<(i32, usize, usize)>,
    pub rows: usize,
    pub left: i32,
    pub right: i32,
}

struct Nbr {
    id: String,
    label: String,
    tag: String,
    mark: char,
    old: bool,
    fresh: Freshness,
}

impl App<'_> {
    // ---------- Nachbarschaft ----------

    fn nbrs(&self, level: Level, id: &str, outgoing: bool, focus_project: Option<&str>) -> Vec<Nbr> {
        match level {
            Level::Notes => {
                let l = self.links_of(id);
                let src = if outgoing { &l.out } else { &l.inc };
                let mut v: Vec<&Link> = src.iter().collect();
                v.sort_by(|a, b| b.modified.cmp(&a.modified).then_with(|| a.title.cmp(&b.title)));
                v.into_iter()
                    .map(|k| Nbr {
                        id: k.path.clone(),
                        label: k.title.clone(),
                        tag: match (&k.project, focus_project) {
                            (Some(p), Some(f)) if p != f => p.clone(),
                            (None, _) => String::new(),
                            _ => String::new(),
                        },
                        mark: k.ntype.mark(),
                        old: k.status == Some(Status::Superseded),
                        fresh: Freshness::of(k.modified, self.today),
                    })
                    .collect()
            }
            Level::Projects => {
                if !outgoing {
                    return Vec::new();
                }
                let mut v: Vec<(&ProjectEdge, &str)> = self
                    .edges
                    .iter()
                    .filter_map(|e| {
                        if e.a == id {
                            Some((e, e.b.as_str()))
                        } else if e.b == id {
                            Some((e, e.a.as_str()))
                        } else {
                            None
                        }
                    })
                    .collect();
                v.sort_by(|a, b| b.0.weight().cmp(&a.0.weight()).then_with(|| a.1.cmp(b.1)));
                v.into_iter()
                    .map(|(e, other)| Nbr {
                        id: other.to_string(),
                        label: other.to_string(),
                        tag: match (e.links, e.shared_sources) {
                            (l, 0) => format!("{l}L"),
                            (0, s) => format!("{s}Q"),
                            (l, s) => format!("{l}L {s}Q"),
                        },
                        mark: ' ',
                        old: self.project(other).is_some_and(|p| p.status == Some(Status::Superseded)),
                        fresh: Freshness::of(self.project(other).and_then(|p| p.last_change), self.today),
                    })
                    .collect()
            }
        }
    }

    fn focus_project(&self, level: Level, focus: &str) -> Option<String> {
        match level {
            Level::Notes => self.doc(focus).and_then(|d| d.info.project.clone()),
            Level::Projects => Some(focus.to_string()),
        }
    }

    /// Eine Seite des Baums (ausgehend oder eingehend) mit Zeilenvergabe.
    fn ego_side(&self, level: Level, focus: &str, outgoing: bool, depth: u8, h: usize, taken: &mut HashSet<String>) -> (Vec<GNode>, Option<(i32, usize, usize)>, usize) {
        let sign: i32 = if outgoing { 1 } else { -1 };
        let fp = self.focus_project(level, focus);
        let n1 = self.nbrs(level, focus, outgoing, fp.as_deref());
        let n1: Vec<Nbr> = n1.into_iter().filter(|n| !taken.contains(&n.id)).collect();
        if n1.is_empty() || h == 0 {
            return (Vec::new(), None, 0);
        }
        let total1 = n1.len();
        let c1 = if depth >= 2 { total1.min((h / 3).max(1)) } else if total1 > h { h - 1 } else { total1 };
        let over1 = total1 - c1;
        let mut nodes: Vec<GNode> = Vec::new();
        let mut row = 0usize;
        let per_group = if depth >= 2 && c1 > 0 {
            ((h.saturating_sub(c1 - 1 + usize::from(over1 > 0))) / c1).max(1)
        } else {
            1
        };
        let mut it = n1.into_iter();
        let first: Vec<Nbr> = it.by_ref().take(c1).collect();
        for n in &first {
            taken.insert(n.id.clone());
        }
        let mut last_had_kids = false;
        for n in first {
            let mut kids: Vec<Nbr> = Vec::new();
            let mut hidden = 0usize;
            if depth >= 2 {
                let all: Vec<Nbr> = self
                    .nbrs(level, &n.id, outgoing, fp.as_deref())
                    .into_iter()
                    .filter(|k| k.id != focus && !taken.contains(&k.id))
                    .collect();
                hidden = all.len().saturating_sub(per_group);
                kids = all.into_iter().take(per_group).collect();
            }
            let m = kids.len();
            let parent_idx = nodes.len();
            let prow = if m > 0 { row + (m - 1) / 2 } else { row };
            nodes.push(GNode { id: n.id, label: n.label, tag: n.tag, mark: n.mark, old: n.old, fresh: n.fresh, col: sign, row: prow, parent: None, more: hidden });
            for (j, k) in kids.into_iter().enumerate() {
                taken.insert(k.id.clone());
                nodes.push(GNode {
                    id: k.id,
                    label: k.label,
                    tag: k.tag,
                    mark: k.mark,
                    old: k.old,
                    fresh: k.fresh,
                    col: 2 * sign,
                    row: row + j,
                    parent: Some(parent_idx),
                    more: 0,
                });
            }
            last_had_kids = m > 0;
            row += if m > 0 { m + 1 } else { 1 };
        }
        let mut rows = row;
        if last_had_kids {
            rows = rows.saturating_sub(1);
        }
        let mut overflow = None;
        if over1 > 0 {
            overflow = Some((sign, rows, over1));
            rows += 1;
        }
        (nodes, overflow, rows)
    }

    pub(crate) fn build_ego(&self, level: Level, focus: &str, depth: u8, h: usize) -> Ego {
        let mut taken: HashSet<String> = HashSet::new();
        taken.insert(focus.to_string());
        let mut ego = Ego::default();
        let (mut right, ro, rrows) = self.ego_side(level, focus, true, depth, h, &mut taken);
        let (mut left, lo, lrows) = if level == Level::Notes {
            // eingehende Nachbarn, die schon rechts stehen, dürfen links trotzdem erscheinen
            let mut t2: HashSet<String> = HashSet::new();
            t2.insert(focus.to_string());
            self.ego_side(level, focus, false, depth, h, &mut t2)
        } else {
            (Vec::new(), None, 0)
        };
        let centre = |ns: &[GNode]| -> Option<usize> {
            let r: Vec<usize> = ns.iter().filter(|n| n.col.abs() == 1).map(|n| n.row).collect();
            if r.is_empty() {
                None
            } else {
                Some((r.iter().min().unwrap() + r.iter().max().unwrap()) / 2)
            }
        };
        let (cl, cr) = (centre(&left), centre(&right));
        let f = cl.unwrap_or(0).max(cr.unwrap_or(0));
        let sl = cl.map(|c| f - c).unwrap_or(0);
        let sr = cr.map(|c| f - c).unwrap_or(0);
        let total = (lrows + sl).max(rrows + sr).max(f + 1);
        let (sl, sr, f, total) = if total > h.max(1) {
            // passt nicht: ohne Ausgleich, Fokus mittig der größeren Seite
            let f = cl.unwrap_or(0).min(cr.unwrap_or(usize::MAX)).min(h.saturating_sub(1) / 2);
            (0, 0, f, lrows.max(rrows).max(1))
        } else {
            (sl, sr, f, total)
        };
        for n in left.iter_mut() {
            n.row += sl;
        }
        for n in right.iter_mut() {
            n.row += sr;
        }
        ego.nodes.push(GNode {
            id: focus.to_string(),
            label: String::new(),
            tag: String::new(),
            mark: ' ',
            old: false,
            fresh: Freshness::Unknown,
            col: 0,
            row: f,
            parent: None,
            more: 0,
        });
        let base = ego.nodes.len();
        // Elternindizes umrechnen
        for (i, mut n) in right.into_iter().enumerate() {
            n.parent = n.parent.map(|p| p + base);
            let _ = i;
            ego.nodes.push(n);
        }
        let base2 = ego.nodes.len();
        for mut n in left {
            n.parent = n.parent.map(|p| p + base2);
            ego.nodes.push(n);
        }
        if let Some((c, r, n)) = ro {
            ego.overflow.push((c, r + sr, n));
        }
        if let Some((c, r, n)) = lo {
            ego.overflow.push((c, r + sl, n));
        }
        ego.left = ego.nodes.iter().map(|n| (-n.col).max(0)).max().unwrap_or(0);
        ego.right = ego.nodes.iter().map(|n| n.col.max(0)).max().unwrap_or(0);
        ego.rows = total.max(ego.nodes.iter().map(|n| n.row + 1).max().unwrap_or(1));
        // Beschriftung des Fokus
        let label = match level {
            Level::Notes => self.doc(focus).map(|d| d.info.title.clone()).unwrap_or_else(|| focus.to_string()),
            Level::Projects => focus.to_string(),
        };
        ego.nodes[0].label = label;
        ego.nodes[0].fresh = match level {
            Level::Notes => Freshness::of(self.doc(focus).and_then(|d| d.info.modified), self.today),
            Level::Projects => Freshness::of(self.project(focus).and_then(|p| p.last_change), self.today),
        };
        ego
    }

    // ---------- Zeichnen ----------

    /// Zeichnet die Nachbarschaft in `r` (Spaltenköpfe in der ersten Zeile).
    pub(crate) fn draw_ego(&self, buf: &mut Buffer, r: Rect, level: Level, focus: &str, depth: u8, sel: Option<&str>) {
        if r.height < 3 || r.width < 20 {
            return;
        }
        let theme = self.theme;
        let g = theme.glyphs();
        let mut depth = depth.clamp(1, 2);
        let ego = loop {
            let ego = self.build_ego(level, focus, depth, (r.height as usize).saturating_sub(1));
            let cols = (ego.left + ego.right + 1) as u16;
            let gutters = (ego.left + ego.right) as u16 * GUTTER;
            let colw = (r.width.saturating_sub(gutters)) / cols.max(1);
            if colw >= 14 || depth == 1 {
                break ego;
            }
            depth = 1;
        };
        let cols = (ego.left + ego.right + 1) as u16;
        let gutters = (ego.left + ego.right) as u16 * GUTTER;
        let colw = (r.width.saturating_sub(gutters) / cols.max(1)).max(8);
        let x_of = |col: i32| -> u16 { r.x + ((col + ego.left) as u16) * (colw + GUTTER) };
        let y0 = r.y + 1;

        // Spaltenköpfe
        let heads = |col: i32| -> &str {
            match (level, col) {
                (_, 0) => "Fokus",
                (Level::Notes, c) if c < 0 => "Eingehend",
                (Level::Notes, _) => "Ausgehend",
                (Level::Projects, _) => "Verbunden",
            }
        };
        for col in -ego.left..=ego.right {
            // zweite Stufe kurz: „Ausgehend ·2“ statt einer abgeschnittenen Erklärung
            let t = if col.abs() == 2 { format!("{} ·2", heads(col)) } else { heads(col).to_string() };
            put(buf, x_of(col), r.y, &truncate(&t, colw as usize, g.ellipsis), colw as usize, theme.dim());
        }

        // Kanten in ein Maskenraster
        let gw = (ego.left + ego.right) as usize * GUTTER as usize;
        let mut mask = vec![0u8; gw * r.height as usize];
        let mut arrow = vec![false; gw * r.height as usize];
        let gx = |gcol: i32| -> usize {
            // Zwischenraum zwischen Spalte gcol und gcol+1, als Index im Raster
            ((gcol + ego.left) as usize) * GUTTER as usize
        };
        // Gruppen: je Elternknoten (Richtung zum Fokus) mit Kindern
        let mut set = |gcol: i32, row: usize, k: usize, bits: u8| {
            let i = row * gw + gx(gcol) + k;
            if row < r.height as usize && i < mask.len() {
                mask[i] |= bits;
            }
        };
        let (u, d, l, rr) = (1u8, 2u8, 4u8, 8u8);
        // Rechte Seite: Eltern links der Schiene
        for side in [1i32, -1i32] {
            let parents: Vec<usize> = (0..ego.nodes.len()).filter(|&i| i == 0 || ego.nodes[i].col.signum() == side).collect();
            for p in parents {
                let kids: Vec<usize> = (0..ego.nodes.len())
                    .filter(|&i| ego.nodes[i].col.signum() == side && ego.nodes[i].col.abs() == ego.nodes[p].col.abs() + 1 && (ego.nodes[i].parent == Some(p) || (p == 0 && ego.nodes[i].parent.is_none())))
                    .collect();
                if kids.is_empty() {
                    continue;
                }
                let prow = ego.nodes[p].row;
                let rows: Vec<usize> = kids.iter().map(|&k| ego.nodes[k].row).collect();
                let lo = rows.iter().copied().chain([prow]).min().unwrap();
                let hi = rows.iter().copied().chain([prow]).max().unwrap();
                if side == 1 {
                    // Gutter zwischen Spalte c und c+1, c = col(p)
                    let gc = ego.nodes[p].col;
                    set(gc, prow, 0, l | rr);
                    set(gc, prow, 1, l | rr);
                    set(gc, prow, 2, l);
                    for &kr in &rows {
                        set(gc, kr, 2, rr);
                        set(gc, kr, 3, l | rr);
                        let i = kr * gw + gx(gc) + 4;
                        if i < arrow.len() {
                            arrow[i] = true;
                        }
                    }
                    for y in lo..=hi {
                        set(gc, y, 2, (if y > lo { u } else { 0 }) | (if y < hi { d } else { 0 }));
                    }
                } else {
                    // Gutter zwischen Spalte -c-1 und -c; Kind links, Elternteil rechts
                    let gc = ego.nodes[p].col - 1;
                    for &kr in &rows {
                        set(gc, kr, 0, l | rr);
                        set(gc, kr, 1, l | rr);
                        set(gc, kr, 2, l);
                    }
                    set(gc, prow, 2, rr);
                    set(gc, prow, 3, l | rr);
                    let i = prow * gw + gx(gc) + 4;
                    if i < arrow.len() {
                        arrow[i] = true;
                    }
                    for y in lo..=hi {
                        set(gc, y, 2, (if y > lo { u } else { 0 }) | (if y < hi { d } else { 0 }));
                    }
                }
            }
        }
        for row in 0..r.height as usize {
            for k in 0..gw {
                let i = row * gw + k;
                if mask[i] != 0 {
                    let ch = if arrow[i] { g.arrow } else { g.line(mask[i]) };
                    put(buf, r.x + k as u16 + (k as u16 / GUTTER + 1) * colw, y0 + row as u16, &ch.to_string(), 1, theme.dim());
                }
            }
        }

        // Knoten
        for n in &ego.nodes {
            let y = y0 + n.row as u16;
            if y >= r.y + r.height {
                continue;
            }
            let x = x_of(n.col);
            let is_sel = sel.map(|s| s == n.id).unwrap_or(false) || (sel.is_none() && n.col == 0 && false);
            // ein Zeichen Abstand zwischen Beschriftung und Verbindungslinie
            let w = (colw as usize).saturating_sub(1).max(1);
            if n.col == 0 {
                crate::text::fill(buf, Rect::new(x, y, colw, 1), theme.tab_active());
                let t = format!("{} {}", g.freshness(n.fresh), n.label);
                put(buf, x, y, &truncate(&t, w, g.ellipsis), w, theme.tab_active());
                continue;
            }
            if is_sel {
                crate::text::fill(buf, Rect::new(x, y, colw, 1), theme.selected());
            }
            let mark_ch = if n.old { g.superseded } else { n.mark };
            let mark = if mark_ch == ' ' { String::new() } else { format!("{mark_ch} ") };
            let more = if n.more > 0 { format!(" +{}", n.more) } else { String::new() };
            let tag = if n.tag.is_empty() { String::new() } else { format!(" {}", n.tag) };
            let fixed = 2 + width(&mark) + width(&more);
            let title_w = w.saturating_sub(fixed).max(4);
            let title = truncate(&n.label, title_w, g.ellipsis);
            let mut cx = put(buf, x, y, &g.freshness(n.fresh).to_string(), 1, theme.fresh(n.fresh));
            cx = put(buf, cx + 1, y, &mark, w.saturating_sub((cx - x) as usize + 1), theme.accent());
            let tstyle = if n.old {
                theme.dim()
            } else if is_sel {
                theme.title()
            } else {
                theme.text()
            };
            cx = put(buf, cx, y, &title, w.saturating_sub((cx - x) as usize), tstyle);
            let rest = w.saturating_sub((cx - x) as usize);
            if !tag.is_empty() && rest > 3 {
                cx = put(buf, cx, y, &truncate(&tag, rest, g.ellipsis), rest, theme.dim());
            }
            let rest = w.saturating_sub((cx - x) as usize);
            if !more.is_empty() && rest >= width(&more) {
                put(buf, cx, y, &more, rest, theme.dim());
            }
        }
        for (col, row, n) in &ego.overflow {
            let y = y0 + *row as u16;
            if y < r.y + r.height {
                put(buf, x_of(*col), y, &truncate(&format!("… +{n} weitere"), colw as usize, g.ellipsis), colw as usize, theme.dim());
            }
        }
    }

    pub(crate) fn draw_ego_notes(&self, buf: &mut Buffer, r: Rect, path: &str, depth: u8, sel: Option<&str>) {
        self.draw_ego(buf, r, Level::Notes, path, depth, sel);
    }

    // ---------- Ansicht ----------

    fn graph_focus(&self) -> String {
        self.gr.focus.clone().unwrap_or_default()
    }

    /// Rechteck des Graphen innerhalb des Rumpfes (gleiche Rechnung für Zeichnen und Tasten).
    fn graph_rect(&self, body: Rect) -> Rect {
        let top = 2;
        let side = if self.gr.level == Level::Projects && body.width >= 100 { 34 } else { 0 };
        let bottom = if self.gr.level == Level::Projects { 2 } else { 0 };
        Rect::new(
            body.x + 1 + side,
            body.y + top,
            body.width.saturating_sub(2 + side),
            body.height.saturating_sub(top + bottom),
        )
    }

    pub(crate) fn graph_headline(&self) -> String {
        let focus = self.graph_focus();
        match self.gr.level {
            Level::Notes => match self.doc(&focus) {
                Some(d) => {
                    let l = self.links_of(&focus);
                    format!("{} · {} eingehend · {} ausgehend", d.info.title, l.inc.len(), l.out.len())
                }
                None => "Keine Notiz im Fokus".to_string(),
            },
            Level::Projects => {
                let connected: HashSet<&str> = self.edges.iter().flat_map(|e| [e.a.as_str(), e.b.as_str()]).collect();
                let k = self.edges.iter().filter(|e| e.a == focus || e.b == focus).count();
                if focus.is_empty() {
                    return format!("{} von {} Projekten sind verbunden", connected.len(), self.projects.len());
                }
                format!(
                    "{} von {} Projekten sind verbunden · {focus} hat {k} Nachbarn",
                    connected.len(),
                    self.projects.len()
                )
            }
        }
    }

    pub(crate) fn graph_hints(&self) -> Vec<(&'static str, &'static str)> {
        let mut v = vec![("←↑↓→", "wählen"), ("o", "lesen"), ("d", "Tiefe"), ("p", "Notizen/Projekte")];
        // Enter wirkt nur auf einen gewählten Knoten (am Fokus selbst tut es nichts)
        if self.gr.sel.is_some() {
            v.insert(1, ("Enter", "Fokus setzen"));
        }
        if self.gr.level == Level::Projects {
            v.insert(v.len() - 3, ("Tab", "nächstes"));
        }
        v.push(("Esc", "zurück"));
        v
    }

    fn graph_selected(&self, ego: &Ego) -> usize {
        let id = self.gr.sel.as_deref().unwrap_or("");
        ego.nodes.iter().position(|n| n.id == id && n.col != 0).unwrap_or(0)
    }

    pub(crate) fn graph_key(&mut self, key: KeyEvent) {
        let focus = self.graph_focus();
        let rect = self.graph_rect(self.body_rect());
        let ego = self.build_ego(self.gr.level, &focus, self.gr.depth, (rect.height as usize).saturating_sub(1));
        let cur = self.graph_selected(&ego);
        let cur_node = &ego.nodes[cur];
        let pick = |app: &mut Self, idx: usize| {
            if idx != 0 {
                app.gr.sel = Some(ego.nodes[idx].id.clone());
            } else {
                app.gr.sel = None;
            }
        };
        match key.code {
            KeyCode::Down | KeyCode::Char('j') | KeyCode::Up | KeyCode::Char('k') => {
                let down = matches!(key.code, KeyCode::Down | KeyCode::Char('j'));
                let col = cur_node.col;
                let mut same: Vec<usize> = (0..ego.nodes.len()).filter(|&i| ego.nodes[i].col == col).collect();
                same.sort_by_key(|&i| ego.nodes[i].row);
                if let Some(p) = same.iter().position(|&i| i == cur) {
                    let np = if down { (p + 1).min(same.len() - 1) } else { p.saturating_sub(1) };
                    pick(self, same[np]);
                }
            }
            KeyCode::Left | KeyCode::Char('h') | KeyCode::Right | KeyCode::Char('l') => {
                let right = matches!(key.code, KeyCode::Right | KeyCode::Char('l'));
                let target_col = cur_node.col + if right { 1 } else { -1 };
                let row = cur_node.row as i64;
                let best = (0..ego.nodes.len())
                    .filter(|&i| ego.nodes[i].col == target_col)
                    .min_by_key(|&i| (ego.nodes[i].row as i64 - row).abs());
                if let Some(b) = best {
                    pick(self, b);
                }
            }
            KeyCode::Enter => {
                if cur != 0 {
                    let id = ego.nodes[cur].id.clone();
                    let level = self.gr.level;
                    let depth = self.gr.depth;
                    self.go(Loc::Graph { level, focus: id, depth });
                }
            }
            KeyCode::Char('o') => {
                if self.gr.level == Level::Notes {
                    let id = ego.nodes[cur].id.clone();
                    self.open_reader(&id);
                } else {
                    let id = ego.nodes[cur].id.clone();
                    self.open_project(&id);
                }
            }
            KeyCode::Char('d') => {
                self.gr.depth = if self.gr.depth >= 2 { 1 } else { 2 };
                self.gr.sel = None;
            }
            KeyCode::Char('p') => {
                let (level, f) = match self.gr.level {
                    Level::Notes => {
                        let proj = self.doc(&focus).and_then(|d| d.info.project.clone()).unwrap_or_default();
                        (Level::Projects, proj)
                    }
                    Level::Projects => {
                        let ov = self.project(&focus).and_then(|p| p.overview.clone()).unwrap_or_default();
                        (Level::Notes, ov)
                    }
                };
                self.go(Loc::Graph { level, focus: f, depth: self.gr.depth });
            }
            KeyCode::Tab | KeyCode::BackTab => {
                if self.gr.level == Level::Projects {
                    let rank = self.project_ranking();
                    if !rank.is_empty() {
                        let at = rank.iter().position(|(n, _)| *n == focus).unwrap_or(rank.len() - 1);
                        let next = if key.code == KeyCode::Tab { (at + 1) % rank.len() } else { (at + rank.len() - 1) % rank.len() };
                        self.gr.focus = Some(rank[next].0.clone());
                        self.gr.sel = None;
                    }
                }
            }
            KeyCode::Esc | KeyCode::Backspace => self.go_back(),
            _ => {}
        }
    }

    /// Verbundene Projekte nach Gesamtgewicht, absteigend.
    pub(crate) fn project_ranking(&self) -> Vec<(String, u32)> {
        let mut w: std::collections::BTreeMap<&str, u32> = std::collections::BTreeMap::new();
        for e in &self.edges {
            *w.entry(&e.a).or_default() += e.weight();
            *w.entry(&e.b).or_default() += e.weight();
        }
        let mut v: Vec<(String, u32)> = w.into_iter().map(|(k, v)| (k.to_string(), v)).collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        v
    }

    pub(crate) fn graph_draw(&self, body: Rect, buf: &mut Buffer) {
        let theme = self.theme;
        let g = theme.glyphs();
        let focus = self.graph_focus();
        let rect = self.graph_rect(body);
        // Kopfzeile des Graphen
        let level_name = match self.gr.level {
            Level::Notes => "Notizen",
            Level::Projects => "Projekte",
        };
        let t = format!("Ebene {level_name} · Tiefe {} · d Tiefe · p Ebene", self.gr.depth);
        put(buf, body.x + 1, body.y, &truncate(&t, body.width as usize - 2, g.ellipsis), body.width as usize - 2, theme.dim());

        let sel_id = self.gr.sel.clone();
        self.draw_ego(buf, Rect::new(rect.x, rect.y, rect.width, rect.height), self.gr.level, &focus, self.gr.depth, sel_id.as_deref());

        if self.gr.level == Level::Projects {
            // Rangliste links (breit)
            let rank = self.project_ranking();
            if body.width >= 100 {
                let lw = 32usize;
                put(buf, body.x + 1, body.y + 2, "Verbindungsstärke", lw, theme.title());
                let max = rank.first().map(|r| r.1).unwrap_or(1).max(1);
                let rows = (body.height as usize).saturating_sub(5);
                let pos = rank.iter().position(|(n, _)| *n == focus).unwrap_or(0);
                let off = scroll_offset(pos, rank.len(), rows);
                for (i, (name, wgt)) in rank.iter().enumerate().skip(off).take(rows) {
                    let y = body.y + 3 + (i - off) as u16;
                    let sel = *name == focus;
                    if sel {
                        crate::text::fill(buf, Rect::new(body.x + 1, y, lw as u16 + 1, 1), theme.selected());
                    }
                    let mut rw = RowWriter::new(buf, body.x + 1, y, lw as u16 + 1);
                    rw.cell(name, 17, Align::Left, if sel { theme.title() } else { theme.text() }, g.ellipsis);
                    rw.gap();
                    rw.cell(&size_bar(*wgt as f64 / max as f64, 9, &theme), 9, Align::Left, theme.bar(), g.ellipsis);
                    rw.gap();
                    rw.cell(&wgt.to_string(), 3, Align::Right, theme.dim(), g.ellipsis);
                }
                for y in body.y + 2..body.y + body.height - 2 {
                    put(buf, body.x + 1 + lw as u16 + 1, y, &g.sep.to_string(), 1, theme.dim());
                }
            }
            // Inseln unten
            let connected: HashSet<&str> = rank.iter().map(|(n, _)| n.as_str()).collect();
            let mut islands: Vec<&str> = self.projects.iter().map(|p| p.name.as_str()).filter(|n| !connected.contains(n)).collect();
            islands.sort();
            let y = body.y + body.height - 2;
            let list = islands.join(", ");
            let head = format!("Ohne Verbindung ({}): ", islands.len());
            put(buf, body.x + 1, y, &head, head.chars().count(), theme.title());
            let hw = head.chars().count();
            let rest = body.width as usize - 2;
            let room = rest.saturating_sub(hw);
            let text = if list.is_empty() { "keine".to_string() } else { list };
            let lines = crate::text::wrap(&[(text, theme.dim())], room.max(10), 0, 0);
            if let Some(l0) = lines.first() {
                crate::text::put_spans(buf, body.x + 1 + hw as u16, y, l0, room);
            }
            if let Some(l1) = lines.get(1) {
                let mut l1 = l1.clone();
                if lines.len() > 2 {
                    l1.push(("…".to_string(), theme.dim()));
                }
                crate::text::put_spans(buf, body.x + 1, y + 1, &l1, rest);
            }
        }
    }
}
