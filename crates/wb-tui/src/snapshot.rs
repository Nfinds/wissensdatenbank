//! Headless-Schnappschüsse: dieselbe Zeichenroutine wie im Terminal, ausgegeben als reiner
//! Text, als farbtreues HTML und als ANSI-Folge.

use crate::app::{App, Level, View};
use crate::model::NoteType;
use crate::source::DataSource;
use crate::theme::Theme;
use ratatui::buffer::{Buffer, Cell};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier};
use std::fmt::Write as _;
use std::io;
use std::path::{Path, PathBuf};
use unicode_width::UnicodeWidthStr;

pub const SIZES: [(u16, u16); 2] = [(80, 24), (160, 45)];

/// Namen der Standard-Szenarien (jede Ansicht, dazu ein paar Zustände).
pub const SCENARIOS: [&str; 8] = [
    "overview",
    "overview-size",
    "project",
    "search",
    "reader",
    "graph-notes",
    "graph-notes-depth2",
    "graph-projects",
];

/// Baut die App im Zustand des Szenarios.
pub fn scenario<'a>(src: &'a dyn DataSource, name: &str, theme: Theme) -> Option<App<'a>> {
    let mut app = App::new(src, theme);
    // Echte Daten: `WB_TUI_PROJECT=<Name>` begrenzt alles, was Notizinhalte zeigt, auf ein Projekt.
    let fixed = fixed_project(&app);
    if let Some(p) = &fixed {
        app.ov.sel = Some(p.clone());
    }
    match name {
        "overview" => {}
        "overview-size" => app.press("s j j j"),
        "project" => {
            app.press("Enter");
            if fixed.is_none() {
                app.press("j j");
            }
        }
        "search" => {
            let q = std::env::var("WB_TUI_QUERY").unwrap_or_else(|_| "probelauf lieferant".to_string());
            app.open_search(&q);
            app.press("Down");
        }
        "reader" => {
            let path = pick_linked_topic(&app)?;
            app.open_reader(&path);
            app.press("Tab Down");
        }
        "graph-notes" => {
            let path = pick_hub(&app)?;
            app.open_graph(Level::Notes, &path, 1);
        }
        "graph-notes-depth2" => {
            let path = pick_linked_topic(&app)?;
            app.open_graph(Level::Notes, &path, 2);
        }
        "graph-projects" => {
            let name = fixed.clone().or_else(|| app.project_ranking().first().map(|r| r.0.clone()))?;
            app.open_graph(Level::Projects, &name, 1);
            app.press("d");
        }
        _ => return None,
    }
    Some(app)
}

fn fixed_project(app: &App) -> Option<String> {
    let want = std::env::var("WB_TUI_PROJECT").ok()?;
    app.projects.iter().find(|p| p.name == want).map(|p| p.name.clone())
}

fn base_project(app: &App) -> Option<String> {
    fixed_project(app).or_else(|| {
        app.projects.iter().max_by_key(|p| (p.notes, std::cmp::Reverse(p.name.clone()))).map(|p| p.name.clone())
    })
}

/// Thema mit vielen Verknüpfungen im größten Projekt.
fn pick_linked_topic(app: &App) -> Option<String> {
    let first = base_project(app)?;
    let notes = app.src.notes(&first);
    notes
        .iter()
        .filter(|n| n.ntype == NoteType::Topic)
        .max_by_key(|n| (n.links_in + n.links_out, std::cmp::Reverse(n.path.clone())))
        .map(|n| n.path.clone())
}

/// Notiz mit den meisten Verknüpfungen insgesamt (zeigt die Kappung großer Nachbarschaften).
fn pick_hub(app: &App) -> Option<String> {
    let first = base_project(app)?;
    app.src
        .notes(&first)
        .iter()
        .max_by_key(|n| (n.links_in + n.links_out, std::cmp::Reverse(n.path.clone())))
        .map(|n| n.path.clone())
}

pub fn render_app(app: &App, w: u16, h: u16) -> Buffer {
    let area = Rect::new(0, 0, w, h);
    let mut buf = Buffer::empty(area);
    app.render(area, &mut buf);
    buf
}

pub fn view_name(v: View) -> &'static str {
    v.name()
}

