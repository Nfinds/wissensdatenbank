//! Gemeinsame Zeichenbausteine der Ansichten: Zeilenschreiber, Sparkline, Balken,
//! Befundzellen, Heatmap, Fußzeile.

use crate::model::*;
use crate::text::{pad_left, pad_right, put, put_spans, truncate, width, Spans};
use crate::theme::Theme;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Left,
    Right,
}

/// Schreibt Zellen mit fester Breite hintereinander.
pub struct RowWriter<'b> {
    pub buf: &'b mut Buffer,
    pub x: u16,
    pub y: u16,
    pub end: u16,
}

impl<'b> RowWriter<'b> {
    pub fn new(buf: &'b mut Buffer, x: u16, y: u16, w: u16) -> Self {
        RowWriter { buf, x, y, end: x + w }
    }

    pub fn cell(&mut self, text: &str, w: usize, align: Align, style: Style, ell: char) {
        let t = truncate(text, w, ell);
        let padded = match align {
            Align::Left => pad_right(&t, w),
            Align::Right => pad_left(&t, w),
        };
        let room = (self.end.saturating_sub(self.x)) as usize;
        self.x = put(self.buf, self.x, self.y, &padded, w.min(room), style);
    }

    pub fn gap(&mut self) {
        self.x = (self.x + 1).min(self.end);
    }

    pub fn spans(&mut self, spans: &[(String, Style)], w: usize) {
        put_spans(self.buf, self.x, self.y, spans, w);
        self.x = (self.x + w as u16).min(self.end);
    }
}

/// Zentriertes Fenster der Länge `h` über `len` Einträgen; zustandslos, damit Schnappschüsse stabil sind.
pub fn scroll_offset(sel: usize, len: usize, h: usize) -> usize {
    if h == 0 || len <= h {
        return 0;
    }
    let half = h / 2;
    sel.saturating_sub(half).min(len - h)
}

/// Sparkline über Wochenwerte, auf das eigene Maximum skaliert; letzte Woche hervorgehoben.
pub fn sparkline(weekly: &[u16], theme: &Theme) -> Spans {
    let g = theme.glyphs();
    let max = weekly.iter().copied().max().unwrap_or(0);
    let mut out: Spans = Vec::new();
    let n = weekly.len();
    for (i, &v) in weekly.iter().enumerate() {
        let ch = if v == 0 || max == 0 {
            g.spark_zero
        } else {
            let idx = ((v as f64 / max as f64) * 7.0).ceil() as usize;
            g.spark[idx.clamp(1, 8) - 1]
        };
        let style = if v == 0 {
            theme.dim()
        } else if i + 1 == n {
            theme.spark_now()
        } else {
            theme.spark_old()
        };
        out.push((ch.to_string(), style));
    }
    out
}

/// Balken aus Achtelblöcken: `frac` in 0..=1 auf `cells` Zellen.
pub fn size_bar(frac: f64, cells: usize, theme: &Theme) -> String {
    let g = theme.glyphs();
    let total = (frac.clamp(0.0, 1.0) * (cells * 8) as f64).round() as usize;
    let total = total.max(1).min(cells * 8);
    let full = total / 8;
    let part = total % 8;
    let mut s = String::new();
    for _ in 0..full {
        s.push(g.eighths[7]);
    }
    if part > 0 && full < cells {
        s.push(g.eighths[part - 1]);
    }
    pad_right(&s, cells)
}

/// Wurzelskala, damit kleine Projekte neben einem 1,3-MB-Riesen sichtbar bleiben.
pub fn sqrt_frac(v: u64, max: u64) -> f64 {
    if max == 0 {
        0.0
    } else {
        (v as f64 / max as f64).sqrt()
    }
}

/// `V G T W` als vier Zellen zu je einem Zeichen mit Leerzeichen dazwischen (7 Spalten).
pub fn finding_cells(f: &Findings, theme: &Theme) -> Spans {
    let g = theme.glyphs();
    let mut out: Spans = Vec::new();
    for (k, n) in f.as_array().iter().enumerate() {
        if k > 0 {
            out.push((" ".to_string(), Style::default()));
        }
        let (ch, st) = match *n {
            0 => (g.dot_none.to_string(), theme.dim()),
            1..=9 => (n.to_string(), theme.finding(k)),
            _ => ("+".to_string(), theme.finding(k)),
        };
        out.push((ch, st));
    }
    out
}

pub const FINDING_HEADER: &str = "V G T W";

/// Fußzeile aus Tastenpaaren. `tail` (z. B. Hilfe, Ende) bleibt immer stehen.
pub fn footer(buf: &mut Buffer, r: Rect, hints: &[(&str, &str)], tail: &[(&str, &str)], theme: &Theme) {
    if r.height == 0 || r.width == 0 {
        return;
    }
    let sep = 2usize;
    let size = |h: &(&str, &str)| width(h.0) + 1 + width(h.1);
    let tail_w: usize = tail.iter().map(|h| size(h) + sep).sum();
    let avail = (r.width as usize).saturating_sub(tail_w + 1);
    let mut used = 0usize;
    let mut shown: Vec<&(&str, &str)> = Vec::new();
    for h in hints {
        let add = size(h) + if shown.is_empty() { 0 } else { sep };
        if used + add > avail {
            break;
        }
        used += add;
        shown.push(h);
    }
    let mut x = r.x + 1;
    let all: Vec<&(&str, &str)> = shown.into_iter().chain(tail.iter()).collect();
    for (i, h) in all.iter().enumerate() {
        if i > 0 {
            x += sep as u16;
        }
        x = put(buf, x, r.y, h.0, (r.x + r.width).saturating_sub(x) as usize, theme.key());
        x = put(buf, x + 1, r.y, h.1, (r.x + r.width).saturating_sub(x + 1) as usize, theme.dim());
    }
}

