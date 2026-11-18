//! `DataSource` über einen offenen `wb_core::Vault` (Feature `core`).
//!
//! Die Oberfläche liest nur. Teure Gesamtauswertungen (Befunde, Projektkanten) werden beim
//! ersten Bedarf einmal berechnet und bis `reload` behalten. Fehler des Kerns führen zu leeren
//! Ergebnissen; der letzte Fehlertext steht in `last_error`.

use crate::model::{self, Date, Findings, SectionKind as Sk};
use crate::source::DataSource;
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use wb_core as core;

#[derive(Default)]
struct Cache {
    findings: Option<HashMap<String, Findings>>,
    edges: Option<Vec<model::ProjectEdge>>,
    sources: Option<u32>,
}

pub struct CoreSource<'v> {
    vault: &'v core::Vault,
    today: Cell<Date>,
    /// „Heute“ ist festgelegt (Tests, Schnappschüsse) und folgt der Uhr nicht.
    fixed_today: bool,
    cache: RefCell<Cache>,
    error: RefCell<Option<String>>,
}

/// Kalendertag (Ortszeit, Sommerzeitregel des jeweiligen Datums, Vertrag §18.6) zu einem Unix-Zeitstempel.
fn local_day(secs: i64) -> i32 {
    crate::localtime::local_day(secs).0
}

fn date_of_secs(secs: i64) -> Option<Date> {
    if secs <= 0 {
        return None;
    }
    Some(Date(local_day(secs)))
}

fn system_today() -> Date {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    Date(local_day(secs))
}

fn ntype(t: core::NoteType) -> model::NoteType {
    match t {
        core::NoteType::Overview => model::NoteType::Overview,
        core::NoteType::Source => model::NoteType::Source,
        // Vertrag §13.3: nur Übersicht, Thema und Quelle; alles andere zeigt die Oberfläche als Thema
        _ => model::NoteType::Topic,
    }
}

fn kind(k: core::SectionKind) -> Sk {
    match k {
        core::SectionKind::State => Sk::State,
        core::SectionKind::Open => Sk::Open,
        core::SectionKind::Decisions => Sk::Decisions,
        core::SectionKind::Lessons => Sk::Lessons,
        core::SectionKind::Evidence => Sk::Evidence,
        core::SectionKind::Sources => Sk::Sources,
        core::SectionKind::Paths => Sk::Paths,
        core::SectionKind::Topics => Sk::Topics,
        core::SectionKind::Other => Sk::Other,
    }
}

/// Vault-Adresse der Oberfläche (ohne `.md`) → Pfad des Kerns.
fn to_core_path(path: &str) -> core::NotePath {
    core::NotePath::new(format!("{path}.md"))
}

impl<'v> CoreSource<'v> {
    pub fn new(vault: &'v core::Vault) -> Self {
        let mut s = Self::with_today(vault, system_today());
        s.fixed_today = false;
        s
    }

    /// „Heute“ festlegen (Tests und reproduzierbare Schnappschüsse).
    pub fn with_today(vault: &'v core::Vault, today: Date) -> Self {
        CoreSource { vault, today: Cell::new(today), fixed_today: true, cache: RefCell::new(Cache::default()), error: RefCell::new(None) }
    }

    pub fn last_error(&self) -> Option<String> {
        self.error.borrow().clone()
    }

    /// Verwirft die Gesamtauswertungen (nach `vault.sync()` aufrufen).
    pub fn invalidate(&self) {
        *self.cache.borrow_mut() = Cache::default();
    }

    fn note_error<T>(&self, r: core::Result<T>) -> Option<T> {
        match r {
            Ok(v) => Some(v),
            Err(e) => {
                *self.error.borrow_mut() = Some(e.to_string());
                None
            }
        }
    }

