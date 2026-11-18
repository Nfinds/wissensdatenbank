//! Öffentliche Datentypen der Wissensbank. Alle Typen sind `Serialize`, damit
//! `wb --json`, die TUI (B) und die Migration (C) dieselben Felder sehen.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

use crate::frontmatter::FrontmatterState;
use crate::text;

/// Pfad einer Notiz relativ zur Vault-Wurzel: Schrägstriche, Unicode-NFC,
/// mit Endung `.md`. Beispiel: `Projects/Example/00 Overview.md`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NotePath(String);

impl NotePath {
    /// Normalisiert: `\` → `/`, führende `./` und `/` weg, NFC.
    pub fn new(p: impl AsRef<str>) -> Self {
        let s = p.as_ref().replace('\\', "/");
        let s = s.trim_start_matches("./").trim_start_matches('/');
        NotePath(text::nfc(s))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
    /// Pfad ohne `.md` (so steht er in Wikilinks und in Adressen von `wb`).
    pub fn without_ext(&self) -> &str {
        strip_md(&self.0)
    }
    /// Dateiname ohne `.md`.
    pub fn stem(&self) -> &str {
        let w = self.without_ext();
        w.rsplit('/').next().unwrap_or(w)
    }
    /// Ordner (ohne abschließenden Schrägstrich), leer für Dateien in der Wurzel.
    pub fn folder(&self) -> &str {
        match self.0.rfind('/') {
            Some(i) => &self.0[..i],
            None => "",
        }
    }
    /// Projektname, wenn der Pfad unter `Projects/<Projekt>/` liegt.
    pub fn project(&self) -> Option<&str> {
        let rest = self.0.strip_prefix("Projects/")?;
        let (name, tail) = rest.split_once('/')?;
        if tail.is_empty() { None } else { Some(name) }
    }
    /// Vergleichsschlüssel: ohne `.md`, kleingeschrieben (Groß/Klein-Gleichheit wie unter Windows).
    pub fn key(&self) -> String {
        path_key(self.without_ext())
    }
    pub fn is_overview(&self) -> bool {
        self.stem().eq_ignore_ascii_case("00 Overview")
    }
}

impl fmt::Display for NotePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

pub(crate) fn strip_md(s: &str) -> &str {
    if has_md_ext(s) { &s[..s.len() - 3] } else { s }
}

/// Endet auf `.md` (ohne Groß/Klein)?
pub fn has_md_ext(s: &str) -> bool {
    s.len() >= 3 && s.as_bytes()[s.len() - 3..].eq_ignore_ascii_case(b".md")
}

/// Schlüssel für den Vergleich von Pfaden und Linkzielen.
pub fn path_key(s: &str) -> String {
    let s = s.replace('\\', "/");
    let s = strip_md(s.trim().trim_start_matches("./").trim_start_matches('/'));
    text::nfc(s).to_lowercase()
}

/// Notizart. Gültige Werte im Frontmatter sind `overview`, `topic`, `source` (Vertrag §13.5).
/// Alte Werte werden beim Lesen abgebildet (`project`/`project-overview` → `Overview`,
/// `project-topic` → `Topic`); der Originalwert steht in [`NoteMeta::type_raw`].
/// `Journal` und `Other` vergibt nur der Index (Journaldateien bzw. Notizen außerhalb von
/// `Projects/` und `Sources/`, im alten Wiki z. B. `System/`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NoteType {
    Overview,
    #[default]
    Topic,
    Source,
    Journal,
    Other,
}

