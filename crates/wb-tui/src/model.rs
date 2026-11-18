//! Datentypen der Menschen-Ebene. Sie spiegeln, was die Ansichten brauchen
//! (Vertrag §7), und sind unabhängig von `wb-core`.

use std::fmt;

/// Kalendertag als Tage seit 1970-01-01 (ohne Zeitzone, ohne Systemuhr).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Date(pub i32);

impl Date {
    pub fn from_ymd(y: i32, m: u32, d: u32) -> Date {
        // Howard Hinnant, days_from_civil
        let y = if m <= 2 { y - 1 } else { y };
        let era = if y >= 0 { y } else { y - 399 } / 400;
        let yoe = (y - era * 400) as i64;
        let mp = ((m as i64) + 9) % 12;
        let doy = (153 * mp + 2) / 5 + d as i64 - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        Date(era * 146_097 + doe as i32 - 719_468)
    }

    pub fn ymd(self) -> (i32, u32, u32) {
        let z = self.0 + 719_468;
        let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
        let doe = (z - era * 146_097) as i64;
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let y = yoe + era as i64 * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
        let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
        ((y + if m <= 2 { 1 } else { 0 }) as i32, m, d)
    }

    /// `JJJJ-MM-TT` am Anfang der Zeichenkette.
    pub fn parse(s: &str) -> Option<Date> {
        let b = s.trim().as_bytes();
        if b.len() < 10 || b[4] != b'-' || b[7] != b'-' {
            return None;
        }
        let num = |r: std::ops::Range<usize>| -> Option<u32> {
            std::str::from_utf8(&b[r]).ok()?.parse().ok()
        };
        let (y, m, d) = (num(0..4)?, num(5..7)?, num(8..10)?);
        if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
            return None;
        }
        Some(Date::from_ymd(y as i32, m, d))
    }

    /// 0 = Montag … 6 = Sonntag.
    pub fn weekday(self) -> u32 {
        (self.0 + 3).rem_euclid(7) as u32
    }

    #[allow(clippy::should_implement_trait)]
    pub fn add(self, days: i32) -> Date {
        Date(self.0 + days)
    }

    /// Tage von `other` bis `self` (positiv, wenn `self` später liegt).
    pub fn since(self, other: Date) -> i32 {
        self.0 - other.0
    }

    pub fn month_abbr(self) -> &'static str {
        const M: [&str; 12] = [
            "Jan", "Feb", "Mär", "Apr", "Mai", "Jun", "Jul", "Aug", "Sep", "Okt", "Nov", "Dez",
        ];
        M[(self.ymd().1 - 1) as usize]
    }
}

impl fmt::Display for Date {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (y, m, d) = self.ymd();
        write!(f, "{y:04}-{m:02}-{d:02}")
    }
}

pub const WEEKDAYS: [&str; 7] = ["Mo", "Di", "Mi", "Do", "Fr", "Sa", "So"];

/// Frische nach Alter der letzten Änderung. Schwellen an genau einer Stelle.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Freshness {
    Fresh,
    Recent,
    Aging,
    Stale,
    Unknown,
}

impl Freshness {
    pub const FRESH_DAYS: i32 = 7;
    pub const RECENT_DAYS: i32 = 30;
    pub const AGING_DAYS: i32 = 90;

    pub fn of_age(age_days: Option<i32>) -> Freshness {
        match age_days {
            None => Freshness::Unknown,
            Some(a) if a <= Self::FRESH_DAYS => Freshness::Fresh,
            Some(a) if a <= Self::RECENT_DAYS => Freshness::Recent,
            Some(a) if a <= Self::AGING_DAYS => Freshness::Aging,
            Some(_) => Freshness::Stale,
        }
    }

    pub fn of(date: Option<Date>, today: Date) -> Freshness {
        Freshness::of_age(date.map(|d| today.since(d).max(0)))
    }
}

