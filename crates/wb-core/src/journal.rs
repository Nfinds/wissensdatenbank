//! Journalzeilen lesen: `- JJJJ-MM-TT HH:MM [[Notiz]] Text` (neu) und
//! `- JJJJ-MM-TT: [[Notiz]] Text` (altes `System/Wiki Log.md`).

use crate::model::{JournalEntry, NotePath, path_key};

fn is_date(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && b.iter().enumerate().all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
}

fn is_time(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 5 && b[2] == b':' && b[0].is_ascii_digit() && b[1].is_ascii_digit() && b[3].is_ascii_digit() && b[4].is_ascii_digit()
}

/// Eine Zeile lesen; `None`, wenn sie kein Journaleintrag ist.
pub fn parse_line(file: &NotePath, line_no: u32, line: &str) -> Option<JournalEntry> {
    let t = line.trim_end_matches('\r').trim_start();
    let rest = t.strip_prefix("- ").or_else(|| t.strip_prefix("* "))?;
    let rest = rest.trim_start();
    if rest.len() < 10 || !rest.is_char_boundary(10) || !is_date(&rest[..10]) {
        return None;
    }
    let date = rest[..10].to_string();
    let mut r = rest[10..].trim_start_matches(['T', ' ']);
    let mut time = None;
    if r.len() >= 5 && r.is_char_boundary(5) && is_time(&r[..5]) {
        time = Some(r[..5].to_string());
        r = &r[5..];
    }
    let r = r.trim_start_matches([':', ' ', '\u{2013}', '-']).trim_start();
    let links = crate::links::parse_wikilinks(r);
    let target = links.first().map(|l| l.target.clone());
    let project = links.iter().find_map(|l| {
        let k = NotePath::new(&l.target);
        k.project().map(str::to_string).or_else(|| {
            let key = path_key(&l.target);
            key.strip_prefix("projects/").map(|rest| {
                let name = rest.split('/').next().unwrap_or(rest);
                // Groß/Klein wie im Linktext
                let start = "Projects/".len();
                l.target.get(start..start + name.len()).unwrap_or(name).to_string()
            })
        })
    });
    Some(JournalEntry { date, time, target, project, text: r.to_string(), file: file.clone(), line: line_no })
}

/// Alle Einträge eines Journaltextes in Dateireihenfolge.
pub fn parse_journal(file: &NotePath, text: &str) -> Vec<JournalEntry> {
    text.split('\n').enumerate().filter_map(|(i, l)| parse_line(file, i as u32 + 1, l)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_and_new_format() {
        let f = NotePath::new("System/Wiki Log.md");
        let e = parse_line(&f, 3, "- 2026-11-09: [[Projects/Beispiel Werk/Thema Eins|Eins]] angelegt").unwrap();
        assert_eq!(e.date, "2026-11-09");
        assert_eq!(e.time, None);
        assert_eq!(e.project.as_deref(), Some("Beispiel Werk"));
        assert_eq!(e.target.as_deref(), Some("Projects/Beispiel Werk/Thema Eins"));
        let e = parse_line(&f, 4, "- 2026-11-10 14:05 [[Sources/Web/X]] Text").unwrap();
        assert_eq!(e.time.as_deref(), Some("14:05"));
        assert_eq!(e.project, None);
        assert!(parse_line(&f, 5, "- kein Datum").is_none());
        assert!(parse_line(&f, 6, "Text 2026-11-09").is_none());
    }
}
