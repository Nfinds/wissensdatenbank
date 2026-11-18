//! Synthetische Datenquelle. Erfundene Projekte, erfundene Texte, eigene deterministische
//! Zufallsfolge. Enthält nichts aus dem echten Wiki (Vertrag K12), bildet aber dessen Form nach:
//! ungleich große Projekte, Übersichten als Naben, Quellen mit Mehrfachverwendung, Absätze als
//! eine lange Zeile, ein paar tote Links, Waisen und veraltete Notizen.

use crate::md::{link_label, wikilinks};
use crate::model::*;
use crate::source::DataSource;
use std::collections::{BTreeMap, HashMap};

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next() % n as u64) as usize
        }
    }
    fn chance(&mut self, p: f64) -> bool {
        (self.next() % 10_000) as f64 / 10_000.0 < p
    }
    fn pick<'a, T>(&mut self, s: &'a [T]) -> &'a T {
        &s[self.below(s.len())]
    }
    fn range(&mut self, lo: usize, hi: usize) -> usize {
        lo + self.below(hi - lo + 1)
    }
}

struct Spec {
    name: &'static str,
    topics: usize,
    activity: u32,
    age: i32,
    status: &'static str,
}

const SPECS: &[Spec] = &[
    Spec { name: "Segelboot-Restaurierung", topics: 58, activity: 9, age: 0, status: "rumpf geschliffen, lackierung offen" },
    Spec { name: "Gartenplaner", topics: 33, activity: 6, age: 2, status: "beete 1 bis 3 geplant" },
    Spec { name: "Heimkino-Netzwerk", topics: 21, activity: 7, age: 1, status: "verkabelung getestet, abnahme offen" },
    Spec { name: "Sauerteig-Labor", topics: 13, activity: 4, age: 9, status: "rezept v3 stabil" },
    Spec { name: "Fahrrad-Werkstatt", topics: 8, activity: 3, age: 12, status: "aktiv" },
    Spec { name: "Kleinstadt-Chronik", topics: 17, activity: 1, age: 75, status: "pausiert bis archivbesuch" },
    Spec { name: "Sternkarten-Atlas", topics: 11, activity: 2, age: 40, status: "entwurf" },
    Spec { name: "Pilz-Zucht", topics: 4, activity: 1, age: 120, status: "ausgesetzt" },
    Spec { name: "Reisekasse", topics: 3, activity: 2, age: 20, status: "aktiv" },
    Spec { name: "Orgelbau-Notizen", topics: 6, activity: 1, age: 55, status: "recherche" },
    Spec { name: "Wetterstation", topics: 5, activity: 5, age: 3, status: "sensor 2 eingebaut" },
    Spec { name: "Lesezirkel", topics: 2, activity: 1, age: 30, status: "aktiv" },
    Spec { name: "Imkerei", topics: 2, activity: 0, age: 160, status: "winterruhe" },
    Spec { name: "Kaffeeroesterei", topics: 1, activity: 2, age: 8, status: "profil 4 getestet" },
    Spec { name: "Steuerkram", topics: 1, activity: 0, age: 200, status: "abgeschlossen" },
    Spec { name: "Modellbahn", topics: 10, activity: 3, age: 25, status: "gleisplan fertig" },
    Spec { name: "Podcast-Schnitt", topics: 3, activity: 2, age: 14, status: "aktiv" },
    Spec { name: "Schachtraining", topics: 4, activity: 1, age: 66, status: "pausiert" },
    Spec { name: "Balkonbewaesserung", topics: 2, activity: 3, age: 5, status: "pumpe bestellt" },
    Spec { name: "Umzugsliste", topics: 1, activity: 0, age: 95, status: "erledigt" },
    Spec { name: "Vogelbeobachtung", topics: 5, activity: 1, age: 33, status: "aktiv" },
    Spec { name: "Loetstation", topics: 2, activity: 1, age: 48, status: "kalibriert" },
    Spec { name: "Familienrezepte", topics: 7, activity: 0, age: 110, status: "gesammelt" },
    Spec { name: "Zeitkapsel", topics: 0, activity: 0, age: 140, status: "idee" },
];