fn cell_width(c: &Cell) -> usize {
    UnicodeWidthStr::width(c.symbol()).max(1)
}

/// Reiner Text: Zeilen rechts beschnitten, Doppelbreiten ohne Doppelzählung.
pub fn to_text(buf: &Buffer) -> String {
    let a = buf.area;
    let mut out = String::new();
    for y in a.y..a.y + a.height {
        let mut line = String::new();
        let mut x = a.x;
        while x < a.x + a.width {
            let c = &buf[(x, y)];
            line.push_str(c.symbol());
            x += cell_width(c) as u16;
        }
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out
}

fn named(c: Color) -> Option<(u8, u8, u8)> {
    Some(match c {
        Color::Rgb(r, g, b) => (r, g, b),
        Color::Black => (0, 0, 0),
        Color::Red => (205, 49, 49),
        Color::Green => (13, 188, 121),
        Color::Yellow => (229, 229, 16),
        Color::Blue => (36, 114, 200),
        Color::Magenta => (188, 63, 188),
        Color::Cyan => (17, 168, 205),
        Color::Gray => (204, 204, 204),
        Color::DarkGray => (102, 102, 102),
        Color::White => (255, 255, 255),
        Color::LightRed => (241, 76, 76),
        Color::LightGreen => (35, 209, 139),
        Color::LightYellow => (245, 245, 67),
        Color::LightBlue => (59, 142, 234),
        Color::LightMagenta => (214, 112, 214),
        Color::LightCyan => (41, 184, 219),
        Color::Indexed(i) => {
            let v = (i as u32 * 10 % 256) as u8;
            (v, v, v)
        }
        Color::Reset => return None,
    })
}

const HTML_FG: (u8, u8, u8) = (214, 219, 227);
const HTML_BG: (u8, u8, u8) = (18, 22, 29);

fn esc(s: &str, out: &mut String) {
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            c => out.push(c),
        }
    }
}

/// Farbtreues HTML: eine Seite, ein `<pre>`, Spans je Stillauf, keine Skripte, kein Netz.
pub fn to_html(buf: &Buffer, title: &str) -> String {
    let a = buf.area;
    let mut out = String::new();
    let _ = write!(
        out,
        "<!doctype html>\n<html lang=\"de\"><head><meta charset=\"utf-8\"><title>{}</title>\n<style>\nbody{{margin:0;padding:18px;background:#0b0e13;color:#{:02x}{:02x}{:02x}}}\n.cap{{font:12px/1.4 \"Segoe UI\",sans-serif;color:#8a93a3;margin:0 0 8px 2px}}\npre{{display:inline-block;margin:0;padding:10px 12px;background:#{:02x}{:02x}{:02x};border-radius:6px;font:14px/1.22 \"Cascadia Mono\",\"Cascadia Code\",Consolas,\"DejaVu Sans Mono\",monospace;font-variant-ligatures:none;tab-size:4}}\n</style></head><body>\n<p class=\"cap\">{} · {}×{}</p>\n<pre>",
        title, HTML_FG.0, HTML_FG.1, HTML_FG.2, HTML_BG.0, HTML_BG.1, HTML_BG.2, title, a.width, a.height
    );
    for y in a.y..a.y + a.height {
        let mut x = a.x;
        let mut cur: Option<(Color, Color, Modifier)> = None;
        while x < a.x + a.width {
            let c = &buf[(x, y)];
            let key = (c.fg, c.bg, c.modifier);
            if cur != Some(key) {
                if cur.is_some() {
                    out.push_str("</span>");
                }
                let mut fg = named(c.fg);
                let mut bg = named(c.bg);
                if c.modifier.contains(Modifier::REVERSED) {
                    let f = fg.unwrap_or(HTML_FG);
                    let b = bg.unwrap_or(HTML_BG);
                    fg = Some(b);
                    bg = Some(f);
                }
                let mut style = String::new();
                if let Some((r, g, b)) = fg {
                    let _ = write!(style, "color:#{r:02x}{g:02x}{b:02x};");
                }
                if let Some((r, g, b)) = bg {
                    let _ = write!(style, "background:#{r:02x}{g:02x}{b:02x};");
                }
                if c.modifier.contains(Modifier::BOLD) {
                    style.push_str("font-weight:bold;");
                }
                if c.modifier.contains(Modifier::ITALIC) {
                    style.push_str("font-style:italic;");
                }
                if c.modifier.contains(Modifier::UNDERLINED) {
                    style.push_str("text-decoration:underline;");
                }
                if c.modifier.contains(Modifier::DIM) {
                    style.push_str("opacity:.7;");
                }
                let _ = write!(out, "<span style=\"{style}\">");
                cur = Some(key);
            }
            esc(c.symbol(), &mut out);
            x += cell_width(c) as u16;
        }
        if cur.is_some() {
            out.push_str("</span>");
        }
        out.push('\n');
    }
    out.push_str("</pre>\n</body></html>\n");
    out
}