impl NoteType {
    pub fn as_str(self) -> &'static str {
        match self {
            NoteType::Overview => "overview",
            NoteType::Topic => "topic",
            NoteType::Source => "source",
            NoteType::Journal => "journal",
            NoteType::Other => "other",
        }
    }
    /// Werte, die im Frontmatter gültig sind.
    pub const VALID: [&'static str; 3] = ["overview", "topic", "source"];

    pub fn parse(s: &str) -> Option<NoteType> {
        Some(match s.trim().to_ascii_lowercase().as_str() {
            "overview" | "project" | "project-overview" => NoteType::Overview,
            "topic" | "project-topic" | "note" => NoteType::Topic,
            "source" | "web-source" | "source-note" | "web" => NoteType::Source,
            "journal" | "log" => NoteType::Journal,
            "other" => NoteType::Other,
            _ => return None,
        })
    }
    /// Art aus Frontmatter-Wert und Pfad (Pfad entscheidet, wenn der Wert fehlt oder unbekannt ist).
    pub fn classify(type_raw: Option<&str>, path: &NotePath) -> NoteType {
        if is_journal_path(path) {
            return NoteType::Journal;
        }
        if path.is_overview() && path.project().is_some() {
            return NoteType::Overview;
        }
        if let Some(t) = type_raw.and_then(NoteType::parse) {
            return t;
        }
        let p = path.as_str();
        if p.starts_with("Sources/") {
            NoteType::Source
        } else if path.project().is_some() {
            NoteType::Topic
        } else {
            NoteType::Other
        }
    }
}

/// `status` im Frontmatter (Vertrag §13.5, nur overview/topic). Freitext gehört nach `state`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NoteStatus {
    Active,
    Paused,
    Done,
    Superseded,
}

impl NoteStatus {
    pub const ALL: [NoteStatus; 4] = [NoteStatus::Active, NoteStatus::Paused, NoteStatus::Done, NoteStatus::Superseded];
    pub fn as_str(self) -> &'static str {
        match self {
            NoteStatus::Active => "active",
            NoteStatus::Paused => "paused",
            NoteStatus::Done => "done",
            NoteStatus::Superseded => "superseded",
        }
    }
    /// Nur die vier Werte (Groß/Klein egal); alles andere ist Freitext und ergibt `None`.
    pub fn parse(s: &str) -> Option<NoteStatus> {
        NoteStatus::ALL.iter().copied().find(|k| k.as_str().eq_ignore_ascii_case(s.trim()))
    }
}

impl fmt::Display for NoteStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl fmt::Display for NoteType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Journalquellen: `Journal/*.md` und das alte Log `System/Wiki Log.md`.
/// Sie werden nicht in die Volltextsuche aufgenommen, sondern zeilenweise ins Journal.
pub fn is_journal_path(path: &NotePath) -> bool {
    let k = path.key();
    k.starts_with("journal/") || k == "system/wiki log"
}

/// Art eines Abschnitts, aus der Überschrift per Schlüsselwort abgeleitet (Vertrag §4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
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
    pub const ALL: [SectionKind; 9] = [
        SectionKind::State,
        SectionKind::Open,
        SectionKind::Decisions,
        SectionKind::Lessons,
        SectionKind::Evidence,
        SectionKind::Sources,
        SectionKind::Paths,
        SectionKind::Topics,
        SectionKind::Other,
    ];
    pub fn as_str(self) -> &'static str {
        match self {
            SectionKind::State => "state",
            SectionKind::Open => "open",
            SectionKind::Decisions => "decisions",
            SectionKind::Lessons => "lessons",
            SectionKind::Evidence => "evidence",
            SectionKind::Sources => "sources",
            SectionKind::Paths => "paths",
            SectionKind::Topics => "topics",
            SectionKind::Other => "other",
        }
    }
    pub fn parse(s: &str) -> Option<SectionKind> {
        SectionKind::ALL.iter().copied().find(|k| k.as_str().eq_ignore_ascii_case(s.trim()))
    }
}

impl fmt::Display for SectionKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Ein Abschnitt einer Notiz. Zeilen sind 1-basiert und zählen die Datei wie ein Editor
/// (Frontmatter eingeschlossen). Abschnitt 0 ist der Vorspann vor der ersten Überschrift
/// (`level == 0`, leerer Titel), sofern er Text enthält.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Section {
    /// Laufende Nummer in der Notiz (0-basiert, in Dateireihenfolge).
    pub index: u32,
    /// Überschriftenebene 1–6, 0 für den Vorspann.
    pub level: u8,
    pub title: String,
    pub kind: SectionKind,
    /// Zeile der Überschrift (bzw. erste Zeile des Vorspanns).
    pub line_start: u32,
    /// Letzte Zeile einschließlich Unterabschnitten.
    pub line_end: u32,
    /// Letzte Zeile des eigenen Texts (vor der nächsten Überschrift beliebiger Ebene).
    pub own_end: u32,
    /// Geschätzte Token für `line_start..=line_end`.
    pub tokens: u32,
    /// Geschätzte Token für `line_start..=own_end`.
    pub own_tokens: u32,
}