const NOUNS: &[&str] = &[
    "Aufbau", "Werkzeuge", "Materialliste", "Zeitplan", "Messwerte", "Fehlerprotokoll", "Lieferanten",
    "Anleitung", "Skizzen", "Budget", "Ersatzteile", "Pflegeplan", "Checkliste", "Probelauf", "Rückblick",
    "Varianten", "Normen", "Kontakte", "Umbau", "Prüfstand", "Kalibrierung", "Verbrauch", "Lagerung",
    "Schnittstellen", "Reihenfolge", "Grenzwerte", "Beschaffung", "Abnahme", "Wartung", "Vergleich",
];
const QUALS: &[&str] = &[
    "Entwurf", "Version 2", "im Detail", "Notizen", "Plan", "Auswertung", "Regeln", "Varianten", "Herbst",
    "Grundlagen", "Sonderfälle", "Stand", "Lehren", "Messung",
];
const THINGS: &[&str] = &[
    "der Rahmen", "die Messreihe", "das Netzteil", "der Lieferant", "die Verkabelung", "der Probelauf",
    "das Gehäuse", "die Zeitplanung", "der Sensor", "die Pumpe", "das Protokoll", "die Halterung",
    "der Ersatzteilbestand", "die Kalibrierung", "das Gewicht", "die Dichtung",
];
const REASONS: &[&str] = &[
    "der Aufwand sonst nicht zu rechtfertigen wäre", "die Messwerte keinen Unterschied zeigen",
    "der Lieferant nur diese Variante führt", "es sich im Probelauf bewährt hat",
    "die Alternative mehr Wartung braucht", "so der Zeitplan hält",
];
const OPEN: &[&str] = &[
    "Prüfen, ob die Halterung das Gewicht dauerhaft trägt.", "Zweiten Lieferanten für Dichtungen anfragen.",
    "Messreihe bei Frost wiederholen.", "Entscheiden, ob das Gehäuse lackiert oder beschichtet wird.",
    "Zeitplan mit dem Lieferanten abstimmen.", "Kalibrierung nach dem Umbau erneut durchführen.",
    "Offene Rechnung für Ersatzteile klären.", "Probelauf mit voller Last einplanen.",
];

/// Freitext der Spezifikation → Aufzählung (nur eindeutige Wörter).
fn status_of(s: &str) -> Option<Status> {
    match s {
        "aktiv" => Some(Status::Active),
        "pausiert" | "ausgesetzt" | "winterruhe" => Some(Status::Paused),
        "abgeschlossen" | "erledigt" => Some(Status::Done),
        _ => None,
    }
}

fn status_key(s: Status) -> &'static str {
    match s {
        Status::Active => "active",
        Status::Paused => "paused",
        Status::Done => "done",
        Status::Superseded => "superseded",
    }
}

/// Der Freitext bleibt, außer er ist nur das Aufzählungswort.
fn state_of(s: &str) -> Option<String> {
    if s == "aktiv" { None } else { Some(s.to_string()) }
}

fn fold(c: char) -> &'static str {
    match c {
        'ä' | 'Ä' => "a",
        'ö' | 'Ö' => "o",
        'ü' | 'Ü' => "u",
        'ß' => "ss",
        _ => "",
    }
}

/// Faltet Text für die Suche (klein, Umlaute ohne Punkte) und merkt die Herkunft je Byte.
fn fold_text(s: &str) -> (String, Vec<usize>) {
    let mut out = String::with_capacity(s.len());
    let mut map = Vec::with_capacity(s.len() + 1);
    for (i, c) in s.char_indices() {
        let f = fold(c);
        if f.is_empty() {
            for lc in c.to_lowercase() {
                let before = out.len();
                out.push(lc);
                for _ in before..out.len() {
                    map.push(i);
                }
            }
        } else {
            for _ in 0..f.len() {
                map.push(i);
            }
            out.push_str(f);
        }
    }
    map.push(s.len());
    (out, map)
}

fn find_all(folded: &str, map: &[usize], term: &str) -> Vec<(usize, usize)> {
    let mut v = Vec::new();
    if term.is_empty() {
        return v;
    }
    let mut from = 0;
    while let Some(p) = folded[from..].find(term) {
        let s = from + p;
        let e = s + term.len();
        v.push((map[s], map[e.min(map.len() - 1)]));
        from = e;
    }
    v
}

struct FxNote {
    info: NoteInfo,
    sections: Vec<Section>,
    text: String,
}

pub struct FixtureSource {
    today: Date,
    projects: Vec<ProjectInfo>,
    notes: Vec<FxNote>,
    by_path: HashMap<String, usize>,
    day_series: Vec<u16>,
    edges: Vec<ProjectEdge>,
    out: Vec<Vec<usize>>,
    inc: Vec<Vec<usize>>,
}

impl Default for FixtureSource {
    fn default() -> Self {
        Self::new()
    }
}

impl FixtureSource {
    pub fn new() -> FixtureSource {
        Self::build(SPECS.len())
    }

    /// Nur die ersten `n` Projekte (für kleine Tests).
    pub fn with_projects(n: usize) -> FixtureSource {
        Self::build(n.min(SPECS.len()))
    }