/// Kurze Altersangabe in höchstens 5 Zeichen: `heute`, `3T`, `2W`, `4M`, `1J`.
pub fn age_label(age_days: Option<i32>) -> String {
    match age_days {
        None => "–".to_string(),
        Some(a) if a <= 0 => "heute".to_string(),
        Some(a) if a < 14 => format!("{a}T"),
        Some(a) if a < 70 => format!("{}W", a / 7),
        Some(a) if a < 365 => format!("{}M", (a / 30).max(2)),
        Some(a) => format!("{}J", a / 365),
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NoteType {
    Overview,
    Topic,
    Source,
}

impl NoteType {
    pub fn label(self) -> &'static str {
        match self {
            NoteType::Overview => "Übersicht",
            NoteType::Topic => "Thema",
            NoteType::Source => "Quelle",
        }
    }
    pub fn mark(self) -> char {
        match self {
            NoteType::Overview => 'Ü',
            NoteType::Source => 'Q',
            NoteType::Topic => ' ',
        }
    }
}

/// Abschnittsarten nach Vertrag §4.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SectionKind {
    State,
    Open,
    Decisions,
    Lessons,
    Evidence,
    Sources,
    Paths,
    Topics,
    Other,
}

impl SectionKind {
    /// Reihenfolge und Buchstaben der Marken in der Projektkarte.
    pub const MARKS: [(SectionKind, char); 6] = [
        (SectionKind::State, 'S'),
        (SectionKind::Open, 'O'),
        (SectionKind::Decisions, 'E'),
        (SectionKind::Lessons, 'L'),
        (SectionKind::Evidence, 'P'),
        (SectionKind::Sources, 'Q'),
    ];

    pub fn bit(self) -> u8 {
        match self {
            SectionKind::State => 1,
            SectionKind::Open => 2,
            SectionKind::Decisions => 4,
            SectionKind::Lessons => 8,
            SectionKind::Evidence => 16,
            SectionKind::Sources => 32,
            SectionKind::Paths => 64,
            SectionKind::Topics => 128,
            SectionKind::Other => 0,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            SectionKind::State => "Stand",
            SectionKind::Open => "Offen",
            SectionKind::Decisions => "Entscheidungen",
            SectionKind::Lessons => "Lehren",
            SectionKind::Evidence => "Prüfungen",
            SectionKind::Sources => "Quellen",
            SectionKind::Paths => "Pfade",
            SectionKind::Topics => "Themen",
            SectionKind::Other => "",
        }
    }

    pub fn mark(self) -> char {
        Self::MARKS
            .iter()
            .find(|(k, _)| *k == self)
            .map(|(_, c)| *c)
            .unwrap_or('·')
    }
}

/// `status` als Aufzählung (Vertrag §13.5). Alles andere ist Freitext und gehört in `state`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Status {
    Active,
    Paused,
    Done,
    Superseded,
}

impl Status {
    pub fn parse(s: &str) -> Option<Status> {
        Some(match s.trim().to_ascii_lowercase().as_str() {
            "active" => Status::Active,
            "paused" => Status::Paused,
            "done" => Status::Done,
            "superseded" => Status::Superseded,
            _ => return None,
        })
    }
    pub fn label(self) -> &'static str {
        match self {
            Status::Active => "aktiv",
            Status::Paused => "pausiert",
            Status::Done => "fertig",
            Status::Superseded => "überholt",
        }
    }
}

/// Trennt einen Rohwert von `status` in Aufzählung und Freitext: ein Wert außerhalb der Aufzählung
/// (altes Wiki) wird zum Freitext, falls `state` fehlt.
pub fn split_status(status: Option<&str>, state: Option<&str>) -> (Option<Status>, Option<String>) {
    let nonempty = |s: &str| !s.trim().is_empty();
    let parsed = status.and_then(Status::parse);
    let free = state
        .filter(|s| nonempty(s))
        .or(if parsed.is_none() { status.filter(|s| nonempty(s)) } else { None })
        .map(|s| s.trim().to_string());
    (parsed, free)
}