/// Wikilink `[[Ziel#Überschrift|Alias]]`, Fundstelle mit Byte-Bereich im Text.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WikiLink {
    /// Ziel wie geschrieben (ohne `#…` und `|…`, getrimmt).
    pub target: String,
    pub heading: Option<String>,
    pub alias: Option<String>,
    /// `![[…]]`
    pub embed: bool,
    /// Alias-Trenner als `\|` geschrieben (Wikilink in einer Markdown-Tabelle).
    pub escaped_pipe: bool,
    /// 1-basierte Zeile.
    pub line: u32,
    /// Byte-Bereich des ganzen Links (`[[` bis `]]`, bei Embed ab `!`) im übergebenen Text.
    pub start: usize,
    pub end: usize,
}

/// Auflösung eines Linkziels gegen den Bestand.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", content = "target", rename_all = "lowercase")]
pub enum LinkStatus {
    Ok(NotePath),
    Ambiguous(Vec<NotePath>),
    Dead,
}

/// Ein Link mit Quelle, Ziel und Auflösung (für `links()`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinkInfo {
    pub source: NotePath,
    pub line: u32,
    pub target: String,
    pub heading: Option<String>,
    pub alias: Option<String>,
    pub status: LinkStatus,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoteLinks {
    pub outgoing: Vec<LinkInfo>,
    pub incoming: Vec<LinkInfo>,
}

/// Metadaten einer Notiz (aus dem Index, billig für Listen).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NoteMeta {
    pub path: NotePath,
    /// H1-Überschrift, sonst Dateiname.
    pub title: String,
    pub note_type: NoteType,
    /// `type` wie im Frontmatter.
    pub type_raw: Option<String>,
    pub project: Option<String>,
    pub status: Option<String>,
    pub state: Option<String>,
    pub checked_on: Option<String>,
    /// Frontmatter `summary`, sonst erster Satz des ersten Absatzes (gekürzt).
    pub summary: Option<String>,
    pub aliases: Vec<String>,
    /// Änderungszeit der Datei, Sekunden seit 1970 (UTC).
    pub mtime: i64,
    pub bytes: u64,
    pub lines: u32,
    pub tokens: u32,
    pub links_out: u32,
    pub links_in: u32,
    /// Frontmatter-Probleme (leer = in Ordnung).
    pub frontmatter_problems: Vec<String>,
}

impl NoteMeta {
    /// `status` als Aufzählung (`None` bei fehlendem oder freiem Wert).
    pub fn status_kind(&self) -> Option<NoteStatus> {
        self.status.as_deref().and_then(NoteStatus::parse)
    }
    /// Überholt (`status: superseded`).
    pub fn is_superseded(&self) -> bool {
        self.status_kind() == Some(NoteStatus::Superseded)
    }
}

/// Eine Notiz mit Abschnitten und Rohtext (frisch von der Platte gelesen).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Note {
    pub meta: NoteMeta,
    pub frontmatter: FrontmatterState,
    pub sections: Vec<Section>,
    /// Dateiinhalt als UTF-8 (BOM entfernt, Zeilenenden unverändert).
    pub text: String,
}

impl Note {
    /// Zeilen der Datei (1-basiert, `\r` entfernt).
    pub fn line(&self, n: u32) -> Option<&str> {
        self.text.split('\n').nth(n.checked_sub(1)? as usize).map(|l| l.strip_suffix('\r').unwrap_or(l))
    }
    /// Text der Zeilen `from..=to` (1-basiert, `\r` entfernt, mit `\n` verbunden).
    pub fn lines(&self, from: u32, to: u32) -> String {
        let mut out = String::new();
        for (i, l) in self.text.split('\n').enumerate() {
            let n = i as u32 + 1;
            if n < from {
                continue;
            }
            if n > to {
                break;
            }
            out.push_str(l.strip_suffix('\r').unwrap_or(l));
            out.push('\n');
        }
        out
    }
}

