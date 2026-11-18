# Vertrag: Wissensbank (`wb`)

Verbindliche Schnittstellen und Entscheidungen. Diese öffentliche Fassung enthält die Abschnitte 3 bis 7 und 10 des Vertrags. Die übrigen Abschnitte (Befunde zum privaten Ausgangsbestand, Migration, Messung, Phasen, Prüfberichte und Nachträge §11 bis §23) beschrieben den privaten Entwicklungsablauf und sind nicht veröffentlicht; Verweise wie „§16“ in Quelltextkommentaren zeigen auf diesen Verlauf.

## 3. Namen und Aufbau

- Projektname „Wissensbank“, Binary `wb`.
- Cargo-Workspace:
  - `crates/wb-core`: Bibliothek. Notizmodell, Frontmatter- und Link-Parser, Abschnitte, Index, Sync, Suche, Graph, Prüfregeln, Journal.
  - `crates/wb-cli`: Binary `wb`. Agent-Ebene; `wb ui` ruft `wb-tui` (Cargo-Feature `ui`, standardmäßig an).
  - `crates/wb-tui`: Bibliothek mit den ratatui-Ansichten. Arbeitet gegen einen eigenen Trait `DataSource`; die Anbindung an `wb-core` liegt hinter dem Feature `core`, Entwicklung und Tests laufen mit synthetischer Fixture-Datenquelle.
- Die Wurzel-`Cargo.toml` nimmt `crates/*` als Mitglieder auf.
- Vault-Suche in dieser Reihenfolge: `--vault`, Umgebungsvariable `WB_VAULT`, Nutzerkonfiguration (`%APPDATA%\wb\config.toml`), dann vom Arbeitsordner aufwärts nach `.wb/`. Index standardmäßig `<vault>/.wb/`, überschreibbar mit `--index` oder `WB_INDEX`.

## 4. Vault-Format

Grundsatz: **Die bewährte Ordnung bleibt, die Metadaten werden streng.** Markdown-Dateien sind die Quelle der Wahrheit; alles unter `.wb/` ist abgeleitet und darf gelöscht werden.

```text
vault/
  Projects/<Projekt>/00 Overview.md     Landkarte (type: overview)
  Projects/<Projekt>/<Thema>.md         Themennotiz (type: topic)
  Sources/Web/<Quelle>.md               Webquelle (type: source)
  Journal/JJJJ-MM.md                    Änderungsjournal, eine Zeile je Eintrag
  .wb/                                  Index (abgeleitet)
```

- Dateien: UTF-8 ohne BOM, LF. Datei- und Ordnernamen bleiben wie im alten Wiki (auch `00 Overview.md`), damit Links und die Wiederholung der Migration einfach bleiben.
- Frontmatter (flache YAML-Teilmenge: Skalare, Inline-Listen, Block-Listen):
  - `type`: `overview` | `topic` | `source` | `rule`
  - `status` (optional, nur overview/topic): `active` | `paused` | `done` | `superseded`
  - `state` (optional): Freitext; hierhin wandert der alte `status`-Text unverändert
  - `checked_on`: `JJJJ-MM-TT`
  - `paths` (overview): echte Projektwurzeln auf der Platte, z. B. `["C:/Code/my-app"]`; Grundlage für die Zuordnung Arbeitsordner → Projekt
  - `aliases`, `summary` (eine Zeile) optional
  - `source_url`, `published_on` (source)
  - unbekannte Schlüssel bleiben beim Lesen und Schreiben erhalten
- Links: Wikilinks `[[Pfad/ab/Vault-Wurzel|Alias]]`, optional `#Überschrift`. Der Index löst sie auf und meldet tote oder mehrdeutige.
- Abschnittsarten: Der Index ordnet H2-Überschriften per Schlüsselwort einer Art zu, ohne den Text zu ändern: `state` (Stand, Aktueller Stand, Status), `open` (Offen, Offene Punkte, Open, Nächster Schritt), `decisions` (Entscheidung…), `lessons` (Sackgassen, Was funktioniert hat …, Stolpersteine), `evidence` (Prüfungen, Tests, Geprüft …), `sources` (Quellen, Primärquellen), `paths` (Pfade, Dateien, Zweck und Ort), `topics` (Themen). Datumszusätze wie „Stand (2026-10-16)“ werden erkannt.
- Journal: `- JJJJ-MM-TT HH:MM [[Notiz]] Text` in Monatsdateien. Es ersetzt `System/Wiki Log.md`, wird nie ganz gelesen, sondern über den Index gefiltert (Projekt, Notiz, Zeitraum).

