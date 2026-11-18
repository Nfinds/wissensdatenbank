//! Oberflächenzustand, Navigation und Rahmen (Kopf, Fuß, Hilfe). Die Ansichten selbst
//! liegen in `views/`.

use crate::model::*;
use crate::source::DataSource;
use crate::text::{put, truncate, width};
use crate::theme::Theme;
use crate::ui;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::Style;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

/// Strg ohne Alt: AltGr (für @ { } [ ] ~ | und das Eurozeichen) erreicht Windows als Strg+Alt und ist ein Zeichen.
pub(crate) fn is_ctrl(key: &KeyEvent) -> bool {
    key.modifiers.contains(KeyModifiers::CONTROL) && !key.modifiers.contains(KeyModifiers::ALT)
}

pub const MIN_W: u16 = 60;
pub const MIN_H: u16 = 16;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum View {
    Overview,
    Project,
    Search,
    Reader,
    Graph,
}

impl View {
    pub fn all() -> [View; 4] {
        [View::Overview, View::Project, View::Search, View::Graph]
    }
    pub fn name(self) -> &'static str {
        match self {
            View::Overview => "overview",
            View::Project => "project",
            View::Search => "search",
            View::Reader => "reader",
            View::Graph => "graph",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Level {
    Notes,
    Projects,
}

#[derive(Clone, Debug)]
pub enum Loc {
    Overview,
    Project(String),
    Search,
    Reader { path: String, scroll: usize },
    Graph { level: Level, focus: String, depth: u8 },
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OvSort {
    Activity,
    Size,
    Age,
    Name,
    Findings,
}

impl OvSort {
    pub fn next(self) -> OvSort {
        match self {
            OvSort::Activity => OvSort::Size,
            OvSort::Size => OvSort::Age,
            OvSort::Age => OvSort::Name,
            OvSort::Name => OvSort::Findings,
            OvSort::Findings => OvSort::Activity,
        }
    }
}

pub struct OverviewState {
    pub sel: Option<String>,
    pub sort: OvSort,
    pub reverse: bool,
    pub hide_inactive: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NoteSort {
    Recent,
    Size,
    Name,
    Findings,
}

impl NoteSort {
    pub fn next(self) -> NoteSort {
        match self {
            NoteSort::Recent => NoteSort::Size,
            NoteSort::Size => NoteSort::Name,
            NoteSort::Name => NoteSort::Findings,
            NoteSort::Findings => NoteSort::Recent,
        }
    }
}

pub struct ProjectState {
    pub name: Option<String>,
    pub notes: Vec<NoteInfo>,
    pub sort: NoteSort,
    pub sel: Option<String>,
}

pub struct SearchState {
    pub query: String,
    /// 0 alle, 1 Themen, 2 Quellen
    pub filter: u8,
    pub hits: Vec<SearchHit>,
    pub sel: usize,
    pub total_unfiltered: usize,
}

pub struct ReaderState {
    pub path: Option<String>,
    pub scroll: usize,
    pub focus_links: bool,
    pub link_sel: usize,
}

pub struct GraphState {
    pub level: Level,
    pub focus: Option<String>,
    pub depth: u8,
    pub sel: Option<String>,
}

pub struct App<'a> {
    pub(crate) src: &'a dyn DataSource,
    pub theme: Theme,
    pub(crate) today: Date,
    pub(crate) view: View,
    pub(crate) back: Vec<Loc>,
    pub(crate) area: Cell<Rect>,
    pub(crate) projects: Vec<ProjectInfo>,
    pub(crate) edges: Vec<ProjectEdge>,
    pub(crate) series: Vec<u16>,
    pub(crate) ov: OverviewState,
    pub(crate) pm: ProjectState,
    pub(crate) se: SearchState,
    pub(crate) rd: ReaderState,
    pub(crate) gr: GraphState,
    pub(crate) docs: RefCell<HashMap<String, Option<Rc<NoteDoc>>>>,
    pub(crate) links: RefCell<HashMap<String, Rc<NoteLinks>>>,
    pub(crate) help: bool,
    /// Erste sichtbare Zeile der Hilfe (die Hilfe scrollt, damit sie auch bei 60×16 ganz erreichbar ist).
    pub(crate) help_scroll: Cell<usize>,
    pub quit: bool,
    /// Gesetzt, wenn der Nutzer `e` drückt; der Terminal-Treiber startet den Editor.
    pub pending_editor: Option<String>,
    pub(crate) message: Option<String>,
    /// Statische Ausgabe: keine Auswahlmarkierung, kein Rollen.
    pub(crate) plain: bool,
}

/// Fenster der Heatmap im Datenabruf: reicht für 26 Wochen plus angebrochene Woche.
pub const SERIES_DAYS: u32 = 26 * 7 + 6;

impl<'a> App<'a> {
    pub fn new(src: &'a dyn DataSource, theme: Theme) -> App<'a> {
        let mut app = App {
            src,
            theme,
            today: src.today(),
            view: View::Overview,
            back: Vec::new(),
            area: Cell::new(Rect::new(0, 0, 80, 24)),
            projects: Vec::new(),
            edges: Vec::new(),
            series: Vec::new(),
            ov: OverviewState { sel: None, sort: OvSort::Activity, reverse: false, hide_inactive: false },
            pm: ProjectState { name: None, notes: Vec::new(), sort: NoteSort::Recent, sel: None },
            se: SearchState { query: String::new(), filter: 0, hits: Vec::new(), sel: 0, total_unfiltered: 0 },
            rd: ReaderState { path: None, scroll: 0, focus_links: false, link_sel: 0 },
            gr: GraphState { level: Level::Notes, focus: None, depth: 1, sel: None },
            docs: RefCell::new(HashMap::new()),
            links: RefCell::new(HashMap::new()),
            help: false,
            help_scroll: Cell::new(0),
            quit: false,
            pending_editor: None,
            message: None,
            plain: false,
        };
        app.reload();
        app
    }

    /// Quelle mit der Außenwelt abgleichen (Index nachziehen, Zwischenspeicher verwerfen) und neu laden.
    /// Das tun `r` und die Rückkehr aus dem Editor; es ist auch im Nur-Lese-Modus erlaubt (§16.2).
    pub fn refresh(&mut self) {
        self.src.refresh();
        self.reload();
    }

    /// Daten neu aus der Quelle holen und die Zwischenspeicher der Oberfläche leeren.
    pub fn reload(&mut self) {
        self.today = self.src.today();
        self.projects = self.src.projects();
        self.edges = self.src.project_edges();
        self.series = self.src.activity(SERIES_DAYS);
        self.docs.borrow_mut().clear();
        self.links.borrow_mut().clear();
        if let Some(name) = self.pm.name.clone() {
            self.pm.notes = self.src.notes(&name);
        }
        let order = self.ov_order();
        if self.ov.sel.as_ref().map(|s| !self.projects.iter().any(|p| &p.name == s)).unwrap_or(true) {
            self.ov.sel = order.first().map(|&i| self.projects[i].name.clone());
        }
        self.refresh_search();
    }

    pub fn view(&self) -> View {
        self.view
    }

    // ---------- Daten mit Zwischenspeicher ----------

    pub(crate) fn doc(&self, path: &str) -> Option<Rc<NoteDoc>> {
        if let Some(d) = self.docs.borrow().get(path) {
            return d.clone();
        }
        let d = self.src.note(path).map(Rc::new);
        self.docs.borrow_mut().insert(path.to_string(), d.clone());
        d
    }

    pub(crate) fn links_of(&self, path: &str) -> Rc<NoteLinks> {
        if let Some(l) = self.links.borrow().get(path) {
            return l.clone();
        }
        let l = Rc::new(self.src.links(path));
        self.links.borrow_mut().insert(path.to_string(), l.clone());
        l
    }

    pub(crate) fn project(&self, name: &str) -> Option<&ProjectInfo> {
        self.projects.iter().find(|p| p.name == name)
    }

    // ---------- Navigation ----------

    pub(crate) fn here(&self) -> Loc {
        match self.view {
            View::Overview => Loc::Overview,
            View::Project => Loc::Project(self.pm.name.clone().unwrap_or_default()),
            View::Search => Loc::Search,
            View::Reader => Loc::Reader { path: self.rd.path.clone().unwrap_or_default(), scroll: self.rd.scroll },
            View::Graph => Loc::Graph {
                level: self.gr.level,
                focus: self.gr.focus.clone().unwrap_or_default(),
                depth: self.gr.depth,
            },
        }
    }

    fn push_here(&mut self) {
        let here = self.here();
        self.back.push(here);
        if self.back.len() > 64 {
            self.back.remove(0);
        }
    }

    pub(crate) fn apply(&mut self, loc: Loc) {
        match loc {
            Loc::Overview => self.view = View::Overview,
            Loc::Project(name) => self.load_project(&name),
            Loc::Search => self.view = View::Search,
            Loc::Reader { path, scroll } => {
                self.load_reader(&path);
                self.rd.scroll = scroll;
            }
            Loc::Graph { level, focus, depth } => {
                self.gr.level = level;
                self.gr.focus = Some(focus);
                self.gr.depth = depth;
                self.gr.sel = None;
                self.view = View::Graph;
            }
        }
    }

    /// Navigiert und merkt den Ausgangspunkt für `Esc`.
    pub(crate) fn go(&mut self, loc: Loc) {
        self.push_here();
        self.apply(loc);
    }

    pub(crate) fn go_back(&mut self) {
        if let Some(loc) = self.back.pop() {
            self.apply(loc);
        } else if self.view != View::Overview {
            self.view = View::Overview;
        }
    }

    pub fn open_project(&mut self, name: &str) {
        self.go(Loc::Project(name.to_string()));
    }

    pub fn open_reader(&mut self, path: &str) {
        self.go(Loc::Reader { path: path.to_string(), scroll: 0 });
    }

    pub fn open_graph(&mut self, level: Level, focus: &str, depth: u8) {
        self.go(Loc::Graph { level, focus: focus.to_string(), depth });
    }

    pub fn open_search(&mut self, query: &str) {
        if self.view != View::Search {
            self.push_here();
        }
        self.view = View::Search;
        self.se.query = query.to_string();
        self.se.sel = 0;
        self.refresh_search();
    }

    pub fn show(&mut self, v: View) {
        match v {
            View::Overview => self.go(Loc::Overview),
            View::Project => {
                let name = self.pm.name.clone().or_else(|| self.ov.sel.clone()).unwrap_or_default();
                self.go(Loc::Project(name));
            }
            View::Search => self.open_search(&self.se.query.clone()),
            View::Reader => {
                if let Some(p) = self.rd.path.clone() {
                    self.go(Loc::Reader { path: p, scroll: 0 });
                }
            }
            View::Graph => {
                let (level, focus) = self.default_graph_focus();
                self.go(Loc::Graph { level, focus, depth: self.gr.depth });
            }
        }
    }

    /// Fokus für die Graph-Ansicht je nach Herkunft.
    pub(crate) fn default_graph_focus(&self) -> (Level, String) {
        match self.view {
            View::Reader => (Level::Notes, self.rd.path.clone().unwrap_or_default()),
            View::Project => match self.pm.sel.clone().or_else(|| self.pm_order().first().map(|n| n.path.clone())) {
                Some(p) => (Level::Notes, p),
                None => (Level::Projects, self.ov.sel.clone().unwrap_or_default()),
            },
            View::Graph => (self.gr.level, self.gr.focus.clone().unwrap_or_default()),
            _ => (Level::Projects, self.ov.sel.clone().unwrap_or_default()),
        }
    }

    pub(crate) fn load_project(&mut self, name: &str) {
        self.pm.name = Some(name.to_string());
        self.pm.notes = self.src.notes(name);
        self.pm.sel = self.pm_order().first().map(|n| n.path.clone());
        self.view = View::Project;
    }

    pub(crate) fn load_reader(&mut self, path: &str) {
        self.rd.path = Some(path.to_string());
        self.rd.scroll = 0;
        self.rd.focus_links = false;
        self.rd.link_sel = 0;
        self.view = View::Reader;
    }

    // ---------- Tasten ----------

    /// Verarbeitet eine Taste. Loslass-Ereignisse (Windows) werden ignoriert.
    pub fn handle_key(&mut self, key: KeyEvent) {
        if key.kind == KeyEventKind::Release {
            return;
        }
        self.message = None;
        // Strg-C beendet in jedem Zustand, auch bei offener Hilfe und zu kleinem Fenster. AltGr
        // kommt unter Windows als Strg+Alt an und ist kein Strg.
        if is_ctrl(&key) && matches!(key.code, KeyCode::Char('c') | KeyCode::Char('C')) {
            self.quit = true;
            return;
        }
        let typing = self.view == View::Search;
        let a = self.area.get();
        if a.width < MIN_W || a.height < MIN_H {
            // Unter der Mindestgröße sieht man nichts: nur Ende bleibt (dieselbe Grenze wie in `render`)
            if !typing && key.code == KeyCode::Char('q') {
                self.quit = true;
            }
            return;
        }
        if self.help {
            let max = self.help_max_scroll();
            let sc = self.help_scroll.get();
            let page = (self.help_rows()).saturating_sub(1).max(1);
            match key.code {
                KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('q') | KeyCode::Enter | KeyCode::F(5) => self.help = false,
                KeyCode::Down | KeyCode::Char('j') => self.help_scroll.set((sc + 1).min(max)),
                KeyCode::Up | KeyCode::Char('k') => self.help_scroll.set(sc.saturating_sub(1)),
                KeyCode::PageDown | KeyCode::Char(' ') => self.help_scroll.set((sc + page).min(max)),
                KeyCode::PageUp => self.help_scroll.set(sc.saturating_sub(page)),
                KeyCode::Home => self.help_scroll.set(0),
                KeyCode::End => self.help_scroll.set(max),
                _ => {}
            }
            return;
        }
        // Funktionstasten gelten überall, auch beim Tippen
        match key.code {
            KeyCode::F(1) => return self.show(View::Overview),
            KeyCode::F(2) => return self.show(View::Project),
            KeyCode::F(3) => return self.show(View::Search),
            KeyCode::F(4) => return self.show(View::Graph),
            KeyCode::F(5) => return self.open_help(),
            _ => {}
        }
        if !typing {
            match key.code {
                KeyCode::Char('q') => {
                    self.quit = true;
                    return;
                }
                KeyCode::Char('?') => return self.open_help(),
                KeyCode::Char('1') => return self.show(View::Overview),
                KeyCode::Char('2') => return self.show(View::Project),
                KeyCode::Char('3') => return self.show(View::Search),
                KeyCode::Char('4') => return self.show(View::Graph),
                KeyCode::Char('/') => return self.open_search(""),
                KeyCode::Char('r') => {
                    self.refresh();
                    self.message = Some("Daten neu geladen".into());
                    return;
                }
                KeyCode::Char('e') => {
                    if let Some(p) = self.current_note_path() {
                        self.pending_editor = Some(p);
                    }
                    return;
                }
                _ => {}
            }
        }
        match self.view {
            View::Overview => self.overview_key(key),
            View::Project => self.project_key(key),
            View::Search => self.search_key(key),
            View::Reader => self.reader_key(key),
            View::Graph => self.graph_key(key),
        }
    }

    fn open_help(&mut self) {
        self.help = true;
        self.help_scroll.set(0);
    }

    pub fn set_message(&mut self, m: String) {
        self.message = Some(m);
    }

    /// Notiz, auf die sich `e` bezieht.
    pub fn current_note_path(&self) -> Option<String> {
        match self.view {
            View::Reader => self.rd.path.clone(),
            View::Project => self.pm.sel.clone(),
            View::Search => self.se.hits.get(self.se.sel).map(|h| h.path.clone()),
            View::Graph => {
                if self.gr.level == Level::Notes {
                    self.gr.sel.clone().or_else(|| self.gr.focus.clone())
                } else {
                    self.project(self.gr.focus.as_deref().unwrap_or("")).and_then(|p| p.overview.clone())
                }
            }
            View::Overview => self
                .ov
                .sel
                .as_deref()
                .and_then(|n| self.project(n))
                .and_then(|p| p.overview.clone()),
        }
    }

    // ---------- Rahmen ----------

    pub fn render(&self, area: Rect, buf: &mut Buffer) {
        self.render_inner(area, buf);
        if self.theme.ascii {
            // §16.7: auch feste Texte und Notizinhalte, nicht nur die Glyphen der Oberfläche
            crate::text::asciify_buffer(buf, area);
        }
    }

    fn render_inner(&self, area: Rect, buf: &mut Buffer) {
        self.area.set(area);
        let theme = self.theme;
        if area.width < MIN_W || area.height < MIN_H {
            let msg = format!("Fenster zu klein: mindestens {MIN_W}×{MIN_H}, jetzt {}×{}", area.width, area.height);
            let lines = crate::text::wrap(&[(msg, theme.title())], area.width.max(1) as usize, 0, 0);
            for (i, l) in lines.iter().enumerate().take(area.height as usize) {
                crate::text::put_spans(buf, area.x, area.y + i as u16, l, area.width as usize);
            }
            return;
        }
        let g = theme.glyphs();
        // Zeile 0: Reiter; Zeile 1: Kernaussage
        let mut x = area.x + 1;
        x = put(buf, x, area.y, "Wissensbank", 11, theme.title());
        x += 2;
        let active = match self.view {
            View::Reader => View::Search,
            v => v,
        };
        for (i, (v, label)) in [
            (View::Overview, "Übersicht"),
            (View::Project, "Projekt"),
            (View::Search, "Suche"),
            (View::Graph, "Graph"),
        ]
        .iter()
        .enumerate()
        {
            let text = format!(" {} {} ", i + 1, label);
            let style = if *v == active { theme.tab_active() } else { theme.dim() };
            x = put(buf, x, area.y, &text, text.chars().count(), style);
            x += 1;
        }
        let label = self.src.label();
        let lw = width(&label) as u16;
        if x + lw + 2 < area.x + area.width {
            put(buf, area.x + area.width - lw - 1, area.y, &label, lw as usize, theme.dim());
        }
        let pos = self.position_label().unwrap_or_default();
        let pw = width(&pos) as u16;
        let hw = (area.width as usize).saturating_sub(2 + if pw > 0 { pw as usize + 2 } else { 0 });
        let headline = self.headline(hw);
        put(buf, area.x + 1, area.y + 1, &truncate(&headline, hw, g.ellipsis), hw, theme.title());
        if pw > 0 {
            put(buf, area.x + area.width - pw - 1, area.y + 1, &pos, pw as usize, theme.dim());
        }

        let body = Rect::new(area.x, area.y + 2, area.width, area.height - 3);
        match self.view {
            View::Overview => self.overview_draw(body, buf),
            View::Project => self.project_draw(body, buf),
            View::Search => self.search_draw(body, buf),
            View::Reader => self.reader_draw(body, buf),
            View::Graph => self.graph_draw(body, buf),
        }

        let foot = Rect::new(area.x, area.y + area.height - 1, area.width, 1);
        if let Some(m) = &self.message {
            put(buf, foot.x + 1, foot.y, m, foot.width as usize - 2, theme.key());
        } else {
            // Beim Tippen (Suche) wirken `?` und `q` als Zeichen: dort gelten F5 und Strg-C
            let tail: [(&str, &str); 2] = if self.view == View::Search { [("F5", "Hilfe"), ("Strg-C", "Ende")] } else { [("?", "Hilfe"), ("q", "Ende")] };
            ui::footer(buf, foot, &self.hints(), &tail, &theme);
        }
        if self.help {
            self.help_draw(area, buf);
        }
    }

    /// „3/24“ rechts neben der Kernaussage, wenn eine Liste gescrollt werden kann.
    fn position_label(&self) -> Option<String> {
        match self.view {
            View::Overview => {
                let order = self.ov_order();
                if order.is_empty() {
                    return None;
                }
                let pos = self.ov_sel_pos(&order);
                Some(format!("{}/{}", pos + 1, order.len()))
            }
            View::Project => {
                let order = self.pm_order();
                if order.is_empty() {
                    return None;
                }
                let pos = self.pm_sel_pos(&order);
                Some(format!("{}/{}", pos + 1, order.len()))
            }
            _ => None,
        }
    }

    fn headline(&self, avail: usize) -> String {
        match self.view {
            View::Overview => self.overview_headline_fit(avail),
            View::Project => self.project_headline(),
            View::Search => self.search_headline(),
            View::Reader => self.reader_headline(),
            View::Graph => self.graph_headline(),
        }
    }

    fn hints(&self) -> Vec<(&'static str, &'static str)> {
        match self.view {
            View::Overview => self.overview_hints(),
            View::Project => self.project_hints(),
            View::Search => self.search_hints(),
            View::Reader => self.reader_hints(),
            View::Graph => self.graph_hints(),
        }
    }

    /// Tasten, die im aktuellen Zustand wirken (nur sie stehen in der Hilfe, §16.5).
    pub(crate) fn help_lines(&self) -> Vec<(&'static str, &'static str)> {
        let typing = self.view == View::Search;
        let mut v: Vec<(&'static str, &'static str)> = Vec::new();
        if typing {
            v.push(("F1–F4", "Übersicht · Projekt · Suche · Graph"));
            v.push(("Text", "Suchbegriffe tippen (Umlaute und Groß/Klein egal)"));
            v.push(("Rücktaste", "Zeichen löschen    Strg-U alles löschen"));
            v.push(("↑ ↓", "Treffer wählen (auch Strg-N / Strg-P)"));
            v.push(("PgUp PgDn", "blättern    Pos1/Ende Anfang/Ende"));
            v.push(("Tab", "Typfilter weiter    Umschalt-Tab zurück"));
            v.push(("Enter", "Treffer im Leser öffnen"));
            v.push(("Esc", "Eingabe leeren, bei leerer Eingabe zurück"));
            v.push(("F5", "diese Hilfe"));
            v.push(("Strg-C", "beenden (`q` und `?` sind hier Zeichen)"));
            return v;
        }
        v.push(("1 2 3 4", "Übersicht · Projekt · Suche · Graph (auch F1–F4)"));
        v.push(("/", "Suche öffnen"));
        match self.view {
            View::Overview => {
                v.push(("↑ ↓ j k", "wählen    PgUp/PgDn blättern    Pos1/Ende Anfang/Ende"));
                v.push(("Enter", "Projekt öffnen"));
                v.push(("s", "Sortierung wechseln (Aktivität, Größe, Alter, Name, Befunde)"));
                v.push(("S", "Sortierung umkehren"));
                v.push(("f", "nur Projekte mit Änderung in den letzten 30 Tagen ein/aus („aktiv“ im Kopfsatz heißt dagegen status: active)"));
                v.push(("g", "Graph des Projekts"));
                v.push(("e", "Übersichtsnotiz im Editor öffnen ($VISUAL, $EDITOR)"));
            }
            View::Project => {
                v.push(("↑ ↓ j k", "wählen    PgUp/PgDn blättern    Pos1/Ende Anfang/Ende"));
                v.push(("Enter", "Notiz lesen"));
                v.push(("s", "Sortierung wechseln (Alter, Größe, Name, Befunde)"));
                v.push(("g", "Graph der Notiz"));
                v.push(("Tab", "nächstes Projekt    Umschalt-Tab voriges"));
                v.push(("Esc", "zurück (Verlauf)"));
                v.push(("e", "Notiz im Editor öffnen ($VISUAL, $EDITOR)"));
            }
            View::Reader if self.rd.focus_links => {
                v.push(("↑ ↓ j k", "Link wählen"));
                v.push(("Enter", "Link folgen"));
                v.push(("Tab Esc", "zurück zum Text"));
                v.push(("e", "Notiz im Editor öffnen ($VISUAL, $EDITOR)"));
            }
            View::Reader => {
                v.push(("↑ ↓ j k", "zeilenweise    Leertaste/PgDn Seite    PgUp zurück    Pos1/Ende"));
                v.push(("] [", "nächster / voriger Abschnitt"));
                v.push(("Tab l", "Links der Notiz wählen"));
                v.push(("g", "Graph der Notiz"));
                v.push(("Esc", "zurück (Verlauf)"));
                v.push(("e", "Notiz im Editor öffnen ($VISUAL, $EDITOR)"));
            }
            View::Graph => {
                v.push(("← ↑ ↓ → h j k l", "Knoten wählen"));
                v.push(("Enter", "gewählten Knoten zum Fokus machen"));
                v.push(("o", "gewählten Knoten lesen bzw. Projekt öffnen"));
                v.push(("d", "Tiefe 1 / 2"));
                v.push(("p", "Notizen- / Projektebene"));
                if self.gr.level == Level::Projects {
                    v.push(("Tab", "nächstes Projekt    Umschalt-Tab voriges"));
                }
                v.push(("Esc", "zurück (Verlauf)"));
                v.push(("e", "Notiz im Editor öffnen ($VISUAL, $EDITOR)"));
            }
            View::Search => {}
        }
        v.push(("r", "Daten neu laden (Index angleichen)"));
        v.push(("?", "diese Hilfe    F5 auch"));
        v.push(("q", "beenden    Strg-C auch"));
        v
    }

    /// Geometrie der Hilfe: Rahmen und Zahl der sichtbaren Textzeilen.
    fn help_box(&self, area: Rect) -> (Rect, usize) {
        let w = (area.width.saturating_sub(6)).min(86);
        let h = (self.help_lines().len() as u16 + 4).min(area.height);
        let r = Rect::new(area.x + (area.width.saturating_sub(w)) / 2, area.y + (area.height.saturating_sub(h)) / 2, w, h);
        (r, (h as usize).saturating_sub(3))
    }

    fn help_rows(&self) -> usize {
        self.help_box(self.area.get()).1
    }

    fn help_max_scroll(&self) -> usize {
        self.help_lines().len().saturating_sub(self.help_rows())
    }

    fn help_draw(&self, area: Rect, buf: &mut Buffer) {
        let theme = self.theme;
        let lines = self.help_lines();
        let (r, rows) = self.help_box(area);
        for y in r.y..r.y + r.height {
            for xx in r.x..r.x + r.width {
                if let Some(c) = buf.cell_mut((xx, y)) {
                    c.reset();
                    c.set_style(Style::default());
                }
            }
        }
        crate::text::fill(buf, r, self.theme.selected().remove_modifier(ratatui::style::Modifier::BOLD));
        put(buf, r.x + 2, r.y + 1, "Tasten", 6, theme.title());
        let max = lines.len().saturating_sub(rows);
        let sc = self.help_scroll.get().min(max);
        if max > 0 {
            let note = format!("{}–{} von {}  ↑↓ blättern", sc + 1, sc + rows, lines.len());
            let note = if theme.ascii { crate::text::asciify(&note) } else { note };
            let nw = width(&note);
            if (r.width as usize) > nw + 12 {
                put(buf, r.x + r.width - nw as u16 - 2, r.y + 1, &note, nw, theme.dim());
            }
        }
        for (i, (k, d)) in lines.iter().skip(sc).take(rows).enumerate() {
            let y = r.y + 2 + i as u16;
            put(buf, r.x + 2, y, k, 15, theme.key());
            put(buf, r.x + 18, y, d, (r.width as usize).saturating_sub(20), theme.text());
        }
    }

    // ---------- Hilfsfunktionen für Tests und Schnappschüsse ----------

    /// Tastenfolge in Kurzschrift: `"j j Enter"`, `"Ctrl-n"`, einzelne Zeichen als Text.
    pub fn press(&mut self, keys: &str) {
        for k in keys.split_whitespace() {
            let (code, mods) = match k {
                "Enter" => (KeyCode::Enter, KeyModifiers::NONE),
                "Esc" => (KeyCode::Esc, KeyModifiers::NONE),
                "Tab" => (KeyCode::Tab, KeyModifiers::NONE),
                "BackTab" => (KeyCode::BackTab, KeyModifiers::SHIFT),
                "Backspace" => (KeyCode::Backspace, KeyModifiers::NONE),
                "Up" => (KeyCode::Up, KeyModifiers::NONE),
                "Down" => (KeyCode::Down, KeyModifiers::NONE),
                "Left" => (KeyCode::Left, KeyModifiers::NONE),
                "Right" => (KeyCode::Right, KeyModifiers::NONE),
                "PgUp" => (KeyCode::PageUp, KeyModifiers::NONE),
                "PgDn" => (KeyCode::PageDown, KeyModifiers::NONE),
                "Home" => (KeyCode::Home, KeyModifiers::NONE),
                "End" => (KeyCode::End, KeyModifiers::NONE),
                "Space" => (KeyCode::Char(' '), KeyModifiers::NONE),
                s if s.starts_with("Ctrl-") => (KeyCode::Char(s.chars().nth(5).unwrap_or('c')), KeyModifiers::CONTROL),
                s if s.starts_with('F') && s.len() > 1 && s[1..].chars().all(|c| c.is_ascii_digit()) => {
                    (KeyCode::F(s[1..].parse().unwrap_or(1)), KeyModifiers::NONE)
                }
                s => {
                    for ch in s.chars() {
                        self.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
                    }
                    continue;
                }
            };
            self.handle_key(KeyEvent::new(code, mods));
        }
    }

    /// Text in die Sucheingabe tippen (Leerzeichen erlaubt).
    pub fn type_text(&mut self, text: &str) {
        for ch in text.chars() {
            self.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }
    }
}