/// Auswahl für [`crate::Vault::notes`]. Leere Felder filtern nicht.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct NoteFilter {
    pub project: Option<String>,
    pub note_type: Option<NoteType>,
    /// Pfadpräfix relativ zur Vault-Wurzel, z. B. `Sources/Web/`.
    pub prefix: Option<String>,
    /// Journalquellen mitliefern (Standard: nein).
    pub include_journal: bool,
}

/// Aktivität in einer Woche (Montag als Wochenbeginn).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WeekActivity {
    /// Montag der Woche, `JJJJ-MM-TT`.
    pub week: String,
    pub count: u32,
}

/// Aktivität an einem Tag (Journalzeilen, die das Projekt/die Notiz betreffen).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DayActivity {
    pub date: String,
    pub project: Option<String>,
    pub count: u32,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ActivityFilter {
    pub project: Option<String>,
    /// `JJJJ-MM-TT`, einschließlich.
    pub since: Option<String>,
    pub until: Option<String>,
}

/// Projekt mit Kennzahlen.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProjectInfo {
    pub name: String,
    pub overview: Option<NotePath>,
    pub notes: u32,
    pub bytes: u64,
    pub tokens: u64,
    /// Neueste Änderungszeit einer Notiz (Sekunden seit 1970).
    pub last_modified: i64,
    /// Neuestes `checked_on` einer Notiz.
    pub last_checked: Option<String>,
    /// `status` der Übersicht.
    pub status: Option<String>,
    /// Listenpunkte in Abschnitten der Art `open` (über alle Notizen des Projekts).
    pub open_items: u32,
    /// Befunde je Code (aus `check`).
    pub findings: BTreeMap<String, u32>,
    /// Journalzeilen je Woche, älteste zuerst.
    pub activity: Vec<WeekActivity>,
    /// Projektwurzeln auf der Platte (Frontmatter `paths`, sonst aus dem Übersichtstext abgeleitet).
    pub paths: Vec<String>,
    /// `true`, wenn `paths` aus dem Text abgeleitet und nicht im Frontmatter angegeben sind.
    pub paths_inferred: bool,
}

/// Suchanfrage.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SearchQuery {
    pub text: String,
    pub project: Option<String>,
    pub note_type: Option<NoteType>,
    pub kind: Option<SectionKind>,
    pub prefix: Option<String>,
    /// Nur Notizen mit diesem `status` (z. B. gezielt die überholten).
    pub status: Option<NoteStatus>,
    /// Treffer überspringen (Blättern).
    pub offset: usize,
    /// Höchstzahl Treffer; das Budget kürzt in der CLI zusätzlich.
    pub limit: usize,
    /// Höchstens so viele Abschnitte je Notiz in der Liste (weitere werden gezählt).
    pub per_note: usize,
}

impl Default for SearchQuery {
    fn default() -> Self {
        SearchQuery {
            text: String::new(),
            project: None,
            note_type: None,
            kind: None,
            prefix: None,
            status: None,
            offset: 0,
            limit: 10,
            per_note: 2,
        }
    }
}

impl SearchQuery {
    pub fn new(text: impl Into<String>) -> Self {
        SearchQuery { text: text.into(), ..Default::default() }
    }
}