## 5. Agent-Ebene

Verbindliche Prinzipien:

1. **Standardausgabe ist das Agent-Format:** reiner Text, UTF-8, LF, keine ANSI-Farben, keine Rahmen, feste Feldreihenfolge. Gleicher Vault-Stand und gleiche Eingabe ergeben bytegleiche Ausgabe (Gleichstand wird über den Pfad aufgelöst; keine Uhrzeiten oder Laufzeiten in stdout). Die Ausgabe hängt nicht davon ab, ob ein Terminal angeschlossen ist.
2. **Budget:** jeder Lesebefehl hat `--budget <Token>` mit sinnvollem Standard, packt das Wichtigste zuerst hinein und sagt ausdrücklich, was fehlt und mit welchem Befehl es weitergeht. Token werden geschätzt; die Schätzformel steht in der Hilfe.
3. **Passagen statt Dateien:** Treffer sind Abschnitte mit Zeilenbereich und satzgenauem Ausschnitt. Jede Trefferzeile trägt die Adresse, mit der `wb read` genau diese Stelle liefert, und den geschätzten Preis in Token.
4. **Zwei weitere Formen:** `--json` (dieselben Felder, für strenge Parser), `--human` (lesbar für Menschen, farbig).
5. **Fehler:** auf stderr als `error[<code>]: <was>` und `hint: <wie beheben>`; dokumentierte Exit-Codes (mindestens: ok, nichts gefunden, falscher Aufruf, Vault-/Indexfehler).
6. Keine Interaktivität außerhalb von `wb ui`.

Befehle nach Priorität (hart priorisiert; P3 erst, wenn P1 und P2 abgenommen sind):

| Prio | Befehl | Zweck |
|---|---|---|
| P1 | `wb sync` | Index inkrementell angleichen; läuft außerdem billig und automatisch vor jedem Lesebefehl (geänderte Dateien per Größe/Änderungszeit/Hash erkennen, nur diese neu indexieren) |
| P1 | `wb context [<Projekt> \| --cwd <Pfad>]` | Einstieg in einem Aufruf: Projekt über `paths` oder Namen finden, Stand und Offenes aus der Übersicht, Themenliste mit Einzeiler, Alter und Tokenpreis, letzte Journalzeilen |
| P1 | `wb search <Begriffe>` | gewichtete Passagensuche über Titel, Überschriften, Aliase, Pfad und Text; Filter nach Projekt, Typ, Abschnittsart; deutsch und englisch, Umlaut-Faltung, Stämme, Komposita/Tippfehler über Rückfall |
| P1 | `wb read <Notiz>[#Abschnitt]` | Gliederung mit Tokenpreisen, einzelner Abschnitt, Abschnittsart (`--part open`), Zeilenbereich; Rück- und Vorwärtslinks auf Wunsch |
| P1 | `wb check [<Pfad>]` | Prüfregeln mit Datei, Zeile, Code, Hinweis: Frontmatter, tote/mehrdeutige Links, zu lange Übersicht (über 100 Zeilen), veraltetes `checked_on`, verwaiste Notiz, übergroße Notiz. |
| P2 | `wb new` | Notiz mit gültigem Frontmatter anlegen (Projekt, Titel, Typ), Pfad ausgeben |
| P2 | `wb log` | Journalzeile anhängen (Text als Argument oder über stdin), atomar |
| P2 | `wb mv <alt> <neu>` | umbenennen/verschieben und alle eingehenden Links mitziehen |
| P3 | weitere Abfragen (`wb links`, `wb recent`) | nur wenn sie nicht schon in `read`/`context` aufgehen |

Schreiben: Agenten ändern Notizen weiter mit ihren eigenen Edit-Werkzeugen; der Index heilt sich beim nächsten Aufruf selbst. `wb` schreibt nur dort selbst, wo es Fehler verhindert (`new`, `log`, `mv`). Schreibende Befehle sind atomar (temporäre Datei plus Umbenennen) und verweigern im Nur-Lese-Modus.

