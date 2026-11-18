//! Farbsprache und Zeichensätze. Farbe gehört dem Zustand (Frische, Befund, Treffer),
//! nie der Identität. Ohne Farbe bleibt alles über Zeichen und Modifikatoren lesbar.

use crate::model::Freshness;
use ratatui::style::{Color, Modifier, Style};

#[derive(Clone, Copy, Debug)]
pub struct Theme {
    pub color: bool,
    pub ascii: bool,
}

const fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::Rgb(r, g, b)
}

/// `NO_COLOR` ist gesetzt und nicht leer (https://no-color.org). Eine Stelle für `wb ui` und `wb stats`.
pub fn no_color_env() -> bool {
    std::env::var_os("NO_COLOR").map(|v| !v.is_empty()).unwrap_or(false)
}

impl Theme {
    pub fn color() -> Theme {
        Theme { color: true, ascii: false }
    }
    pub fn mono() -> Theme {
        Theme { color: false, ascii: false }
    }
    pub fn with_ascii(mut self, ascii: bool) -> Theme {
        self.ascii = ascii;
        self
    }

    /// `NO_COLOR` (gesetzt und nicht leer) schaltet Farbe aus; dieselbe Regel gilt für `wb stats`.
    pub fn from_env() -> Theme {
        Theme { color: !no_color_env(), ascii: false }
    }

    fn c(&self, color: Color, fallback: Modifier) -> Style {
        if self.color {
            Style::default().fg(color)
        } else {
            Style::default().add_modifier(fallback)
        }
    }

    pub fn text(&self) -> Style {
        Style::default()
    }
    pub fn bold(&self) -> Style {
        Style::default().add_modifier(Modifier::BOLD)
    }
    /// Zurückgenommener Text (Spaltenköpfe, Hinweise).
    pub fn dim(&self) -> Style {
        if self.color {
            Style::default().fg(rgb(120, 128, 140))
        } else {
            Style::default()
        }
    }
    pub fn accent(&self) -> Style {
        self.c(rgb(94, 190, 214), Modifier::BOLD)
    }
    pub fn title(&self) -> Style {
        if self.color {
            Style::default().fg(rgb(235, 238, 242)).add_modifier(Modifier::BOLD)
        } else {
            Style::default().add_modifier(Modifier::BOLD)
        }
    }
    pub fn key(&self) -> Style {
        self.c(rgb(240, 200, 110), Modifier::BOLD)
    }
    pub fn selected(&self) -> Style {
        if self.color {
            Style::default().bg(rgb(38, 50, 68)).add_modifier(Modifier::BOLD)
        } else {
            Style::default().add_modifier(Modifier::REVERSED)
        }
    }
    pub fn tab_active(&self) -> Style {
        if self.color {
            Style::default()
                .fg(rgb(15, 20, 28))
                .bg(rgb(94, 190, 214))
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD)
        }
    }
    pub fn link(&self) -> Style {
        if self.color {
            Style::default()
                .fg(rgb(120, 170, 255))
                .add_modifier(Modifier::UNDERLINED)
        } else {
            Style::default().add_modifier(Modifier::UNDERLINED)
        }
    }
    pub fn code(&self) -> Style {
        self.c(rgb(206, 160, 120), Modifier::empty())
    }
    pub fn quote(&self) -> Style {
        if self.color {
            Style::default().fg(rgb(150, 160, 120)).add_modifier(Modifier::ITALIC)
        } else {
            Style::default().add_modifier(Modifier::ITALIC)
        }
    }
    /// Treffer in Suche und Vorschau.
    pub fn hit(&self) -> Style {
        if self.color {
            Style::default()
                .fg(rgb(20, 20, 20))
                .bg(rgb(240, 200, 90))
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
        }
    }

    pub fn fresh(&self, f: Freshness) -> Style {
        match f {
            Freshness::Fresh => self.c(rgb(84, 204, 124), Modifier::BOLD),
            Freshness::Recent => self.c(rgb(170, 206, 96), Modifier::empty()),
            Freshness::Aging => self.c(rgb(232, 172, 64), Modifier::empty()),
            Freshness::Stale => self.c(rgb(226, 92, 92), Modifier::empty()),
            Freshness::Unknown => self.dim(),
        }
    }

    /// Befundzahl: Farbe nach Art (Veraltet bernstein, Groß violett, Tot rot, Waise grau-blau).
    pub fn finding(&self, kind: usize) -> Style {
        match kind {
            0 => self.c(rgb(232, 172, 64), Modifier::BOLD),
            1 => self.c(rgb(196, 140, 230), Modifier::BOLD),
            2 => self.c(rgb(240, 100, 100), Modifier::BOLD),
            _ => self.c(rgb(140, 160, 200), Modifier::BOLD),
        }
    }

    pub fn spark_old(&self) -> Style {
        self.c(rgb(86, 124, 160), Modifier::empty())
    }
    pub fn spark_now(&self) -> Style {
        self.c(rgb(120, 214, 236), Modifier::BOLD)
    }
    pub fn bar(&self) -> Style {
        self.c(rgb(104, 140, 190), Modifier::empty())
    }

    /// Stufe 0..=4 der Heatmap (Grün, einfarbige Rampe).
    pub fn heat(&self, level: u8) -> Style {
        match level {
            0 => self.dim(),
            1 => self.c(rgb(40, 110, 70), Modifier::empty()),
            2 => self.c(rgb(46, 150, 82), Modifier::empty()),
            3 => self.c(rgb(70, 196, 100), Modifier::empty()),
            _ => self.c(rgb(140, 240, 150), Modifier::BOLD),
        }
    }

    pub fn glyphs(&self) -> Glyphs {
        if self.ascii {
            Glyphs::ascii()
        } else {
            Glyphs::unicode()
        }
    }
}