/// Wie ein Suchbegriff verwendet wurde.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueryTerm {
    pub raw: String,
    /// Indexformen (gefaltet, Stämme DE/EN).
    pub forms: Vec<String>,
    /// Ersatzwort bei Tippfehler (aus dem Wortschatz des Index).
    pub corrected: Option<String>,
    /// Teilwortsuche (Trigramm) wurde benutzt.
    pub substring: bool,
    /// Als Füllwort verworfen.
    pub stopword: bool,
    /// Abschnitte mit diesem Begriff.
    pub sections: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SearchHit {
    /// 1-basierter Rang in der Gesamtliste.
    pub rank: u32,
    pub path: NotePath,
    /// Adresse für `wb read`, z. B. `Projects/X/Thema#Offen` oder `Projects/X/Thema#L40`.
    pub address: String,
    pub section_index: u32,
    pub section_title: String,
    pub kind: SectionKind,
    pub line_start: u32,
    pub line_end: u32,
    pub note_type: NoteType,
    /// `status` als Aufzählung; `Some(Superseded)` heißt überholt (abgewertet, in der Zeile markiert).
    pub status: Option<NoteStatus>,
    pub project: Option<String>,
    /// `checked_on` der Notiz, sonst Datum der Änderungszeit.
    pub date: Option<String>,
    /// Preis von `wb read <address>` in Token.
    pub tokens: u32,
    pub score: f64,
    /// Satzgenauer Ausschnitt (Leerraum zusammengefasst).
    pub snippet: String,
    /// Byte-Bereiche der Trefferwörter in `snippet`.
    pub highlights: Vec<(usize, usize)>,
    /// Weitere passende Abschnitte derselben Notiz, die wegen `per_note` nicht gelistet sind.
    pub more_in_note: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SearchResult {
    pub query: String,
    pub terms: Vec<QueryTerm>,
    /// Zahl aller Treffer nach Filtern und Zusammenfassen je Notiz.
    pub total: u32,
    pub offset: usize,
    pub hits: Vec<SearchHit>,
}

/// Ergebnis von [`crate::Vault::sync`].
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncReport {
    pub scanned: u32,
    pub added: u32,
    pub changed: u32,
    pub removed: u32,
    /// Nur die Änderungszeit war anders, der Inhalt gleich.
    pub touched: u32,
    pub full_rebuild: bool,
    /// Laufzeit in Millisekunden (nur für Menschen und Messungen, nie in stdout; K13).
    #[serde(skip_serializing)]
    pub duration_ms: u64,
    /// Dateien, die nicht gelesen werden konnten (Pfad, Grund).
    pub errors: Vec<(String, String)>,
}

impl SyncReport {
    pub fn is_noop(&self) -> bool {
        self.added == 0 && self.changed == 0 && self.removed == 0 && self.touched == 0 && !self.full_rebuild
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
    Info,
}

/// Befund einer Prüfregel.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    pub path: NotePath,
    pub line: u32,
    /// Stabiler Code, z. B. `link-dead`, `fm-missing`.
    pub code: String,
    pub severity: Severity,
    pub message: String,
    pub hint: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CheckOptions {
    /// Nur Notizen unter diesem Pfadpräfix.
    pub prefix: Option<String>,
    /// Bezugsdatum `JJJJ-MM-TT` für „veraltet“ (ohne Angabe keine Altersregel, damit die Ausgabe
    /// deterministisch bleibt).
    pub today: Option<String>,
    /// Tage, ab denen `checked_on` als veraltet gilt (Standard 60).
    pub stale_days: Option<u32>,
}

/// Journalzeile (`Journal/*.md` oder altes `System/Wiki Log.md`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JournalEntry {
    pub date: String,
    pub time: Option<String>,
    /// Erstes Linkziel der Zeile.
    pub target: Option<String>,
    pub project: Option<String>,
    pub text: String,
    pub file: NotePath,
    pub line: u32,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct JournalFilter {
    pub project: Option<String>,
    /// Notizadresse (Linkziel); passt auf Zeilen, die diese Notiz verlinken.
    pub note: Option<String>,
    pub since: Option<String>,
    pub until: Option<String>,
    /// Neueste zuerst, höchstens so viele (0 = alle).
    pub limit: usize,
}

/// Wofür `context` den Einstieg sucht.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ContextTarget {
    /// Projektname (Groß/Klein und Umlaute egal, eindeutiger Präfix genügt).
    Project(String),
    /// Arbeitsordner; Zuordnung über `paths` der Übersichten.
    Cwd(String),
}

/// Abschnittstext für `context`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SectionText {
    pub address: String,
    pub title: String,
    pub kind: SectionKind,
    pub line_start: u32,
    pub line_end: u32,
    pub tokens: u32,
    /// Text ohne die Überschriftszeile.
    pub text: String,
}