/// Quantilgrenzen (25/50/75 %) der Werte größer null.
pub fn heat_thresholds(series: &[u16]) -> [u16; 3] {
    let mut v: Vec<u16> = series.iter().copied().filter(|&x| x > 0).collect();
    if v.is_empty() {
        return [1, 2, 3];
    }
    v.sort_unstable();
    let q = |p: f64| v[((v.len() - 1) as f64 * p).round() as usize];
    [q(0.25), q(0.5), q(0.75)]
}

pub fn heat_level(v: u16, th: &[u16; 3]) -> u8 {
    if v == 0 {
        0
    } else if v <= th[0] {
        1
    } else if v <= th[1] {
        2
    } else if v <= th[2] {
        3
    } else {
        4
    }
}

pub const HEATMAP_ROWS: u16 = 9;

/// Kennzahlen des Heatmap-Fensters für die Unterschrift.
pub struct HeatSummary {
    pub total: u32,
    pub active_days: u32,
    pub days: u32,
    pub peak: u16,
    pub peak_date: Option<Date>,
}

/// Heatmap: Zeilen Mo–So, Spalten Wochen; Zelle zwei Zeichen. Belegt `HEATMAP_ROWS` Zeilen.
/// `series` endet mit `today`. Gibt (Summary, x des rechten Randes der Karte) zurück.
pub fn draw_heatmap(
    buf: &mut Buffer,
    r: Rect,
    series: &[u16],
    all_series: &[u16],
    today: Date,
    weeks: usize,
    theme: &Theme,
) -> (HeatSummary, u16) {
    let g = theme.glyphs();
    let th = heat_thresholds(all_series);
    let wd_today = today.weekday() as i32;
    let n = series.len() as i32;
    let value_at = |date: Date| -> Option<u16> {
        if date > today {
            return None;
        }
        let back = today.since(date);
        if back >= n {
            return Some(0);
        }
        Some(series[(n - 1 - back) as usize])
    };

    let label_w = 3u16;
    let mut sum = HeatSummary { total: 0, active_days: 0, days: 0, peak: 0, peak_date: None };
    // Monatsbeschriftung (Zeile 1)
    let mut last_month = 0u32;
    let mut next_free = 0usize;
    for w in 0..weeks {
        let monday = today.add(-wd_today - 7 * (weeks - 1 - w) as i32);
        let (_, m, _) = monday.ymd();
        // `last_month` erst setzen, wenn die Beschriftung wirklich steht; sonst fiele ein Monat aus,
        // dessen Name wegen des Vorgängers keinen Platz hatte (N9)
        if m != last_month && w >= next_free {
            last_month = m;
            put(buf, r.x + label_w + (w * 2) as u16, r.y + 1, monday.month_abbr(), 3, theme.dim());
            next_free = w + 2;
        }
        for wd in 0..7u32 {
            let date = monday.add(wd as i32);
            let x = r.x + label_w + (w * 2) as u16;
            let y = r.y + 2 + wd as u16;
            match value_at(date) {
                None => {}
                Some(v) => {
                    let lvl = heat_level(v, &th);
                    put(buf, x, y, g.heat[lvl as usize], 2, theme.heat(lvl));
                    sum.days += 1;
                    sum.total += v as u32;
                    if v > 0 {
                        sum.active_days += 1;
                    }
                    if v > sum.peak {
                        sum.peak = v;
                        sum.peak_date = Some(date);
                    }
                }
            }
        }
    }
    for (wd, name) in WEEKDAYS.iter().enumerate() {
        put(buf, r.x, r.y + 2 + wd as u16, name, 2, theme.dim());
    }
    (sum, r.x + label_w + (weeks * 2) as u16)
}

/// Unterschrift der Heatmap in der längsten Fassung, die in `max_w` Spalten passt; nie abgeschnitten.
pub fn heat_caption(s: &HeatSummary, weeks: usize, theme: &Theme, max_w: usize) -> Spans {
    let peak = match s.peak_date {
        Some(d) => format!("Spitze {} {} ({})", WEEKDAYS[d.weekday() as usize], d, s.peak),
        None => "keine Aktivität".to_string(),
    };
    let variants = [
        format!("{} Wochen · {} Einträge an {} Tagen · {}", weeks, s.total, s.active_days, peak),
        format!("{} Wochen · {} Einträge an {} Tagen", weeks, s.total, s.active_days),
        format!("{} Wochen · {} Einträge", weeks, s.total),
        format!("{} Einträge", s.total),
    ];
    let head = "Aktivität ";
    let rest = variants.iter().find(|v| width(head) + width(v) <= max_w).cloned().unwrap_or_default();
    let head = if rest.is_empty() && width(head.trim_end()) > max_w { String::new() } else { head.to_string() };
    vec![(head, theme.title()), (rest, theme.dim())]
}

pub fn finding_legend() -> [(&'static str, &'static str); 4] {
    [("V", "veraltet"), ("G", "groß"), ("T", "tote Links"), ("W", "Waise")]
}
