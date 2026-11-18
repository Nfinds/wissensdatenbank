//! Schreibbefehle `wb new`, `wb log`, `wb mv`: eine Kopfzeile mit dem Ergebnis, dann Einzelheiten.

use std::io::Read;

use wb_core::*;

use crate::fmt::{CmdLine, Ctx, clean, json};
use crate::{LogArgs, Mode, NewArgs};

type Res = std::result::Result<i32, Error>;

fn line(out: &mut Vec<u8>, s: &str) {
    out.extend_from_slice(clean(s).as_bytes());
    out.push(b'\n');
}

pub(crate) fn new(out: &mut Vec<u8>, ctx: &Ctx, v: &Vault, a: &NewArgs) -> Res {
    let mode = ctx.mode;
    let note_type = match a.note_type.as_str() {
        "topic" => NoteType::Topic,
        "overview" => NoteType::Overview,
        "source" => NoteType::Source,
        t => return Err(Error::Usage { message: format!("unbekannte Notizart: {t}"), hint: Some("topic | overview | source".into()) }),
    };
    let status = match &a.status {
        None => None,
        Some(s) => Some(NoteStatus::parse(s).ok_or_else(|| Error::Usage {
            message: format!("unbekannter status: {s}"),
            hint: Some("active | paused | done | superseded".into()),
        })?),
    };
    let r = v.new_note(&NewNote {
        title: a.title.clone(),
        note_type,
        project: a.project.clone(),
        status,
        date: a.date.clone(),
        paths: a.paths.clone(),
        source_url: a.url.clone(),
        summary: a.summary.clone(),
        no_link: a.no_link,
    })?;
    if mode == Mode::Json {
        json(out, &r);
        return Ok(exit::OK);
    }
    line(out, &format!("created {}", r.path));
    line(out, &format!("link: {}", r.link));
    // Themennotiz: in der Übersicht eingetragen (`linked:`), sonst der Eintrag zum Nachtragen.
    match (&r.overview, &r.entry, r.linked_line) {
        (Some(o), _, Some(l)) => line(out, &format!("linked: {o} L{l}")),
        (Some(o), Some(e), None) => line(out, &format!("next: in {o} unter Themen verlinken: {e}")),
        _ => {}
    }
    Ok(exit::OK)
}

pub(crate) fn log(out: &mut Vec<u8>, ctx: &Ctx, v: &Vault, a: &LogArgs) -> Res {
    let mode = ctx.mode;
    if v.is_read_only() {
        return Err(Error::ReadOnly { action: "wb log".into(), hint: None });
    }
    let text = if a.text.is_empty() {
        let mut s = String::new();
        std::io::stdin().read_to_string(&mut s).map_err(|e| Error::Io { path: "stdin".into(), source: e })?;
        s
    } else {
        a.text.join(" ")
    };
    let time = a.time.as_ref().map(|t| if t == "now" { local_time() } else { t.clone() });
    let r = v.append_journal(&NewJournalEntry { text, note: a.note.clone(), date: a.date.clone(), time })?;
    v.sync()?;
    if mode == Mode::Json {
        json(out, &r);
        return Ok(exit::OK);
    }
    line(out, &format!("logged {} L{}", r.file, r.line));
    line(out, &format!("  {}", r.text));
    Ok(exit::OK)
}

pub(crate) fn mv(out: &mut Vec<u8>, ctx: &Ctx, v: &Vault, from: &str, to: &str) -> Res {
    let mode = ctx.mode;
    let r = v.move_note(from, to)?;
    if mode == Mode::Json {
        json(out, &r);
        return Ok(exit::OK);
    }
    line(out, &format!("moved {} -> {} links={} notes={}", r.from, r.to, r.links, r.changed.len()));
    for (p, n) in &r.changed {
        line(out, &format!("  {} {n}", p));
    }
    let mut c = CmdLine::new(&Ctx { mode: Mode::Agent, color: false, global: ctx.global.clone() }, "read");
    c.positional(r.to.without_ext());
    for l in c.lines("next") {
        line(out, &l);
    }
    Ok(exit::OK)
}