/// Zeichensatz; der ASCII-Rückfall ist für alte Konsolenschriften gedacht.
#[derive(Clone, Copy, Debug)]
pub struct Glyphs {
    pub spark: [char; 8],
    pub spark_zero: char,
    /// Teilblöcke von 1/8 bis 7/8 und voller Block.
    pub eighths: [char; 8],
    pub heat: [&'static str; 5],
    pub dot_full: char,
    /// ≤ 30 Tage; ein eigenes Zeichen, damit der Unterschied zu „frisch“ ohne Farbe bleibt.
    pub dot_recent: char,
    pub dot_half: char,
    pub dot_empty: char,
    pub dot_none: char,
    pub ellipsis: char,
    pub sort_down: char,
    pub sort_up: char,
    pub arrow: char,
    pub bullet: char,
    pub bullet2: char,
    pub quote_bar: char,
    pub rule: char,
    pub sep: char,
    pub cursor: char,
    /// Marke vor überholten Notizen und Projekten (ohne Farbe erkennbar).
    pub superseded: char,
    ascii: bool,
}

impl Glyphs {
    pub fn unicode() -> Glyphs {
        Glyphs {
            spark: ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'],
            spark_zero: '·',
            eighths: ['▏', '▎', '▍', '▌', '▋', '▊', '▉', '█'],
            heat: ["· ", "░░", "▒▒", "▓▓", "██"],
            dot_full: '●',
            dot_recent: '◕',
            dot_half: '◐',
            dot_empty: '○',
            dot_none: '·',
            ellipsis: '…',
            sort_down: '▼',
            sort_up: '▲',
            arrow: '▸',
            bullet: '•',
            bullet2: '◦',
            quote_bar: '▎',
            rule: '─',
            sep: '│',
            cursor: '▏',
            superseded: '×',
            ascii: false,
        }
    }

    pub fn ascii() -> Glyphs {
        Glyphs {
            spark: ['_', '.', '-', '=', '+', '*', '#', '@'],
            spark_zero: '.',
            eighths: ['-', '-', '=', '=', '=', '#', '#', '#'],
            heat: [". ", "::", "++", "**", "##"],
            dot_full: '*',
            dot_recent: '+',
            dot_half: 'o',
            dot_empty: '-',
            dot_none: '.',
            ellipsis: '~',
            sort_down: 'v',
            sort_up: '^',
            arrow: '>',
            bullet: '*',
            bullet2: '-',
            quote_bar: '|',
            rule: '-',
            sep: '|',
            cursor: '_',
            superseded: 'x',
            ascii: true,
        }
    }

    pub fn freshness(&self, f: Freshness) -> char {
        match f {
            Freshness::Fresh => self.dot_full,
            Freshness::Recent => self.dot_recent,
            Freshness::Aging => self.dot_half,
            Freshness::Stale => self.dot_empty,
            Freshness::Unknown => self.dot_none,
        }
    }

    /// Linienzeichen aus Richtungsmaske: oben=1, unten=2, links=4, rechts=8.
    pub fn line(&self, mask: u8) -> char {
        if self.ascii {
            return match mask {
                0 => ' ',
                m if m & 3 != 0 && m & 12 != 0 => '+',
                m if m & 3 != 0 => '|',
                _ => '-',
            };
        }
        match mask {
            0 => ' ',
            1..=3 => '│',
            4 | 8 | 12 => '─',
            10 => '┌',
            6 => '┐',
            9 => '└',
            5 => '┘',
            11 => '├',
            7 => '┤',
            14 => '┬',
            13 => '┴',
            15 => '┼',
            _ => ' ',
        }
    }
}