/// Themennotiz in `context`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TopicLine {
    pub path: NotePath,
    pub title: String,
    pub summary: Option<String>,
    pub date: Option<String>,
    pub tokens: u32,
    pub kinds: Vec<SectionKind>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProjectContext {
    pub project: ProjectInfo,
    /// Wie das Projekt gefunden wurde, z. B. `name`, `name-prefix`, `paths:G:/X`, `vault-folder`.
    pub matched_by: String,
    pub overview: Option<NoteMeta>,
    pub state: Vec<SectionText>,
    pub open: Vec<SectionText>,
    /// Neueste zuerst (Datum absteigend, dann Pfad). Ohne überholte Notizen.
    pub topics: Vec<TopicLine>,
    /// Zahl der überholten Notizen (`status: superseded`), die in `topics` fehlen (§13.10).
    pub superseded: u32,
    /// Neueste zuerst.
    pub journal: Vec<JournalEntry>,
}

// ---------------- Schreibweg ----------------

/// Auftrag für [`crate::Vault::new_note`].
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct NewNote {
    /// Titel (H1 und Dateiname). Für eine Übersicht optional (dann der Projektname).
    pub title: Option<String>,
    pub note_type: NoteType,
    /// Pflicht für overview und topic; bei topic muss es das Projekt geben.
    pub project: Option<String>,
    pub status: Option<NoteStatus>,
    /// `checked_on`; Standard: heute (Ortszeit).
    pub date: Option<String>,
    /// overview: Projektwurzeln auf der Platte.
    pub paths: Vec<String>,
    /// source: Adresse der Quelle.
    pub source_url: Option<String>,
    /// topic: Beschreibung für den Link in der Übersicht (`- [[Pfad|Beschreibung]]`); sie ist
    /// der Text, den `wb context` in der Themenliste zeigt (§23.1).
    #[serde(default)]
    pub summary: Option<String>,
    /// topic: die Übersicht nicht anfassen, nur den Link nennen.
    #[serde(default)]
    pub no_link: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NewNoteReport {
    pub path: NotePath,
    /// Fertiger Wikilink, z. B. `[[Projects/X/Titel]]`.
    pub link: String,
    /// Übersicht, in der die neue Themennotiz verlinkt wird.
    pub overview: Option<NotePath>,
    /// Listenzeile für den Themenabschnitt der Übersicht, z. B. `- [[Projects/X/Titel|Beschreibung]]`.
    #[serde(default)]
    pub entry: Option<String>,
    /// Zeile der Übersicht, in der der Link steht (von `wb new` eingetragen oder schon vorhanden).
    /// `None`: nicht verlinkt, `entry` ist von Hand einzutragen.
    #[serde(default)]
    pub linked_line: Option<u32>,
}

/// Auftrag für [`crate::Vault::append_journal`].
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct NewJournalEntry {
    pub text: String,
    /// Notizadresse; wird aufgelöst und als `[[Pfad]]` geschrieben.
    pub note: Option<String>,
    /// Standard: heute (Ortszeit).
    pub date: Option<String>,
    /// `HH:MM`, optional (Vertrag §13.3).
    pub time: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JournalWrite {
    /// Monatsdatei, z. B. `Journal/2026-11.md`.
    pub file: NotePath,
    /// Zeile der neuen Zeile (1-basiert).
    pub line: u32,
    /// Die geschriebene Zeile ohne Zeilenumbruch.
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MoveReport {
    pub from: NotePath,
    pub to: NotePath,
    /// Umgeschriebene Links insgesamt.
    pub links: u32,
    /// Geänderte Notizen (neuer Pfad) mit Zahl der umgeschriebenen Links, nach Pfad.
    pub changed: Vec<(NotePath, u32)>,
}