    fn build(nproj: usize) -> FixtureSource {
        let today = Date::from_ymd(2026, 11, 9);
        let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
        let specs = &SPECS[..nproj];

        // --- Quellen ---
        let src_titles = [
            "Handbuch Dichtungstechnik", "Normblatt Sicherungen", "Forum: Probleme mit Netzteilen", "Anleitung Lackierung",
            "Datenblatt Sensor", "Vortrag Wetterdaten", "Wikipedia: Sauerteig", "Händlerkatalog Ersatzteile",
            "Artikel Bewässerung", "Aufsatz Orgelregister", "Messprotokoll Vorlage", "Tabelle Holzarten",
            "Blog: Gleisplanung", "Leitfaden Lötzinn", "Kurs Schachendspiele", "Verzeichnis Pilzarten",
            "Studie Bienenvölker", "Karte Sternbilder", "Reisebericht Küstenstraße", "Ratgeber Steuererklärung",
            "Ratgeber Umzug", "Buchbesprechung Lesekreis", "Tutorial Audioschnitt", "Anleitung Röstprofile",
        ];
        let mut notes: Vec<FxNote> = Vec::new();
        let mut by_path: HashMap<String, usize> = HashMap::new();
        let mut source_idx: Vec<usize> = Vec::new();
        for (i, t) in src_titles.iter().enumerate() {
            let path = format!("Sources/Web/{t}");
            let age = 30 + (i as i32 * 3) % 40;
            let info = NoteInfo {
                path: path.clone(),
                title: (*t).to_string(),
                project: None,
                ntype: NoteType::Source,
                status: None,
                state: Some("summarized".into()),
                checked_on: Some(today.add(-age)),
                modified: Some(today.add(-age)),
                tokens: 250 + (i as u32 * 37) % 400,
                bytes: 0,
                summary: format!("Quellennotiz zu {t}."),
                sections: SectionKind::Sources.bit(),
                links_out: 0,
                links_in: 0,
                findings: Findings::default(),
            };
            let text = format!(
                "---\ntype: source\nchecked_on: {}\n---\n# {t}\n\n## Aussagen der Quelle\n\nDie Quelle behandelt {} und nennt Grenzwerte, Preise und Ansprechpartner.\n",
                today.add(-age),
                t.to_lowercase()
            );
            by_path.insert(path, notes.len());
            source_idx.push(notes.len());
            let sections = vec![Section { level: 2, title: "Aussagen der Quelle".into(), kind: SectionKind::Other, line_start: 6, line_end: 8, tokens: 120 }];
            notes.push(FxNote { info, sections, text });
        }

        // --- Projekte und Notizen: erst Gerüst ---
        let mut proj_notes: Vec<Vec<usize>> = Vec::new();
        for sp in specs.iter() {
            let mut idxs = Vec::new();
            let mut used: BTreeMap<String, u32> = BTreeMap::new();
            // Übersicht
            let opath = format!("Projects/{}/00 Overview", sp.name);
            let big = (sp.topics as u32 * 90).clamp(500, 9000);
            let oinfo = NoteInfo {
                path: opath.clone(),
                title: format!("{} Übersicht", sp.name),
                project: Some(sp.name.to_string()),
                ntype: NoteType::Overview,
                status: status_of(sp.status),
                state: state_of(sp.status),
                checked_on: Some(today.add(-sp.age)),
                modified: Some(today.add(-sp.age)),
                tokens: big,
                bytes: 0,
                summary: format!("Landkarte und Stand von {}.", sp.name),
                sections: 0,
                links_out: 0,
                links_in: 0,
                findings: Findings::default(),
            };
            by_path.insert(opath, notes.len());
            idxs.push(notes.len());
            notes.push(FxNote { info: oinfo, sections: vec![], text: String::new() });
            for ti in 0..sp.topics {
                let noun = *rng.pick(NOUNS);
                let qual = *rng.pick(QUALS);
                let mut title = format!("{noun} {qual}");
                let n = used.entry(title.clone()).or_insert(0);
                *n += 1;
                if *n > 1 {
                    title = format!("{title} {n}");
                }
                let path = format!("Projects/{}/{}", sp.name, title);
                // ältere Notizen mit wachsendem Index; die ersten sind „heiß“
                let spread = (ti as f64).sqrt() * 5.5;
                let extra = (rng.next() % 1000) as f64 / 1000.0 * spread;
                let age = (sp.age as f64 + extra) as i32 + if ti < 2 { 0 } else { rng.below(4) as i32 };
                let age = if sp.activity == 0 { sp.age + rng.below(40) as i32 } else { age };
                let toks = match rng.below(100) {
                    0..=59 => rng.range(250, 1600),
                    60..=89 => rng.range(1600, 4200),
                    90..=96 => rng.range(4200, 6800),
                    _ => rng.range(6800, 14000),
                } as u32;
                let chk_delta = rng.below(9) as i32;
                let info = NoteInfo {
                    path: path.clone(),
                    title: title.clone(),
                    project: Some(sp.name.to_string()),
                    ntype: NoteType::Topic,
                    status: if rng.below(100) < 7 { Some(Status::Superseded) } else { None },
                    state: None,
                    checked_on: Some(today.add(-(age + chk_delta))),
                    modified: Some(today.add(-age)),
                    tokens: toks,
                    bytes: 0,
                    summary: String::new(),
                    sections: 0,
                    links_out: 0,
                    links_in: 0,
                    findings: Findings::default(),
                };
                by_path.insert(path, notes.len());
                idxs.push(notes.len());
                notes.push(FxNote { info, sections: vec![], text: String::new() });
            }
            proj_notes.push(idxs);
        }

        // --- Texte ---
        let gen_para = |rng: &mut Rng, proj: &str, related: &[usize], notes: &Vec<FxNote>, n_sent: usize| -> String {
            let mut s = String::new();
            for k in 0..n_sent {
                if k > 0 {
                    s.push(' ');
                }
                match rng.below(5) {
                    0 => s.push_str(&format!(
                        "{} wurde im Probelauf geprüft; das Ergebnis liegt im Rahmen der Erwartung und bleibt für {} maßgeblich.",
                        cap(rng.pick(THINGS)),
                        proj
                    )),
                    1 => s.push_str(&format!(
                        "Entscheidung: {} bleibt wie geplant, weil {}.",
                        rng.pick(THINGS),
                        rng.pick(REASONS)
                    )),
                    2 => s.push_str(&format!(
                        "Gemessen wurden {} Werte über {} Tage, die Streuung ist klein genug für den Vergleich.",
                        rng.range(8, 240),
                        rng.range(2, 30)
                    )),
                    3 => {
                        if let Some(&r) = related.get(rng.below(related.len().max(1))) {
                            let n = &notes[r].info;
                            s.push_str(&format!("Die Einzelheiten stehen unter [[{}|{}]].", n.path, n.title));
                        } else {
                            s.push_str("Weitere Einzelheiten folgen nach dem nächsten Probelauf.");
                        }
                    }
                    _ => s.push_str(&format!(
                        "Der Lieferant nannte für {} eine Lieferzeit von {} Tagen; der Zeitplan wurde angepasst.",
                        rng.pick(THINGS),
                        rng.range(3, 40)
                    )),
                }
            }
            s
        };

        let mut dead_budget = 6;
        let mut cross_links: Vec<(usize, usize)> = Vec::new();
        for (pi, sp) in specs.iter().enumerate() {
            let idxs = proj_notes[pi].clone();
            let topics: Vec<usize> = idxs[1..].to_vec();
            for (k, &ni) in idxs.iter().enumerate() {
                let is_overview = k == 0;
                // Verknüpfungen: Nachbarn im Projekt, Quellen, selten ein anderes Projekt
                let mut related: Vec<usize> = Vec::new();
                if !is_overview {
                    for _ in 0..rng.below(3) {
                        if !topics.is_empty() {
                            let r = topics[rng.below(topics.len())];
                            if r != ni {
                                related.push(r);
                            }
                        }
                    }
                    if rng.chance(0.35) {
                        related.push(*rng.pick(&source_idx));
                    }
                    if rng.chance(0.07) && nproj > 1 {
                        let op = rng.below(nproj);
                        if op != pi && proj_notes[op].len() > 1 {
                            let r = proj_notes[op][1 + rng.below(proj_notes[op].len() - 1)];
                            related.push(r);
                            cross_links.push((ni, r));
                        }
                    }
                }
                let info_title = notes[ni].info.title.clone();
                let info_checked = notes[ni].info.checked_on.unwrap();
                let mut lines: Vec<String> = Vec::new();
                let mut sections: Vec<Section> = Vec::new();
                lines.push("---".into());
                lines.push(format!("type: {}", if is_overview { "overview" } else { "topic" }));
                if is_overview {
                    if let Some(st) = status_of(sp.status) {
                        lines.push(format!("status: {}", status_key(st)));
                    }
                    lines.push(format!("state: {}", sp.status));
                    lines.push(format!("paths: [\"G:/Projekte/{}\"]", sp.name));
                }
                if notes[ni].info.superseded() {
                    lines.push("status: superseded".into());
                }
                lines.push(format!("checked_on: {info_checked}"));
                lines.push("---".into());
                lines.push(format!("# {info_title}"));
                lines.push(String::new());

                let mut mask = 0u8;
                let mut open_items = 0u32;
                let mut kinds: Vec<(SectionKind, &str)> = Vec::new();
                if is_overview || rng.chance(0.8) {
                    kinds.push((SectionKind::State, *rng.pick(&["Stand", "Aktueller Stand", "Stand (2026-10-16)"])));
                }
                if is_overview || rng.chance(0.55) {
                    kinds.push((SectionKind::Open, *rng.pick(&["Offen", "Offene Punkte", "Nächster Schritt"])));
                }
                if rng.chance(0.4) {
                    kinds.push((SectionKind::Decisions, "Entscheidungen mit Grund"));
                }
                if rng.chance(0.35) {
                    kinds.push((SectionKind::Lessons, *rng.pick(&["Sackgassen", "Was funktioniert hat, Sackgassen"])));
                }
                if rng.chance(0.3) {
                    kinds.push((SectionKind::Evidence, "Prüfungen"));
                }
                if rng.chance(0.3) {
                    kinds.push((SectionKind::Sources, "Quellen"));
                }
                if is_overview {
                    kinds.push((SectionKind::Topics, "Themen"));
                }
                if kinds.is_empty() {
                    kinds.push((SectionKind::Other, "Aufbau"));
                }
                for (kind, heading) in kinds {
                    mask |= kind.bit();
                    let start = lines.len() as u32 + 1;
                    lines.push(format!("## {heading}"));
                    lines.push(String::new());
                    match kind {
                        SectionKind::Open => {
                            let n = rng.range(1, if is_overview { 4 } else { 3 });
                            for _ in 0..n {
                                lines.push(format!("- {}", rng.pick(OPEN)));
                                open_items += 1;
                            }
                        }
                        SectionKind::Sources => {
                            for &r in related.iter().filter(|&&r| notes[r].info.ntype == NoteType::Source) {
                                lines.push(format!("- [[{}|{}]]", notes[r].info.path, notes[r].info.title));
                            }
                            let r = *rng.pick(&source_idx);
                            lines.push(format!("- [[{}|{}]]", notes[r].info.path, notes[r].info.title));
                        }
                        SectionKind::Topics => {
                            for &t in &topics {
                                let ti = &notes[t].info;
                                // Übersicht verlinkt ~92 % der Themen; der Rest bleibt Waise
                                if ((t * 2654435761usize) >> 4) % 100 < 92 {
                                    lines.push(format!("- [[{}|{}]]", ti.path, ti.title));
                                }
                            }
                        }
                        _ => {
                            let paras = if is_overview { 1 } else { rng.range(1, 2) };
                            for _ in 0..paras {
                                let ns = rng.range(2, 5);
                                lines.push(gen_para(&mut rng, sp.name, &related, &notes, ns));
                                lines.push(String::new());
                            }
                            if !is_overview && kind == SectionKind::State && dead_budget > 0 && rng.chance(0.03) {
                                lines.push(format!("Siehe auch [[Projects/{}/Gestrichene Notiz|Gestrichene Notiz]].", sp.name));
                                dead_budget -= 1;
                            }
                        }
                    }
                    if lines.last().map(|l| !l.is_empty()).unwrap_or(false) {
                        lines.push(String::new());
                    }
                    let end = lines.len() as u32 - 1;
                    let level_tokens = ((end - start + 1) * 28).max(40);
                    sections.push(Section { level: 2, title: heading.to_string(), kind, line_start: start, line_end: end, tokens: level_tokens });
                }
                let text = lines.join("\n");
                let summary = if is_overview {
                    format!("Landkarte und Stand von {} ({} Themen).", sp.name, topics.len())
                } else {
                    format!("{} im Projekt {}.", info_title, sp.name)
                };
                let n = &mut notes[ni];
                n.info.sections = mask;
                n.info.summary = summary;
                n.info.bytes = n.info.tokens as u64 * 4;
                n.sections = sections;
                n.text = text;
                // offene Punkte merken wir im Projekt-Block unten
                let _ = open_items;
            }
        }

        // --- Verknüpfungen auflösen ---
        let mut out: Vec<Vec<usize>> = vec![Vec::new(); notes.len()];
        let mut dead: Vec<u32> = vec![0; notes.len()];
        for (i, n) in notes.iter().enumerate() {
            let mut seen = std::collections::HashSet::new();
            for l in wikilinks(&n.text) {
                match by_path.get(&l.target) {
                    Some(&t) if t != i => {
                        if seen.insert(t) {
                            out[i].push(t);
                        }
                    }
                    Some(_) => {}
                    None => dead[i] += 1,
                }
            }
        }
        let mut inc: Vec<Vec<usize>> = vec![Vec::new(); notes.len()];
        for (i, v) in out.iter().enumerate() {
            for &t in v {
                inc[t].push(i);
            }
        }
        for (i, n) in notes.iter_mut().enumerate() {
            n.info.links_out = out[i].len() as u32;
            n.info.links_in = inc[i].len() as u32;
            let stale = today.since(n.info.checked_on.unwrap_or(today)) > 45;
            n.info.findings = Findings {
                stale: stale as u32,
                oversize: (n.info.tokens > 6000) as u32,
                dead_links: dead[i],
                orphans: (inc[i].is_empty() && n.info.ntype != NoteType::Overview && n.info.ntype != NoteType::Source) as u32,
                other: 0,
            };
        }
        let _ = cross_links;

        // --- Aktivität: Tagesreihe je Projekt ---
        let span = 140usize;
        let mut day_series = vec![0u16; span];
        let mut projects = Vec::new();
        for (pi, sp) in specs.iter().enumerate() {
            let mut daily = vec![0u16; span]; // Index = Alter in Tagen (0 = heute)
            if sp.activity > 0 {
                let start = (sp.age + 55).min(span as i32 - 1) as usize;
                for (age, slot) in daily.iter_mut().enumerate().take(start + 1).skip(sp.age as usize) {
                    let wd = today.add(-(age as i32)).weekday();
                    let weekend = if wd >= 5 { 0.5 } else { 1.0 };
                    let p = (0.18 + sp.activity as f64 * 0.08) * weekend;
                    if rng.chance(p) {
                        let burst = if rng.chance(0.12) { 4 } else { 1 };
                        *slot = (rng.range(1, 1 + sp.activity as usize * 2) * burst) as u16;
                    }
                }
                daily[sp.age as usize] = daily[sp.age as usize].max(1);
            }
            let mut weekly = vec![0u16; 12];
            let wd_today = today.weekday() as usize;
            for (age, &v) in daily.iter().enumerate() {
                let week_back = if age <= wd_today { 0 } else { (age - wd_today - 1) / 7 + 1 };
                if week_back < 12 {
                    weekly[11 - week_back] = weekly[11 - week_back].saturating_add(v);
                }
                day_series[age] = day_series[age].saturating_add(v);
            }
            let idxs = &proj_notes[pi];
            let mut findings = Findings::default();
            let mut bytes = 0u64;
            let mut tokens = 0u32;
            let mut last: Option<Date> = None;
            let mut open_points = 0u32;
            for &ni in idxs {
                let n = &notes[ni];
                findings.stale += n.info.findings.stale;
                findings.oversize += n.info.findings.oversize;
                findings.dead_links += n.info.findings.dead_links;
                findings.orphans += n.info.findings.orphans;
                findings.other += n.info.findings.other;
                bytes += n.info.bytes;
                tokens += n.info.tokens;
                last = last.max(n.info.modified);
                for s in &n.sections {
                    if s.kind == SectionKind::Open {
                        open_points += n
                            .text
                            .lines()
                            .skip(s.line_start as usize)
                            .take((s.line_end - s.line_start + 1) as usize)
                            .filter(|l| l.starts_with("- "))
                            .count() as u32;
                    }
                }
            }
            projects.push(ProjectInfo {
                name: sp.name.to_string(),
                notes: idxs.len() as u32,
                bytes,
                tokens,
                last_change: last,
                open_points,
                findings,
                weekly,
                status: status_of(sp.status),
                state: state_of(sp.status),
                paths: vec![format!("G:/Projekte/{}", sp.name)],
                overview: Some(notes[idxs[0]].info.path.clone()),
            });
        }
        // Gesamtreihe: älteste zuerst
        day_series.reverse();

        // --- Projektkanten ---
        let mut pair_links: BTreeMap<(String, String), u32> = BTreeMap::new();
        let mut src_projects: BTreeMap<usize, Vec<String>> = BTreeMap::new();
        for (i, v) in out.iter().enumerate() {
            let Some(pa) = notes[i].info.project.clone() else { continue };
            for &t in v {
                match &notes[t].info.project {
                    Some(pb) if *pb != pa => {
                        let key = if pa < *pb { (pa.clone(), pb.clone()) } else { (pb.clone(), pa.clone()) };
                        *pair_links.entry(key).or_default() += 1;
                    }
                    None if notes[t].info.ntype == NoteType::Source => {
                        let e = src_projects.entry(t).or_default();
                        if !e.contains(&pa) {
                            e.push(pa.clone());
                        }
                    }
                    _ => {}
                }
            }
        }
        let mut pair_src: BTreeMap<(String, String), u32> = BTreeMap::new();
        for ps in src_projects.values() {
            for a in 0..ps.len() {
                for b in a + 1..ps.len() {
                    let key = if ps[a] < ps[b] { (ps[a].clone(), ps[b].clone()) } else { (ps[b].clone(), ps[a].clone()) };
                    *pair_src.entry(key).or_default() += 1;
                }
            }
        }
        let mut keys: Vec<(String, String)> = pair_links.keys().chain(pair_src.keys()).cloned().collect();
        keys.sort();
        keys.dedup();
        let edges = keys
            .into_iter()
            .map(|k| ProjectEdge {
                links: pair_links.get(&k).copied().unwrap_or(0),
                shared_sources: pair_src.get(&k).copied().unwrap_or(0),
                a: k.0,
                b: k.1,
            })
            .collect();

        FixtureSource { today, projects, notes, by_path, day_series, edges, out, inc }
    }