    fn findings_map(&self) -> HashMap<String, Findings> {
        if let Some(m) = &self.cache.borrow().findings {
            return m.clone();
        }
        let opts = core::CheckOptions { prefix: None, today: Some(self.today.get().to_string()), stale_days: Some(60) };
        let mut map: HashMap<String, Findings> = HashMap::new();
        if let Some(list) = self.note_error(self.vault.check(&opts)) {
            for f in list {
                let e = map.entry(f.path.without_ext().to_string()).or_default();
                match f.code.as_str() {
                    "stale" => e.stale += 1,
                    // zu lange Übersicht zählt wie eine zu große Notiz unter G
                    "note-large" | "overview-long" => e.oversize += 1,
                    "link-dead" => e.dead_links += 1,
                    "orphan" => e.orphans += 1,
                    // alles Übrige ab Warnung zählt mit, nur Hinweise (Info) ohne eigene Spalte nicht
                    _ if f.severity != core::Severity::Info => e.other += 1,
                    _ => {}
                }
            }
        }
        self.cache.borrow_mut().findings = Some(map.clone());
        map
    }

    fn info(&self, m: &core::NoteMeta, mask: u8, findings: &HashMap<String, Findings>) -> model::NoteInfo {
        let key = m.path.without_ext().to_string();
        model::NoteInfo {
            title: m.title.clone(),
            project: m.project.clone(),
            ntype: ntype(m.note_type),
            status: model::split_status(m.status.as_deref(), m.state.as_deref()).0,
            state: model::split_status(m.status.as_deref(), m.state.as_deref()).1,
            checked_on: m.checked_on.as_deref().and_then(Date::parse),
            modified: date_of_secs(m.mtime),
            tokens: m.tokens,
            bytes: m.bytes,
            summary: m.summary.clone().unwrap_or_default(),
            sections: mask,
            links_out: m.links_out,
            links_in: m.links_in,
            findings: findings.get(&key).copied().unwrap_or_default(),
            path: key,
        }
    }

    fn link_of(&self, p: &core::NotePath) -> Option<model::Link> {
        let m = self.vault.meta(p).ok()?;
        Some(model::Link {
            path: m.path.without_ext().to_string(),
            title: m.title.clone(),
            project: m.project.clone(),
            ntype: ntype(m.note_type),
            status: model::split_status(m.status.as_deref(), None).0,
            modified: date_of_secs(m.mtime),
        })
    }

    fn compute_edges(&self) -> Vec<model::ProjectEdge> {
        let Some(all) = self.note_error(self.vault.all_links()) else {
            return Vec::new();
        };
        let mut links: BTreeMap<(String, String), u32> = BTreeMap::new();
        let mut source_users: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for li in all {
            let Some(from) = li.source.project().map(str::to_string) else { continue };
            let core::LinkStatus::Ok(target) = li.status else { continue };
            match target.project() {
                Some(to) if to != from => {
                    let key = if from.as_str() < to { (from.clone(), to.to_string()) } else { (to.to_string(), from.clone()) };
                    *links.entry(key).or_default() += 1;
                }
                Some(_) => {}
                None if target.as_str().starts_with("Sources/") => {
                    source_users.entry(target.as_str().to_string()).or_default().insert(from);
                }
                None => {}
            }
        }
        let mut shared: BTreeMap<(String, String), u32> = BTreeMap::new();
        for users in source_users.values() {
            let v: Vec<&String> = users.iter().collect();
            for i in 0..v.len() {
                for j in i + 1..v.len() {
                    *shared.entry((v[i].clone(), v[j].clone())).or_default() += 1;
                }
            }
        }
        let keys: BTreeSet<(String, String)> = links.keys().chain(shared.keys()).cloned().collect();
        keys.into_iter()
            .map(|k| model::ProjectEdge {
                links: links.get(&k).copied().unwrap_or(0),
                shared_sources: shared.get(&k).copied().unwrap_or(0),
                a: k.0,
                b: k.1,
            })
            .collect()
    }
}