Richtungsskizze der Suchausgabe (endgültiges Format in `Docs/AGENT-GUIDE.md`):

```text
q="backup restore" hits=3/17 ~410t
1 Projects/Example/Backup#Restore L14-18 topic 2026-01-01 ~60t
  … Copy the folder back with timestamps preserved, then run `wb sync --full` …
2 …
more: wb search "backup restore" --offset 3
```

## 6. Menschen-Ebene

Ziel: Ein Mensch sieht in Sekunden, wo sein Wissen liegt, was lebt, was verrottet, und findet eine Notiz ohne Dateibaum. Umfang hart begrenzt auf vier Ansichten in `wb ui` plus eine statische Übersicht:

1. **Übersicht:** Projekte nach Aktivität (Sparkline je Projekt über die letzten Wochen), Größe, Alter, offene Punkte, Zustand (veraltet, übergroß, tote Links, verwaist); Aktivitäts-Heatmap über Tage.
2. **Projektkarte:** Themen eines Projekts mit Größenbalken, Frische, Abschnittsarten als Marken; daneben der Linkgraph des Projekts.
3. **Suche mit Vorschau:** Tippen filtert sofort, rechts die Passage mit hervorgehobenen Treffern; Enter öffnet den Leser (Markdown lesbar gesetzt, Rück- und Vorwärtslinks anspringbar).
4. **Graph:** Nachbarschaft einer Notiz oder eines Projekts (1–2 Schritte) lesbar beschriftet; auf Projektebene das Gesamtbild (Projekte als Knoten, Verbindungen über Querverweise und gemeinsame Quellen).

- Statische Übersicht ohne Interaktion: `wb stats` (bzw. `--human`-Formen), für den schnellen Blick.
- **Headless-Schnappschuss** jeder Ansicht in eine Datei (Text und eine farbtreue Form, z. B. HTML oder SVG), damit Tests, Belege und die Abnahme ohne Terminal möglich sind. Tastenkürzel sichtbar, Bedienung ohne Maus, sauberes Verhalten bei kleinem Fenster (80×24) und ohne Farbunterstützung.
- Bearbeiten ist nicht Aufgabe der TUI: eine Taste öffnet die Notiz im Editor des Nutzers.

## 7. Schnittstelle `wb-core`

Benötigt werden mindestens:

- Vault öffnen (Pfad, Indexpfad, Nur-Lese), `sync()` mit Bericht (neu, geändert, gelöscht, Dauer).
- Projektliste mit Kennzahlen: Notizen, Bytes, Token, letzte Änderung, offene Punkte, Befunde je Art, Aktivität je Woche.
- Notizliste mit Metadaten (Pfad, Titel, Typ, Projekt, Status, `checked_on`, Änderungszeit, Token, Einzeiler, ein- und ausgehende Links).
- Notiz mit Abschnitten (Ebene, Titel, Art, Zeilenbereich, Token) und Rohtext.
- Suche (Anfrage, Filter, Budget) mit Treffern (Notiz, Abschnitt, Zeilenbereich, Ausschnitt, Trefferstellen für die Hervorhebung).
- Links einer Notiz (ein- und ausgehend), Befunde (`check`), Journal (gefiltert), Aktivität je Tag.
- Frontmatter lesen/schreiben (unbekannte Schlüssel erhalten), Wikilinks parsen und umschreiben, Notiz atomar schreiben.

## 10. Tests

Im Repo nur synthetische Fixtures. Pflichtfälle: Frontmatter fehlend, ungeschlossen, kaputtes YAML, doppelte Schlüssel, BOM, CRLF; leere Datei; riesige Datei (zur Testzeit erzeugt, zweistellige MB); Umlaute und ß in Namen und Text, NFC gegen NFD, Groß/Klein; Windows-Pfade (Backslash, Leerzeichen, über 260 Zeichen, Groß/Klein-Gleichheit, reservierte Namen); tote, mehrdeutige und Alias-/Überschriften-Links; gleiche Dateinamen in verschiedenen Ordnern; gelöschte und umbenannte Dateien beim Sync; zwei gleichzeitige Prozesse; Nur-Lese-Modus schreibt nachweislich nichts; Determinismus der Ausgabe.
