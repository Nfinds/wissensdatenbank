//! Der Einstieg: ein Vault mit abgeleitetem Index öffnen und abfragen.

use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::model::*;

/// Wie ein Vault geöffnet wird.
#[derive(Clone, Debug)]
pub struct OpenOptions {
    /// Vault-Wurzel.
    pub root: PathBuf,
    /// Indexordner. `None` → `<root>/.wb/`.
    pub index_dir: Option<PathBuf>,
    /// Nur lesen: kein Schreibbefehl an Notizen; der Index muss außerhalb des Vaults liegen.
    pub read_only: bool,
    /// Indexordner anlegen, falls er fehlt. Ohne diese Angabe wird `<root>/.wb/` nie
    /// stillschweigend angelegt (Schutz fremder Ordner, z. B. des alten Wikis).
    pub create_index: bool,
    /// Lässt sich die Indexdatei nicht als Index lesen (keine Datenbank, beschädigt), sie
    /// verwerfen und neu anlegen statt zu scheitern (`wb sync --full`, §15.8).
    pub discard_broken: bool,
}

impl OpenOptions {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        OpenOptions { root: root.into(), index_dir: None, read_only: false, create_index: false, discard_broken: false }
    }
    pub fn index_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.index_dir = Some(dir.into());
        self
    }
    pub fn read_only(mut self, yes: bool) -> Self {
        self.read_only = yes;
        self
    }
    pub fn create_index(mut self, yes: bool) -> Self {
        self.create_index = yes;
        self
    }
    pub fn discard_broken(mut self, yes: bool) -> Self {
        self.discard_broken = yes;
        self
    }
}

/// Geöffnetes Vault. Ein `Vault` gehört einem Thread (`Send`, nicht `Sync`); mehrere Prozesse
/// dürfen dasselbe Vault gleichzeitig öffnen (SQLite WAL, Schreiber warten aufeinander).
pub struct Vault {
    pub(crate) root: PathBuf,
    pub(crate) index_dir: PathBuf,
    pub(crate) read_only: bool,
    pub(crate) db: crate::index::Db,
    /// Ein unterbrochenes `wb mv`, das sich beim Öffnen weder abschließen noch zurücknehmen ließ.
    pub(crate) pending_problem: Option<String>,
}

impl Vault {
    /// Öffnet das Vault und den Index (legt das Schema an, baut aber nicht auf: dafür [`Vault::sync`]).
    pub fn open(opts: OpenOptions) -> Result<Vault> {
        crate::index::open_vault(opts)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn index_dir(&self) -> &Path {
        &self.index_dir
    }
    pub fn is_read_only(&self) -> bool {
        self.read_only
    }
    /// Text für eine `note:`-Zeile, wenn ein unterbrochenes `wb mv` hängt (weder abschließbar
    /// noch zurücknehmbar). Lesebefehle laufen trotzdem; Schreibbefehle verweigern mit Exit 3.
    pub fn pending_problem(&self) -> Option<&str> {
        self.pending_problem.as_deref()
    }

    /// Lesesicht: bis der zurückgegebene Wächter fällt, sehen alle Abfragen dieses Vaults
    /// denselben Indexstand, auch wenn ein anderer Prozess gleichzeitig synct (§15.1).
    /// In der Lesesicht kein `sync` und kein Schreibbefehl aufrufen.
    pub fn read_view(&self) -> Result<ReadView<'_>> {
        self.db.conn.execute_batch("BEGIN DEFERRED")?;
        // Die erste Abfrage legt den Stand fest.
        if let Err(e) = self.db.conn.query_row("SELECT count(*) FROM meta", [], |r| r.get::<_, i64>(0)) {
            let _ = self.db.conn.execute_batch("ROLLBACK");
            return Err(e.into());
        }
        Ok(ReadView { v: self })
    }

    /// Gleicht den Index an die Dateien an (neu, geändert, gelöscht). Ändert nie Notizen.
    /// Billig, wenn nichts geändert ist (nur Verzeichnislauf mit Größe/Änderungszeit).
    pub fn sync(&self) -> Result<SyncReport> {
        crate::sync::sync(self, false)
    }
    /// Index verwerfen und neu aufbauen.
    pub fn rebuild(&self) -> Result<SyncReport> {
        crate::sync::sync(self, true)
    }

