//! Statische Übersicht für `wb stats`: Kopfsatz, die obersten Projekte, Aktivitäts-Heatmap.
//! Eine Bibliotheksfunktion, die A in die CLI einhängt.

use crate::app::App;
use crate::snapshot::{to_ansi, to_text};
use crate::source::DataSource;
use crate::text::{put, truncate, width};
use crate::theme::Theme;
use crate::ui::HEATMAP_ROWS;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

/// Kleinste und größte Breite von `wb stats --width N` (Vertrag §16.4). Kleinere Werte werden auf
/// `STATS_MIN_WIDTH` gesetzt, größere auf `STATS_MAX_WIDTH`; die Hilfe von `--width` soll das sagen
/// (`STATS_WIDTH_HELP`).
pub const STATS_MIN_WIDTH: u16 = 40;
pub const STATS_MAX_WIDTH: u16 = 240;
pub const STATS_WIDTH_HELP: &str = "Breite in Spalten, 40 bis 240 (kleinere Werte gelten als 40, größere als 240)";

/// Höchstens so viele Projekte stehen in der Tabelle (der Rest wird als „n von m“ angezeigt).
pub const STATS_ROWS: usize = 12;

/// Rendert die Übersicht auf `width` Spalten. `color=false` liefert reinen Text ohne
/// Escape-Folgen, sonst ANSI mit 24-Bit-Farben. Deterministisch, wenn die Quelle es ist.
pub fn render(src: &dyn DataSource, width_cols: u16, color: bool) -> String {
    render_with(src, width_cols, if color { Theme::color() } else { Theme::mono() }, color)
}

/// Einstieg für `wb stats`: `ascii` für `--ascii`, `tty` = Ausgabe geht an ein Terminal. Farbe gibt es nur
/// am Terminal und nur ohne `NO_COLOR` (gesetzt und nicht leer, Vertrag §16.7).
pub fn render_for(src: &dyn DataSource, width_cols: u16, ascii: bool, tty: bool) -> String {
    let ansi = tty && Theme::from_env().color;
    let theme = if ansi { Theme::color() } else { Theme::mono() }.with_ascii(ascii);
    render_with(src, width_cols, theme, ansi)
}

pub fn render_with(src: &dyn DataSource, width_cols: u16, theme: Theme, ansi: bool) -> String {
    let w = width_cols.clamp(STATS_MIN_WIDTH, STATS_MAX_WIDTH);
    let mut app = App::new(src, theme);
    app.plain = true;
    let n = app.projects.len().clamp(1, STATS_ROWS);
    let hint = app.projects.len() > STATS_ROWS;
    // Kopfzeile + Projekte (+ „n von m“) + Leerzeile + Heatmap (+ Legende darunter, wenn sie daneben nicht passt)
    let legend_below = crate::views::overview::heat_side_width(w) < 26;
    let body_h = 1 + n as u16 + hint as u16 + 1 + HEATMAP_ROWS + legend_below as u16;
    let h = 2 + body_h;
    let area = Rect::new(0, 0, w, h);
    app.area.set(area);
    let mut buf = Buffer::empty(area);
    let g = theme.glyphs();
    put(&mut buf, 1, 0, "Wissensbank", 11, theme.title());
    let label = src.label();
    let lw = width(&label) as u16;
    if lw + 14 <= w {
        put(&mut buf, w - lw - 1, 0, &label, lw as usize, theme.dim());
    }
    let headline = app.overview_headline_fit(w as usize - 2);
    put(&mut buf, 1, 1, &truncate(&headline, w as usize - 2, g.ellipsis), w as usize - 2, theme.title());
    let body = Rect::new(0, 2, w, body_h);
    app.overview_draw(body, &mut buf);
    if theme.ascii {
        crate::text::asciify_buffer(&mut buf, area);
    }
    if ansi {
        to_ansi(&buf)
    } else {
        to_text(&buf)
    }
}