/// ANSI-Folge mit 24-Bit-Farben (für `stats` und Terminal-Ausgabe), Zeilen mit `\n`.
pub fn to_ansi(buf: &Buffer) -> String {
    let a = buf.area;
    let mut out = String::new();
    for y in a.y..a.y + a.height {
        // letzte nicht leere Spalte bestimmen
        let mut last = a.x;
        for x in a.x..a.x + a.width {
            let c = &buf[(x, y)];
            if c.symbol() != " " || c.bg != Color::Reset {
                last = x + 1;
            }
        }
        let mut x = a.x;
        let mut cur: (Color, Color, Modifier) = (Color::Reset, Color::Reset, Modifier::empty());
        while x < last {
            let c = &buf[(x, y)];
            let key = (c.fg, c.bg, c.modifier);
            if key != cur {
                out.push_str("\x1b[0m");
                let mut codes: Vec<String> = Vec::new();
                if c.modifier.contains(Modifier::BOLD) {
                    codes.push("1".into());
                }
                if c.modifier.contains(Modifier::ITALIC) {
                    codes.push("3".into());
                }
                if c.modifier.contains(Modifier::UNDERLINED) {
                    codes.push("4".into());
                }
                if c.modifier.contains(Modifier::REVERSED) {
                    codes.push("7".into());
                }
                if let Some((r, g, b)) = named(c.fg) {
                    codes.push(format!("38;2;{r};{g};{b}"));
                }
                if let Some((r, g, b)) = named(c.bg) {
                    codes.push(format!("48;2;{r};{g};{b}"));
                }
                if !codes.is_empty() {
                    let _ = write!(out, "\x1b[{}m", codes.join(";"));
                }
                cur = key;
            }
            out.push_str(c.symbol());
            x += cell_width(c) as u16;
        }
        if cur != (Color::Reset, Color::Reset, Modifier::empty()) {
            out.push_str("\x1b[0m");
        }
        out.push('\n');
    }
    out
}

/// Prüft, ob der Puffer keinerlei Farbe trägt (Vorder- oder Hintergrund).
pub fn is_colorless(buf: &Buffer) -> bool {
    let a = buf.area;
    for y in a.y..a.y + a.height {
        for x in a.x..a.x + a.width {
            let c = &buf[(x, y)];
            if c.fg != Color::Reset || c.bg != Color::Reset {
                return false;
            }
        }
    }
    true
}

/// Schreibt alle Szenarien in allen Größen als `.txt` und `.html` nach `dir`.
pub fn write_all(src: &dyn DataSource, dir: &Path, prefix: &str) -> io::Result<Vec<PathBuf>> {
    std::fs::create_dir_all(dir)?;
    let mut written = Vec::new();
    for name in SCENARIOS {
        for (w, h) in SIZES {
            let Some(app) = scenario(src, name, Theme::color()) else { continue };
            let buf = render_app(&app, w, h);
            let stem = format!("{prefix}{name}-{w}x{h}");
            let t = dir.join(format!("{stem}.txt"));
            std::fs::write(&t, to_text(&buf))?;
            let hp = dir.join(format!("{stem}.html"));
            std::fs::write(&hp, to_html(&buf, &stem))?;
            written.push(t);
            written.push(hp);
            // ohne Farbe nur als Text
            if let Some(m) = scenario(src, name, Theme::mono()) {
                let mb = render_app(&m, w, h);
                let tp = dir.join(format!("{stem}.nocolor.txt"));
                std::fs::write(&tp, to_text(&mb))?;
                written.push(tp);
            }
        }
    }
    Ok(written)
}
