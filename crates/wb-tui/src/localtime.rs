//! Ortszeit nach der Regel des jeweiligen Datums (Sommerzeit), Vertrag §16.8 und §18.6.
//!
//! Windows beschreibt eine Zeitzone durch `TIME_ZONE_INFORMATION`: Abstand zu UTC, Abstände der
//! Normal- und Sommerzeit und je eine Übergangsregel („letzter Sonntag im März, 2 Uhr“). Die
//! Umrechnung ist hier eine reine Funktion dieser Regel, damit sie ohne Rücksicht auf die Zone
//! des Rechners prüfbar ist. Ein einziger gemessener Abstand für alle Zeitstempel verschiebt
//! Sommerzeit-Daten zwischen 0 und 1 Uhr auf den Vortag.

use crate::model::Date;

/// Übergang als Regel des Monats (wie `SYSTEMTIME` in `TIME_ZONE_INFORMATION`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Transition {
    /// 1–12; 0 = kein Übergang.
    pub month: u8,
    /// 1–4 = erste bis vierte Woche, 5 = letzte.
    pub week: u8,
    /// 0 = Sonntag … 6 = Samstag.
    pub weekday: u8,
    pub hour: u8,
    pub minute: u8,
}

/// Zeitzonenregel mit Windows-Vorzeichen: UTC = Ortszeit + Abstand (in Minuten).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TzRule {
    pub bias: i32,
    pub standard_bias: i32,
    pub daylight_bias: i32,
    /// Wechsel zur Normalzeit, angegeben in Sommerzeit.
    pub standard_start: Transition,
    /// Wechsel zur Sommerzeit, angegeben in Normalzeit.
    pub daylight_start: Transition,
}

impl TzRule {
    /// Mitteleuropa: MEZ/MESZ (Berlin).
    pub const CENTRAL_EUROPE: TzRule = TzRule {
        bias: -60,
        standard_bias: 0,
        daylight_bias: -60,
        standard_start: Transition { month: 10, week: 5, weekday: 0, hour: 3, minute: 0 },
        daylight_start: Transition { month: 3, week: 5, weekday: 0, hour: 2, minute: 0 },
    };

    /// Ohne Sommerzeit, fester Abstand zu UTC in Sekunden.
    pub fn fixed(offset_secs: i64) -> TzRule {
        let none = Transition { month: 0, week: 0, weekday: 0, hour: 0, minute: 0 };
        TzRule { bias: -(offset_secs / 60) as i32, standard_bias: 0, daylight_bias: 0, standard_start: none, daylight_start: none }
    }

    /// Abstand der Ortszeit zu UTC in Sekunden zum UTC-Zeitpunkt `utc` (nach der Regel dieses Zeitpunkts).
    pub fn offset_at(&self, utc: i64) -> i64 {
        let has_dst = self.standard_start.month != 0 && self.daylight_start.month != 0;
        let mut bias = self.bias + self.standard_bias;
        if has_dst {
            // Jahr der Normalzeit-Ortszeit genügt: Übergänge liegen nie um den Jahreswechsel
            let year = Date((utc - bias as i64 * 60).div_euclid(86_400) as i32).ymd().0;
            let to_dst = transition_local(year, self.daylight_start) + (self.bias + self.standard_bias) as i64 * 60;
            let to_std = transition_local(year, self.standard_start) + (self.bias + self.daylight_bias) as i64 * 60;
            let in_dst = if to_dst < to_std { utc >= to_dst && utc < to_std } else { !(utc >= to_std && utc < to_dst) };
            if in_dst {
                bias = self.bias + self.daylight_bias;
            }
        }
        -(bias as i64) * 60
    }

    /// Kalendertag (Ortszeit) zum Unix-Zeitstempel.
    pub fn day_of(&self, utc: i64) -> Date {
        Date((utc + self.offset_at(utc)).div_euclid(86_400) as i32)
    }
}

/// Sekunden seit 1970 (Wanduhr-Zeit, nicht UTC) des Übergangs im Jahr `year`.
fn transition_local(year: i32, t: Transition) -> i64 {
    let first = Date::from_ymd(year, t.month as u32, 1);
    let first_wd = (first.0 + 4).rem_euclid(7); // 1970-01-01 war ein Donnerstag; 0 = Sonntag
    let mut day = 1 + (t.weekday as i32 - first_wd).rem_euclid(7) + 7 * (t.week.clamp(1, 5) as i32 - 1);
    let next = if t.month == 12 { Date::from_ymd(year + 1, 1, 1) } else { Date::from_ymd(year, t.month as u32 + 1, 1) };
    let dim = next.0 - first.0;
    while day > dim {
        day -= 7;
    }
    (first.0 as i64 + day as i64 - 1) * 86_400 + t.hour as i64 * 3600 + t.minute as i64 * 60
}