impl DataSource for CoreSource<'_> {
    fn today(&self) -> Date {
        self.today.get()
    }

    fn refresh(&self) {
        // Nur-Lese-Modus: der Index liegt außerhalb des Vaults, das Angleichen ist erlaubt (§16.2)
        self.note_error(self.vault.sync());
        if !self.fixed_today {
            self.today.set(system_today());
        }
        self.invalidate();
    }

    /// Ausgangsliste der Suche aus einer einzigen Abfrage; die Abschnittsmaske bleibt leer, die
    /// Liste braucht sie nicht (sonst würde jede Notiz je Tastendruck von der Platte gelesen).
    fn recent(&self, limit: usize) -> Vec<model::NoteInfo> {
        let Some(mut list) = self.note_error(self.vault.notes(&core::NoteFilter::default())) else { return Vec::new() };
        list.sort_by(|a, b| b.mtime.cmp(&a.mtime).then_with(|| a.path.cmp(&b.path)));
        list.truncate(limit);
        let findings = self.findings_map();
        list.iter().map(|m| self.info(m, 0, &findings)).collect()
    }

    fn label(&self) -> String {
        let name = self.vault.root().file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "vault".into());
        if self.vault.is_read_only() {
            format!("{name} (nur lesen)")
        } else {
            name
        }
    }

    fn projects(&self) -> Vec<model::ProjectInfo> {
        let Some(list) = self.note_error(self.vault.projects()) else { return Vec::new() };
        let findings = self.findings_map();
        let monday = self.today.get().add(-(self.today.get().weekday() as i32));
        list.into_iter()
            .map(|p| {
                let mut f = Findings::default();
                for (path, x) in &findings {
                    if path.strip_prefix("Projects/").and_then(|r| r.split('/').next()) == Some(p.name.as_str()) {
                        f.stale += x.stale;
                        f.oversize += x.oversize;
                        f.dead_links += x.dead_links;
                        f.orphans += x.orphans;
                        f.other += x.other;
                    }
                }
                let by_week: HashMap<i32, u32> =
                    p.activity.iter().filter_map(|w| Some((Date::parse(&w.week)?.0, w.count))).collect();
                let weekly: Vec<u16> =
                    (0..12).map(|k| by_week.get(&monday.add(-7 * (11 - k)).0).copied().unwrap_or(0).min(u16::MAX as u32) as u16).collect();
                let state_meta = p.overview.as_ref().and_then(|o| self.vault.meta(o).ok()).and_then(|m| m.state);
                let (status, state) = model::split_status(p.status.as_deref(), state_meta.as_deref());
                model::ProjectInfo {
                    notes: p.notes,
                    bytes: p.bytes,
                    tokens: p.tokens.min(u32::MAX as u64) as u32,
                    last_change: date_of_secs(p.last_modified),
                    open_points: p.open_items,
                    findings: f,
                    weekly,
                    status,
                    state,
                    paths: p.paths.clone(),
                    overview: p.overview.as_ref().map(|o| o.without_ext().to_string()),
                    name: p.name,
                }
            })
            .collect()
    }

    fn notes(&self, project: &str) -> Vec<model::NoteInfo> {
        let filter = core::NoteFilter { project: Some(project.to_string()), ..Default::default() };
        let Some(list) = self.note_error(self.vault.notes(&filter)) else { return Vec::new() };
        let findings = self.findings_map();
        list.iter()
            .map(|m| {
                let mask = self
                    .vault
                    .note(&m.path)
                    .map(|n| n.sections.iter().fold(0u8, |a, s| a | kind(s.kind).bit()))
                    .unwrap_or(0);
                self.info(m, mask, &findings)
            })
            .collect()
    }

    fn note(&self, path: &str) -> Option<model::NoteDoc> {
        let n = self.note_error(self.vault.note(&to_core_path(path)))?;
        let findings = self.findings_map();
        let mask = n.sections.iter().fold(0u8, |a, s| a | kind(s.kind).bit());
        let info = self.info(&n.meta, mask, &findings);
        let sections = n
            .sections
            .iter()
            .filter(|s| s.level > 0)
            .map(|s| model::Section {
                level: s.level,
                title: s.title.clone(),
                kind: kind(s.kind),
                line_start: s.line_start,
                line_end: s.line_end,
                tokens: s.tokens,
            })
            .collect();
        Some(model::NoteDoc { info, sections, text: n.text })
    }

    fn search(&self, query: &str, limit: usize) -> Vec<model::SearchHit> {
        if query.trim().is_empty() {
            return Vec::new();
        }
        let q = core::SearchQuery { limit, per_note: 1, ..core::SearchQuery::new(query) };
        // Eine Eingabe ohne verwertbare Begriffe (ein einzelnes Zeichen beim Tippen) ist kein Fehler des Kerns
        let res = match self.vault.search(&q) {
            Err(core::Error::Usage { .. }) => return Vec::new(),
            r => r,
        };
        let Some(res) = self.note_error(res) else { return Vec::new() };
        res.hits
            .into_iter()
            .map(|h| {
                let meta = self.vault.meta(&h.path).ok();
                model::SearchHit {
                    path: h.path.without_ext().to_string(),
                    title: meta.as_ref().map(|m| m.title.clone()).unwrap_or_else(|| h.path.stem().to_string()),
                    project: h.project,
                    ntype: ntype(h.note_type),
                    status: meta.as_ref().and_then(|m| model::split_status(m.status.as_deref(), None).0),
                    modified: meta.as_ref().and_then(|m| date_of_secs(m.mtime)),
                    section: if h.section_title.is_empty() { None } else { Some(h.section_title) },
                    line_start: h.line_start,
                    line_end: h.line_end,
                    tokens: h.tokens,
                    snippet: h.snippet,
                    marks: h.highlights,
                }
            })
            .collect()
    }

    fn links(&self, path: &str) -> model::NoteLinks {
        let Some(l) = self.note_error(self.vault.links(&to_core_path(path))) else { return Default::default() };
        let mut out = Vec::new();
        let mut seen = BTreeSet::new();
        for li in l.outgoing {
            if let core::LinkStatus::Ok(t) = li.status {
                if seen.insert(t.clone()) {
                    if let Some(x) = self.link_of(&t) {
                        out.push(x);
                    }
                }
            }
        }
        let mut inc = Vec::new();
        let mut seen = BTreeSet::new();
        for li in l.incoming {
            if seen.insert(li.source.clone()) {
                if let Some(x) = self.link_of(&li.source) {
                    inc.push(x);
                }
            }
        }
        model::NoteLinks { out, inc }
    }

    fn activity(&self, days: u32) -> Vec<u16> {
        let since = self.today.get().add(-(days as i32 - 1));
        let f = core::ActivityFilter { project: None, since: Some(since.to_string()), until: Some(self.today.get().to_string()) };
        let mut per_day: HashMap<i32, u32> = HashMap::new();
        if let Some(list) = self.note_error(self.vault.activity(&f)) {
            for d in list {
                if let Some(day) = Date::parse(&d.date) {
                    *per_day.entry(day.0).or_default() += d.count;
                }
            }
        }
        (0..days as i32).map(|i| per_day.get(&(since.0 + i)).copied().unwrap_or(0).min(u16::MAX as u32) as u16).collect()
    }

    fn project_edges(&self) -> Vec<model::ProjectEdge> {
        if let Some(e) = &self.cache.borrow().edges {
            return e.clone();
        }
        let e = self.compute_edges();
        self.cache.borrow_mut().edges = Some(e.clone());
        e
    }

    fn sources(&self) -> u32 {
        if let Some(n) = self.cache.borrow().sources {
            return n;
        }
        let filter = core::NoteFilter { note_type: Some(core::NoteType::Source), ..Default::default() };
        let n = self.note_error(self.vault.notes(&filter)).map(|v| v.len() as u32).unwrap_or(0);
        self.cache.borrow_mut().sources = Some(n);
        n
    }

    fn file_path(&self, path: &str) -> Option<std::path::PathBuf> {
        Some(self.vault.file_path(&to_core_path(path)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_time_decides_the_calendar_day() {
        // „heute“ und die Änderungszeit eines Eintrags von jetzt liegen auf demselben Tag
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
        assert_eq!(date_of_secs(now).unwrap(), system_today());
        // die Regel des Rechners ist plausibel (Abstand zu UTC höchstens 14 Stunden, Viertelstundenraster)
        let off = crate::localtime::system_rule().offset_at(now);
        assert!(off.abs() <= 14 * 3600 && off % 900 == 0, "Abstand zu UTC unplausibel: {off}");
    }
}