    /// Notizen mit Metadaten, nach Pfad sortiert.
    pub fn notes(&self, filter: &NoteFilter) -> Result<Vec<NoteMeta>> {
        crate::query::notes(self, filter)
    }
    /// Metadaten einer Notiz.
    pub fn meta(&self, path: &NotePath) -> Result<NoteMeta> {
        crate::query::meta(self, path)
    }
    /// Adresse auflösen: Pfad (mit/ohne `.md`, `\` oder `/`, absolut im Vault), Pfadende,
    /// Dateiname oder Titel. Ein `#Abschnitt` am Ende wird ignoriert (siehe [`split_address`]).
    pub fn resolve(&self, address: &str) -> Result<NotePath> {
        crate::query::resolve_address(self, address).map(|x| x.0)
    }
    /// Adresse in Notiz und Abschnitt zerlegen. Ein `#` im Dateinamen (`C# Grundlagen`) zählt
    /// zum Namen, wenn der Teil davor so als Pfad existiert; sonst trennt das erste `#`.
    pub fn resolve_address(&self, address: &str) -> Result<(NotePath, Option<String>)> {
        crate::query::resolve_address(self, address)
    }
    /// Notiz frisch von der Platte lesen und zerlegen.
    pub fn note(&self, path: &NotePath) -> Result<Note> {
        crate::query::note(self, path)
    }
    /// Abschnitt einer Notiz finden: Titel (gefaltet, exakt vor Präfix), `L<Zeile>` oder
    /// Nummer `3`.
    pub fn find_section<'a>(&self, note: &'a Note, spec: &str) -> Result<&'a Section> {
        crate::query::find_section(note, spec)
    }

    /// Projekte (Ordner unter `Projects/`) mit Kennzahlen, nach Name sortiert.
    pub fn projects(&self) -> Result<Vec<ProjectInfo>> {
        crate::query::projects(self)
    }

    /// Passagensuche.
    pub fn search(&self, q: &SearchQuery) -> Result<SearchResult> {
        crate::search::search(self, q)
    }

    /// Ein- und ausgehende Links einer Notiz.
    pub fn links(&self, path: &NotePath) -> Result<NoteLinks> {
        crate::query::links(self, path)
    }

    /// Alle ausgehenden Links des Vaults (Quelle, Zeile sortiert) in einer Abfrage;
    /// für Graphen statt `links()` je Notiz.
    pub fn all_links(&self) -> Result<Vec<LinkInfo>> {
        crate::query::all_links(self)
    }

    /// Prüfregeln (Befunde nach Pfad, Zeile, Code sortiert).
    pub fn check(&self, opts: &CheckOptions) -> Result<Vec<Finding>> {
        crate::check::check(self, opts)
    }

    /// Journalzeilen, neueste zuerst.
    pub fn journal(&self, f: &JournalFilter) -> Result<Vec<JournalEntry>> {
        crate::query::journal(self, f)
    }

    /// Journalzeilen je Tag (und Projekt), Datum aufsteigend.
    pub fn activity(&self, f: &ActivityFilter) -> Result<Vec<DayActivity>> {
        crate::query::activity(self, f)
    }

    /// Einstieg für ein Projekt (Name oder Arbeitsordner).
    pub fn context(&self, target: &ContextTarget) -> Result<ProjectContext> {
        crate::query::context(self, target)
    }

    /// Notiz atomar schreiben (temporäre Datei + Umbenennen). Verweigert im Nur-Lese-Modus.
    /// Der Index wird beim nächsten `sync` nachgezogen.
    pub fn write_note(&self, path: &NotePath, content: &str) -> Result<()> {
        if self.read_only {
            return Err(Error::ReadOnly { action: format!("Schreiben von {path}"), hint: None });
        }
        let full = self.root.join(path.as_str());
        crate::fsutil::write_atomic(&full, content.as_bytes()).map_err(|e| Error::io(&full, e))
    }

    /// Neue Notiz mit gültigem Frontmatter anlegen (nie überschreiben); danach `sync`.
    pub fn new_note(&self, n: &NewNote) -> Result<NewNoteReport> {
        crate::write::new_note(self, n)
    }

    /// Journalzeile `- JJJJ-MM-TT [HH:MM] [[Notiz]] Text` an `Journal/JJJJ-MM.md` anhängen
    /// (exklusive Dateisperre, eine ganze Zeile je Schreibaufruf).
    pub fn append_journal(&self, e: &NewJournalEntry) -> Result<JournalWrite> {
        crate::write::append_journal(self, e)
    }

    /// Notiz umbenennen oder verschieben und alle eingehenden Links mitziehen (auch im Journal).
    /// Alias, `#Überschrift`, Einbettung und `\|` in Tabellen bleiben, Code bleibt unberührt.
    /// Ziel ohne `/`: gleicher Ordner; mit `/` am Ende: in diesen Ordner; sonst ab Vault-Wurzel.
    pub fn move_note(&self, from: &str, to: &str) -> Result<MoveReport> {
        crate::write::move_note(self, from, to)
    }

    /// Absoluter Dateipfad einer Notiz.
    pub fn file_path(&self, path: &NotePath) -> PathBuf {
        crate::query::fs_path(self, path)
    }
}

/// Wächter von [`Vault::read_view`]; beendet die Lesetransaktion beim Fallen.
pub struct ReadView<'a> {
    v: &'a Vault,
}

impl Drop for ReadView<'_> {
    fn drop(&mut self) {
        let _ = self.v.db.conn.execute_batch("COMMIT");
    }
}

/// Zerlegt `Notiz#Abschnitt` in (Notiz, Abschnitt) am ersten `#`, ohne Index.
/// [`Vault::resolve_address`] berücksichtigt auch `#` im Dateinamen.
pub fn split_address(address: &str) -> (&str, Option<&str>) {
    let a = address.trim();
    let a = a.strip_prefix("[[").and_then(|x| x.strip_suffix("]]")).unwrap_or(a);
    let a = a.split('|').next().unwrap_or(a);
    match a.find('#') {
        Some(i) => (&a[..i], Some(&a[i + 1..]).filter(|s| !s.is_empty())),
        None => (a, None),
    }
}