    fn link_of(&self, i: usize) -> Link {
        let n = &self.notes[i].info;
        Link { path: n.path.clone(), title: n.title.clone(), project: n.project.clone(), ntype: n.ntype, status: n.status, modified: n.modified }
    }
}

fn cap(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

impl DataSource for FixtureSource {
    fn sources(&self) -> u32 {
        self.notes.iter().filter(|n| n.info.ntype == NoteType::Source).count() as u32
    }

    fn today(&self) -> Date {
        self.today
    }

    fn label(&self) -> String {
        "fixture".to_string()
    }

    fn projects(&self) -> Vec<ProjectInfo> {
        self.projects.clone()
    }

    fn notes(&self, project: &str) -> Vec<NoteInfo> {
        let mut v: Vec<NoteInfo> = self
            .notes
            .iter()
            .filter(|n| n.info.project.as_deref() == Some(project))
            .map(|n| n.info.clone())
            .collect();
        v.sort_by(|a, b| a.path.cmp(&b.path));
        v
    }

    fn note(&self, path: &str) -> Option<NoteDoc> {
        let i = *self.by_path.get(path)?;
        let n = &self.notes[i];
        Some(NoteDoc { info: n.info.clone(), sections: n.sections.clone(), text: n.text.clone() })
    }

    fn search(&self, query: &str, limit: usize) -> Vec<SearchHit> {
        let (fq, _) = fold_text(query.trim());
        let terms: Vec<&str> = fq.split_whitespace().collect();
        if terms.is_empty() {
            return Vec::new();
        }
        let mut scored: Vec<(u32, SearchHit)> = Vec::new();
        for n in &self.notes {
            let (ftitle, tmap) = fold_text(&n.info.title);
            let (ftext, map) = fold_text(&n.text);
            // alle Begriffe müssen irgendwo vorkommen
            if !terms.iter().all(|t| ftext.contains(t) || ftitle.contains(t)) {
                continue;
            }
            let title_hits: u32 = terms.iter().map(|t| find_all(&ftitle, &tmap, t).len() as u32).sum();
            // bester Abschnitt
            let lines: Vec<&str> = n.text.split('\n').collect();
            let mut best: Option<(u32, &Section)> = None;
            let mut line_off = vec![0usize; lines.len() + 1];
            for (i, l) in lines.iter().enumerate() {
                line_off[i + 1] = line_off[i] + l.len() + 1;
            }
            for s in &n.sections {
                let a = line_off[(s.line_start as usize - 1).min(lines.len())];
                let b = line_off[(s.line_end as usize).min(lines.len())].min(n.text.len());
                // Bereich im gefalteten Text bestimmen: über map rückwärts (grob, per Suche)
                let mut sc = 0u32;
                for t in &terms {
                    for (ms, _) in find_all(&ftext, &map, t) {
                        if ms >= a && ms < b {
                            sc += 1;
                        }
                    }
                }
                let (fh, hm) = fold_text(&s.title);
                for t in &terms {
                    sc += 3 * find_all(&fh, &hm, t).len() as u32;
                }
                if sc > 0 && best.map(|(bs, _)| sc > bs).unwrap_or(true) {
                    best = Some((sc, s));
                }
            }
            let score = title_hits * 5 + best.map(|(s, _)| s.min(8)).unwrap_or(1);
            let (section, ls, le, region) = match best {
                Some((_, s)) => {
                    let a = line_off[(s.line_start as usize - 1).min(lines.len())];
                    let b = line_off[(s.line_end as usize).min(lines.len())].min(n.text.len());
                    (Some(s.title.clone()), s.line_start, s.line_end, (a, b))
                }
                None => (None, 1, lines.len() as u32, (0, n.text.len())),
            };
            // Ausschnitt um den ersten Treffer im Bereich
            let first = terms
                .iter()
                .flat_map(|t| find_all(&ftext, &map, t))
                .filter(|(s, _)| *s >= region.0 && *s < region.1)
                .map(|(s, _)| s)
                .min()
                .unwrap_or(region.0);
            let text = &n.text;
            let mut a = first.saturating_sub(70);
            while a > 0 && !text.is_char_boundary(a) {
                a -= 1;
            }
            let mut b = (first + 160).min(text.len());
            while b < text.len() && !text.is_char_boundary(b) {
                b += 1;
            }
            // an Wortgrenzen schneiden
            if a > 0 {
                if let Some(p) = text[a..first.max(a)].find(' ') {
                    a += p + 1;
                }
            }
            if b < text.len() {
                if let Some(p) = text[first.min(b)..b].rfind(' ') {
                    b = first.min(b) + p;
                }
            }
            let mut snippet = text[a..b].replace('\n', " ");
            let lead = if a > 0 { "… " } else { "" };
            if b < text.len() {
                snippet.push_str(" …");
            }
            snippet = format!("{lead}{snippet}");
            let (fs, sm) = fold_text(&snippet);
            let mut marks: Vec<(usize, usize)> = terms.iter().flat_map(|t| find_all(&fs, &sm, t)).collect();
            marks.sort();
            marks.dedup();
            let hit = SearchHit {
                path: n.info.path.clone(),
                title: n.info.title.clone(),
                project: n.info.project.clone(),
                ntype: n.info.ntype,
                status: n.info.status,
                modified: n.info.modified,
                section,
                line_start: ls,
                line_end: le,
                tokens: ((le - ls + 1) * 28).max(40),
                snippet,
                marks,
            };
            scored.push((score, hit));
        }
        scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.path.cmp(&b.1.path)));
        scored.into_iter().take(limit).map(|(_, h)| h).collect()
    }

    fn links(&self, path: &str) -> NoteLinks {
        let Some(&i) = self.by_path.get(path) else { return NoteLinks::default() };
        let mut inc: Vec<Link> = self.inc[i].iter().map(|&j| self.link_of(j)).collect();
        inc.sort_by(|a, b| a.path.cmp(&b.path));
        NoteLinks { out: self.out[i].iter().map(|&j| self.link_of(j)).collect(), inc }
    }

    fn activity(&self, days: u32) -> Vec<u16> {
        let n = self.day_series.len();
        let d = days as usize;
        if d >= n {
            let mut v = vec![0u16; d - n];
            v.extend_from_slice(&self.day_series);
            v
        } else {
            self.day_series[n - d..].to_vec()
        }
    }

    fn project_edges(&self) -> Vec<ProjectEdge> {
        self.edges.clone()
    }
}

