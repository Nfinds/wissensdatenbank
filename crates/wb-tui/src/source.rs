//! `DataSource`: die einzige Sicht der Oberfläche auf Daten (angelehnt an Vertrag §7).
//!
//! Alle Methoden liefern Eigentum (`Vec`, `String`), keine Verweise, damit eine Quelle
//! hinter einer Datenbank genauso passt wie die Fixture-Quelle. Die Oberfläche ruft nur
//! bei Bedarf (Ansicht öffnen, Auswahl ändern, Taste in der Suche) und cached nichts
//! über `reload` hinaus.

use crate::model::*;

pub trait DataSource {
    /// „Heute“ der Daten. Kommt aus der Quelle, nicht aus der Systemuhr, damit Schnappschüsse
    /// und Tests deterministisch bleiben.
    fn today(&self) -> Date;

    /// Kurzbeschreibung des Vaults für die Kopfzeile (z. B. `fixture` oder der Ordnername).
    fn label(&self) -> String;

    /// Alle Projekte mit Kennzahlen (Reihenfolge egal, die Oberfläche sortiert).
    fn projects(&self) -> Vec<ProjectInfo>;

    /// Notizen eines Projekts (Übersicht inklusive).
    fn notes(&self, project: &str) -> Vec<NoteInfo>;

    /// Eine Notiz mit Abschnitten und Rohtext.
    fn note(&self, path: &str) -> Option<NoteDoc>;

    /// Passagensuche. Leere Anfrage liefert nichts; die Oberfläche nimmt dann `recent`.
    fn search(&self, query: &str, limit: usize) -> Vec<SearchHit>;

    /// Ein- und ausgehende Links einer Notiz (aufgelöste Ziele, Reihenfolge wie im Text).
    fn links(&self, path: &str) -> NoteLinks;

    /// Journalzeilen je Tag für die letzten `days` Tage, **endend mit `today()`**, älteste zuerst.
    fn activity(&self, days: u32) -> Vec<u16>;

    /// Verbindungen zwischen Projekten (nur Paare mit Gewicht > 0).
    fn project_edges(&self) -> Vec<ProjectEdge>;

    /// Zahl der Quellnotizen des Vaults (liegen außerhalb der Projekte, zählen nicht in `notes`).
    fn sources(&self) -> u32 {
        0
    }

    /// Gleicht die Quelle mit der Außenwelt ab (bei einem Vault: den Index mit den Dateien) und
    /// verwirft ihre Zwischenspeicher. Die Oberfläche ruft das vor jedem Neuladen (`r`, nach dem Editor).
    fn refresh(&self) {}

    /// Absoluter Dateipfad einer Notiz (für „im Editor öffnen“); `None`, wenn es keine Datei gibt.
    fn file_path(&self, _path: &str) -> Option<std::path::PathBuf> {
        None
    }

    /// Zuletzt geänderte Notizen (Ausgangsliste der Suche).
    fn recent(&self, limit: usize) -> Vec<NoteInfo> {
        let mut all: Vec<NoteInfo> = Vec::new();
        for p in self.projects() {
            all.extend(self.notes(&p.name));
        }
        all.sort_by(|a, b| b.modified.cmp(&a.modified).then_with(|| a.path.cmp(&b.path)));
        all.truncate(limit);
        all
    }
}