/// Regel des Rechners (Windows: `GetTimeZoneInformation`), sonst UTC.
pub fn system_rule() -> TzRule {
    static RULE: std::sync::OnceLock<TzRule> = std::sync::OnceLock::new();
    *RULE.get_or_init(read_system_rule)
}

#[cfg(windows)]
fn read_system_rule() -> TzRule {
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct SystemTime {
        year: u16,
        month: u16,
        day_of_week: u16,
        day: u16,
        hour: u16,
        minute: u16,
        second: u16,
        millis: u16,
    }
    #[repr(C)]
    struct TimeZoneInformation {
        bias: i32,
        standard_name: [u16; 32],
        standard_date: SystemTime,
        standard_bias: i32,
        daylight_name: [u16; 32],
        daylight_date: SystemTime,
        daylight_bias: i32,
    }
    extern "system" {
        fn GetTimeZoneInformation(info: *mut TimeZoneInformation) -> u32;
    }
    // SAFETY: Struktur wie TIME_ZONE_INFORMATION der Windows-API; die Funktion schreibt nur hinein.
    let info = unsafe {
        let mut info: TimeZoneInformation = std::mem::zeroed();
        GetTimeZoneInformation(&mut info);
        info
    };
    let tr = |s: SystemTime| Transition { month: s.month as u8, week: s.day as u8, weekday: s.day_of_week as u8, hour: s.hour as u8, minute: s.minute as u8 };
    TzRule {
        bias: info.bias,
        standard_bias: info.standard_bias,
        daylight_bias: info.daylight_bias,
        standard_start: tr(info.standard_date),
        daylight_start: tr(info.daylight_date),
    }
}

#[cfg(not(windows))]
fn read_system_rule() -> TzRule {
    TzRule::fixed(0)
}

/// Kalendertag (Ortszeit des Rechners, Regel des jeweiligen Datums) zum Unix-Zeitstempel.
pub fn local_day(utc: i64) -> Date {
    system_rule().day_of(utc)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utc(y: i32, m: u32, d: u32, h: i64, min: i64) -> i64 {
        Date::from_ymd(y, m, d).0 as i64 * 86_400 + h * 3600 + min * 60
    }

    #[test]
    fn offsets_follow_the_rule_of_the_date() {
        let z = TzRule::CENTRAL_EUROPE;
        assert_eq!(z.offset_at(utc(2026, 1, 15, 12, 0)), 3600);
        assert_eq!(z.offset_at(utc(2026, 7, 15, 12, 0)), 7200);
        // Sommerzeit beginnt 2026-03-29 um 01:00 UTC, endet 2026-10-25 um 01:00 UTC
        assert_eq!(z.offset_at(utc(2026, 3, 29, 0, 59)), 3600);
        assert_eq!(z.offset_at(utc(2026, 3, 29, 1, 0)), 7200);
        assert_eq!(z.offset_at(utc(2026, 10, 25, 0, 59)), 7200);
        assert_eq!(z.offset_at(utc(2026, 10, 25, 1, 0)), 3600);
    }

    #[test]
    fn half_past_midnight_in_summer_stays_on_the_local_day() {
        let z = TzRule::CENTRAL_EUROPE;
        // 2026-08-01 00:30 MESZ = 2026-07-31 22:30 UTC
        assert_eq!(z.day_of(utc(2026, 7, 31, 22, 30)), Date::from_ymd(2026, 8, 1));
        // mit einem festen Winterabstand (+1 h, dem Fehler vor §18.6) bliebe es der 31. Juli
        assert_eq!(TzRule::fixed(3600).day_of(utc(2026, 7, 31, 22, 30)), Date::from_ymd(2026, 7, 31));
        // 2026-11-10 00:30 MEZ = 2026-11-09 23:30 UTC
        assert_eq!(z.day_of(utc(2026, 11, 9, 23, 30)), Date::from_ymd(2026, 11, 10));
        // kurz vor Mitternacht Ortszeit bleibt am Vortag
        assert_eq!(z.day_of(utc(2026, 7, 31, 21, 59)), Date::from_ymd(2026, 7, 31));
    }

    #[test]
    fn southern_hemisphere_and_no_dst() {
        // Sydney: Sommerzeit von Oktober bis April (Normalzeit +10, Sommerzeit +11)
        let syd = TzRule {
            bias: -600,
            standard_bias: 0,
            daylight_bias: -60,
            standard_start: Transition { month: 4, week: 1, weekday: 0, hour: 3, minute: 0 },
            daylight_start: Transition { month: 10, week: 1, weekday: 0, hour: 2, minute: 0 },
        };
        assert_eq!(syd.offset_at(utc(2026, 1, 15, 0, 0)), 11 * 3600);
        assert_eq!(syd.offset_at(utc(2026, 7, 15, 0, 0)), 10 * 3600);
        assert_eq!(TzRule::fixed(0).offset_at(utc(2026, 7, 15, 0, 0)), 0);
    }
}