/// Für Tests: Anzeigename eines Links im Text.
#[allow(dead_code)]
pub(crate) fn label_of(text: &str) -> Vec<String> {
    wikilinks(text).iter().map(link_label).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic() {
        let a = FixtureSource::new();
        let b = FixtureSource::new();
        assert_eq!(a.projects().len(), 24);
        let pa = a.projects();
        let pb = b.projects();
        for (x, y) in pa.iter().zip(pb.iter()) {
            assert_eq!(x.name, y.name);
            assert_eq!(x.weekly, y.weekly);
            assert_eq!(x.bytes, y.bytes);
        }
        assert_eq!(a.activity(30), b.activity(30));
    }

    #[test]
    fn shape_resembles_real_vault() {
        let s = FixtureSource::new();
        let ps = s.projects();
        let big = ps.iter().map(|p| p.notes).max().unwrap();
        let small = ps.iter().filter(|p| p.notes <= 5).count();
        assert!(big > 50 && small >= 10);
        assert!(ps.iter().any(|p| p.findings.dead_links > 0));
        assert!(ps.iter().any(|p| p.findings.orphans > 0));
        assert!(ps.iter().any(|p| p.findings.stale > 0));
        assert!(!s.project_edges().is_empty());
    }

    #[test]
    fn search_finds_with_umlaut_folding() {
        let s = FixtureSource::new();
        let hits = s.search("prüfstand", 10);
        assert!(!hits.is_empty(), "Thema 'Prüfstand' muss gefunden werden");
        assert_eq!(hits.len(), s.search("PRUFSTAND", 10).len(), "Faltung ü/u und Großschreibung");
        let h = &hits[0];
        assert!(!h.marks.is_empty());
        for (a, b) in &h.marks {
            assert!(h.snippet.is_char_boundary(*a) && h.snippet.is_char_boundary(*b));
        }
        assert!(s.search("", 5).is_empty());
        assert!(s.search("xyzzyquux", 5).is_empty());
    }

    #[test]
    fn links_are_consistent() {
        let s = FixtureSource::new();
        let p = s.projects().into_iter().next().unwrap();
        let ov = p.overview.unwrap();
        let l = s.links(&ov);
        assert!(l.out.len() > 10);
        for t in l.out.iter().take(5) {
            assert!(s.links(&t.path).inc.iter().any(|x| x.path == ov));
        }
    }

    #[test]
    fn activity_aligns_to_today() {
        let s = FixtureSource::new();
        let a = s.activity(7);
        assert_eq!(a.len(), 7);
        assert_eq!(s.activity(200).len(), 200);
    }
}