/// Befunde aus `check` (Vertrag §5), als Anzahl je Art.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Findings {
    pub stale: u32,
    pub oversize: u32,
    pub dead_links: u32,
    pub orphans: u32,
    /// Befunde ab Warnung, die keine eigene Spalte haben (leere Notiz, doppeltes Ziel, Geheimnisverdacht,
    /// Frontmatter-Fehler …). Zählt zur Summe, steht aber nur als Zahl im Detail (Vertrag §18.5).
    pub other: u32,
}

impl Findings {
    pub fn total(&self) -> u32 {
        self.stale + self.oversize + self.dead_links + self.orphans + self.other
    }
    pub fn as_array(&self) -> [u32; 4] {
        [self.stale, self.oversize, self.dead_links, self.orphans]
    }
}

#[derive(Clone, Debug)]
pub struct ProjectInfo {
    pub name: String,
    pub notes: u32,
    pub bytes: u64,
    pub tokens: u32,
    pub last_change: Option<Date>,
    pub open_points: u32,
    pub findings: Findings,
    /// Journalzeilen je Woche, älteste zuerst, letzte = laufende Woche.
    pub weekly: Vec<u16>,
    /// `status` der Übersicht als Aufzählung.
    pub status: Option<Status>,
    /// Freitext (`state`) der Übersicht, nur für das Detailfeld.
    pub state: Option<String>,
    pub paths: Vec<String>,
    /// Vault-Adresse der Übersicht (ohne `.md`).
    pub overview: Option<String>,
}

impl ProjectInfo {
    pub fn activity(&self, last_weeks: usize) -> u32 {
        self.weekly
            .iter()
            .rev()
            .take(last_weeks)
            .map(|&v| v as u32)
            .sum()
    }
}

#[derive(Clone, Debug)]
pub struct NoteInfo {
    /// Vault-Adresse ohne `.md`, z. B. `Projects/Segelboot/00 Overview`.
    pub path: String,
    pub title: String,
    pub project: Option<String>,
    pub ntype: NoteType,
    pub status: Option<Status>,
    pub state: Option<String>,
    pub checked_on: Option<Date>,
    pub modified: Option<Date>,
    pub tokens: u32,
    pub bytes: u64,
    pub summary: String,
    /// Bitmaske der vorhandenen Abschnittsarten (`SectionKind::bit`).
    pub sections: u8,
    pub links_out: u32,
    pub links_in: u32,
    pub findings: Findings,
}

impl NoteInfo {
    pub fn has(&self, k: SectionKind) -> bool {
        self.sections & k.bit() != 0 && k.bit() != 0
    }
    pub fn superseded(&self) -> bool {
        self.status == Some(Status::Superseded)
    }
}

#[derive(Clone, Debug)]
pub struct Section {
    pub level: u8,
    pub title: String,
    pub kind: SectionKind,
    /// 1-basiert, einschließlich Frontmatter.
    pub line_start: u32,
    pub line_end: u32,
    pub tokens: u32,
}

#[derive(Clone, Debug)]
pub struct NoteDoc {
    pub info: NoteInfo,
    pub sections: Vec<Section>,
    pub text: String,
}

#[derive(Clone, Debug)]
pub struct SearchHit {
    pub path: String,
    pub title: String,
    pub project: Option<String>,
    pub ntype: NoteType,
    pub status: Option<Status>,
    pub modified: Option<Date>,
    pub section: Option<String>,
    pub line_start: u32,
    pub line_end: u32,
    pub tokens: u32,
    /// Ausschnitt (Satz oder Fenster), nie eine ganze Zeile.
    pub snippet: String,
    /// Trefferstellen als Byte-Bereiche in `snippet`.
    pub marks: Vec<(usize, usize)>,
}

#[derive(Clone, Debug)]
pub struct Link {
    pub path: String,
    pub title: String,
    pub project: Option<String>,
    pub ntype: NoteType,
    pub status: Option<Status>,
    pub modified: Option<Date>,
}

#[derive(Clone, Debug, Default)]
pub struct NoteLinks {
    pub out: Vec<Link>,
    pub inc: Vec<Link>,
}

#[derive(Clone, Debug)]
pub struct ProjectEdge {
    pub a: String,
    pub b: String,
    /// Querverweise zwischen den Projekten (Richtung egal).
    pub links: u32,
    /// Quellen, die beide Projekte verlinken.
    pub shared_sources: u32,
}

impl ProjectEdge {
    pub fn weight(&self) -> u32 {
        self.links + self.shared_sources
    }
}

/// Schätzung wie in der Agent-Ebene; nur für Anzeige, wenn die Quelle nichts liefert.
pub fn tokens_of_bytes(bytes: u64) -> u32 {
    (bytes / 4) as u32
}

/// `1.3M`, `250K`, `812B` in höchstens 5 Zeichen.
pub fn bytes_short(b: u64) -> String {
    if b < 1000 {
        format!("{b}B")
    } else if b < 1_000_000 {
        format!("{}K", (b + 500) / 1000)
    } else if b < 100_000_000 {
        format!("{:.1}M", b as f64 / 1_000_000.0)
    } else {
        format!("{}M", b / 1_000_000)
    }
}

/// `~310`, `~3.2k`, `~41k`.
pub fn tokens_short(t: u32) -> String {
    if t < 1000 {
        format!("~{t}")
    } else if t < 10_000 {
        format!("~{:.1}k", t as f64 / 1000.0)
    } else {
        format!("~{}k", t / 1000)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn date_roundtrip() {
        for (y, m, d) in [(1970, 1, 1), (2000, 2, 29), (2026, 11, 9), (2024, 12, 31), (1969, 12, 31)] {
            let dt = Date::from_ymd(y, m, d);
            assert_eq!(dt.ymd(), (y, m, d));
        }
        assert_eq!(Date::from_ymd(1970, 1, 1).0, 0);
    }

    #[test]
    fn weekday_known() {
        // 2026-11-09 ist ein Montag.
        assert_eq!(Date::from_ymd(2026, 11, 9).weekday(), 0);
        assert_eq!(Date::from_ymd(2026, 11, 15).weekday(), 6);
        assert_eq!(Date::from_ymd(1970, 1, 1).weekday(), 3);
    }

    #[test]
    fn date_parse_and_display() {
        let d = Date::parse("2026-10-16 12:00").unwrap();
        assert_eq!(d.to_string(), "2026-10-16");
        assert!(Date::parse("2026-13-01").is_none());
        assert!(Date::parse("kaputt").is_none());
        assert_eq!(Date::parse("2026-11-09").unwrap().since(Date::parse("2026-11-02").unwrap()), 7);
    }

    #[test]
    fn freshness_thresholds() {
        assert_eq!(Freshness::of_age(Some(0)), Freshness::Fresh);
        assert_eq!(Freshness::of_age(Some(7)), Freshness::Fresh);
        assert_eq!(Freshness::of_age(Some(8)), Freshness::Recent);
        assert_eq!(Freshness::of_age(Some(31)), Freshness::Aging);
        assert_eq!(Freshness::of_age(Some(91)), Freshness::Stale);
        assert_eq!(Freshness::of_age(None), Freshness::Unknown);
    }

    #[test]
    fn labels() {
        assert_eq!(age_label(Some(0)), "heute");
        assert_eq!(age_label(Some(3)), "3T");
        assert_eq!(age_label(Some(21)), "3W");
        assert_eq!(age_label(Some(120)), "4M");
        assert_eq!(age_label(Some(800)), "2J");
        assert_eq!(bytes_short(1_340_170), "1.3M");
        assert_eq!(bytes_short(250_159), "250K");
        assert_eq!(tokens_short(3200), "~3.2k");
    }
}
