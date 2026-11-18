# wb für Agenten

`wb` ersetzt Glob, Grep und das Lesen ganzer Notizen. Jede Ausgabe nennt ihren Preis in Token
(`~Nt`, geschätzt als Zeichen / 3,5) und eine Adresse, mit der man genau den Teil liest, der zählt.

## Ablauf

1. **Einstieg:** `wb context <projekt>` oder `wb context --cwd <arbeitsordner>`. Liefert Stand,
   Offenes, Pfade, die Themenliste (neueste zuerst, je Thema Datum, Preis, Abschnittsarten und ein
   Einzeiler) und die letzten Journalzeilen, zusammen höchstens 1500 Token (`--budget`). Überholte
   Themen werden nur gezählt (`superseded: N (wb search -p <projekt> -s superseded <begriffe>)`).
   Ohne Projekt und ohne passenden Arbeitsordner kommt eine kurze Projektliste (Name, Notizen,
   Token, Datum, offene Punkte, Status), neueste zuerst.
2. **Suchen:** `wb search <begriffe>`. Treffer sind Abschnitte, nicht Dateien: Rang, Adresse, Zeilen,
   Art, Datum, Preis, ein satzgenauer Ausschnitt. Filter: `-p <projekt>`, `-t overview|topic|source|other`,
   `-s active|paused|done|superseded`, `-k state|open|decisions|lessons|evidence|sources|paths|topics`,
   `--prefix Sources/Web/`. Quellen und überholte Notizen zählen je ×0,7; überholte tragen
   `superseded` in der Trefferzeile. Fragt ein Filter genau danach, entfällt der Faktor.
3. **Lesen:** `wb read "<adresse>"` liest genau den Abschnitt aus der Trefferzeile. Größere Notizen
   kommen erst als Gliederung (`--outline`), dann gezielt per `#Titel`, `#L<zeile>`, `--part open`
   oder `--lines A-B` (auch `A`, `A-`, `A:S-B` ab Zeichen S, mehrere mit Komma). `--links` hängt ein-
   und ausgehende Links an.

Überblick über alle Projekte: `wb stats` (Text, `--width 40..240`, `--ascii`) oder `wb stats --json`.
Prüfbefunde: `wb check` (Fehler zuerst; Hinweise über den ganzen Vault je Regel in einer Zeile,
einzeln mit `wb check <Ordner>` oder `--all`). Menschen: `wb ui` (`--ascii` für Konsolen ohne
Unicode-Schrift).

## Schreiben

- **Neue Notiz:** `wb new "<Titel>" -p <Projekt> --summary "<ein Satz, wozu die Notiz da ist>"
  [-s active|paused|done|superseded] [--date JJJJ-MM-TT]`. `wb new` trägt das Thema selbst in die
  Themenliste der Übersicht ein (`- [[Projects/<Projekt>/<Titel>|<Satz>]]`, ohne `--summary` der bloße
  Link) und meldet `linked: <Übersicht> L<Zeile>`; die Übersicht muss dafür nicht gelesen werden. Der
  Satz ist der Text, den `wb context` in der Themenliste zeigt. Nur wenn stattdessen `next:` erscheint
  (Übersicht ohne Abschnitt „Themen“, Themen in Unterabschnitten, `--no-link`), den genannten Eintrag
  von Hand einfügen. Übersicht für ein neues Projekt: `wb new -t overview -p <Projekt> [--path <ordner>]…`.
  Quelle: `wb new "<Titel>" -t source --url <adresse>` (`--summary` gilt nur für Themen, sonst Exit 2).
  `wb new` überschreibt nie, auch keine Schreibvariante (Exit 2).
- **Journal:** `wb log -n <notiz> "<text>"` hängt `- JJJJ-MM-TT [[notiz]] text` an `Journal/JJJJ-MM.md`
  an (`--time` mit Uhrzeit, ohne Text von stdin). Ein Datum, das es nicht gibt (`2026-02-30`), ist Exit 2.
- **Umbenennen:** nur `wb mv <notiz> <ziel>` (neuer Name, `Ordner/` oder Pfad ab Vault-Wurzel), nie
  per Hand. Alle eingehenden Links ziehen mit, Alias und `#Überschrift` bleiben, Code bleibt unberührt.
  Journaldateien lassen sich nicht umbenennen (Exit 2).
- **Genaue Adressen:** `wb mv` und `wb log -n` nehmen nur den vollen Pfad (`Projects/X/Notiz`, mit
  oder ohne `.md`) oder den vollen Namen (`Notiz`), nie Namensanfang oder Pfadende (Exit 1 mit
  Vorschlägen auf stderr). Groß/Klein und Umlaute bleiben egal.
- **Vorher geprüft:** zu langer Datei- oder Pfadname (255 Zeichen je Name, 259 für den Pfad), ein
  Zielordner, den es als Datei gibt, leeres oder vorhandenes Ziel: Exit 2, nichts angelegt.
- **Ein Schreiber zur Zeit:** `new`, `log` und `mv` halten eine Sperre für den ganzen Vault
  (`<vault>/.wb-lock`). Ein zweiter Schreibbefehl wartet bis zu 5 s und endet sonst mit Exit 3
  (`ein anderer Schreibbefehl läuft`), ohne etwas zu ändern; einfach erneut ausführen.
- **Abgebrochenes `mv`:** scheitert ein Schritt, nimmt `wb mv` alles zurück (Exit 3, `vollständig
  zurückgenommen`, nichts geändert). Bricht der Prozess ab, schließt der nächste Aufruf den Umzug ab.
  Geht beides nicht (Datei in einem anderen Programm offen, keine Rechte), laufen `search`, `read`,
  `context` und `check` weiter mit `note: unterbrochenes wb mv hängt (…\.wb-mv-pending)`; Schreibbefehle
  enden mit Exit 3 und nennen die Datei. Ursache beheben, dann `wb sync`.
- Steuerzeichen in Titel, Projekt, Pfad, Adresse (`new`) und Journaltext (`log`) fallen beim
  Schreiben weg.
- Ausgaben: `created <pfad>` / `logged <datei> L<n>` / `moved A -> B links=N notes=M`, dazu `link:`,
  bei `new` `linked:` (in der Übersicht eingetragen) oder `next:` (Eintrag zum Nachtragen), bei `mv`
  `next:`. `new --json` nennt `entry` und `linked_line`. Im Nur-Lese-Modus verweigern alle drei mit
  Exit 4 und schreiben nichts.

## Ausgabezeilen

- **`more: wb …`** ist ein fertiger Befehl, der genau dort weitermacht, wo das Budget abschnitt. Er
  läuft unverändert in Git-Bash und PowerShell und trägt alle ergebnisbestimmenden Schalter mit,
  auch die auf der Befehlszeile gesetzten `--vault`, `--index`, `--read-only`, `--no-sync`
  (Umgebungsvariablen wiederholt er nicht). Eine lange Zeile ohne Satzende wird geteilt und mit
  `--lines Z:S-…` fortgesetzt.
- **`note: …`** erklärt (ignorierte Füllwörter, Trefferzahl ohne Filter, Befunde a–b von n). Steht
  `note: Argument mit Anführungszeichen: Befehl ist für Bash zitiert`, gilt die `more:`-Zeile nur in Bash
  (ein Argument mit `'` zusammen mit `$` oder Backtick lässt sich nicht für beide Shells zitieren).
- **`next: …`** nach Schreibbefehlen: der nächste sinnvolle Schritt. **`linked: <Übersicht> L<n>`** nach
  `wb new`: der Link steht in dieser Zeile der Übersicht, nichts weiter zu tun.
- **Budget:** `--budget` ist eine harte Obergrenze für Text und `--json`. Mindestwerte: search 80,
  read 60, context 200. Ein kleineres Budget wird angehoben und in der Kopfzeile als
  `budget=N(angehoben)` genannt (JSON `budget_raised`). Kopfzeile und erster Treffer kommen immer.
- Steuerzeichen aus Notizen erscheinen sichtbar ersetzt (ESC als `␛`, Zeilentrenner U+2028 und
  U+2029 als `␤`, Richtungszeichen wie U+202E als `�`).
- `read --json` kommt an sehr langen Zeilen ohne Satzende je Aufruf nur wenig voran (Feldnamen und
  `\uXXXX` kosten mit); dafür die Textform nehmen.

## Format (warum so)

- **Eine Zeile je Treffer, Felder in fester Reihenfolge, ohne Rahmen und Farben:** Agenten lesen
  Text billiger als JSON, und `grep`/`head` bleiben brauchbar.
- **Adresse statt Pfad und Zeilennummer:** `Projects/X/Notiz#Abschnitt` bleibt gültig, wenn sich
  Zeilen darüber verschieben; die Zeilen (`L15-26`) stehen trotzdem dabei.
- **Preis vor dem Lesen:** `~660t` an jedem Treffer erlaubt die Entscheidung, ob sich das Lesen lohnt.
- **Bytegleich bei gleichem Stand:** keine Uhrzeiten oder Laufzeiten in stdout, Gleichstand nach Pfad.
- **`--json`** für strenge Parser (reines ASCII mit `\uXXXX`, ein Treffer, Thema oder Journaleintrag je
  Zeile, leere Felder fehlen), **`--human`** für Menschen. `search --json`: Kopf `query`, `total`, `offset`,
  `budget`, `budget_raised`, `shown`, `more`; je Treffer `rank`, `address` (Notizpfad ohne `.md` vor dem `#`),
  `line_start`, `line_end`, `kind` (fehlt bei `other`), `note_type`, `status`, `project`, `date`, `tokens`,
  `snippet`, `more_in_note`. `terms` steht nur, wenn ein Begriff korrigiert (`corrected`), als Füllwort
  verworfen (`stopword`) oder ohne Treffer (`sections: 0`) ist.
- **Preis der JSON-Form:** Feldnamen und `\uXXXX` machen sie rund 1,5-mal so teuer wie Text. Beim
  Standardbudget zeigt `search --json` im Median 5 statt 7 Treffer, `context --json` rund zwei Drittel
  der Themen, `read --json` eines Abschnitts kostet etwa 20 % mehr. Wer nur liest, nimmt Text.

## Adressen

`Notiz`, `Projects/X/Notiz`, mit oder ohne `.md`, Groß/Klein und Umlaute egal (`Pruefung` = `Prüfung`),
Backslashes erlaubt. Abschnitt dahinter mit `#`: Titel (Anfang genügt), `L<zeile>` oder Nummer aus
`--outline`. Überschriften mit `#` im Titel adressiert die Suche als `#L<zeile>`. Mehrdeutig → Exit 2
mit Kandidaten auf stderr. Adressen außerhalb des Vaults werden nie gelesen.

## Exit-Codes und Fehler

- 0 ok, 1 nichts gefunden oder leer (auch `--lines` hinter dem Ende), 2 falscher Aufruf oder
  mehrdeutig, 3 Vault-/Indexfehler, 4 im Nur-Lese-Modus verweigert (auch: Index läge im Vault,
  ebenso über eine Verknüpfung).
- `wb check` endet mit 0, wenn die Prüfung lief; `--strict` gibt 1 bei Befunden ab Warnung; ein
  Pfad, unter dem keine Notiz liegt, ist Exit 1. Fortsetzung mit `--offset`. Reihenfolge: Fehler,
  Warnungen, Hinweise, darin nach Pfad und Zeile. Ohne Pfadpräfix stehen Hinweise (`info`, etwa
  `orphan`, `note-large`) gebündelt als `info <regel> <n>: <Ordner> <n>, … | hint: …`; `wb check
  <Ordner>` und `--all` listen jeden einzeln. `check --json` ist immer vollständig:
  `total`, `counts` (je Regel), `offset`, `shown`, `more`, `findings` (beachtet `--limit`/`--offset`).
  Regel `secret-suspect` nennt Pfad, Zeile und Klasse, nie den Wert.
- `--offset` hinter dem Ende (search, check) endet mit Exit 0 und leerer Seite, weil die Anfrage
  Treffer hat; Exit 1 heißt: die Anfrage selbst findet nichts.
- Fehler auf stderr als `error[<code>]: …` und immer eine Zeile `hint: …`.
- Suchbegriffe, die mit `-` beginnen, nach `--`: `wb search -- -begriff`.

## Shells

- Git-Bash: UTF-8 ohne weiteres.
- PowerShell 5.1 liest eingefangene Ausgabe (`$x = wb …`, Pipes) in der OEM-Codepage. Vorher
  `[Console]::OutputEncoding = [Text.Encoding]::UTF8` setzen oder `--json` nehmen (reines ASCII).
  Skripte mit Umlauten als UTF-8 mit BOM speichern.

## Index und Vault

- Vault: `--vault`, sonst `WB_VAULT`, sonst `%APPDATA%\wb\config.toml` (`vault = "…"`), sonst aufwärts
  der erste Ordner mit `.wb/`. Index: `<vault>/.wb/` oder `--index` / `WB_INDEX`.
- Vor jedem Lesebefehl gleicht `wb` den Index an (rund 5 ms ohne Änderung, ~20 ms je geänderter
  Datei); abschalten mit `--no-sync`. `wb sync --full` baut neu auf, auch einen beschädigten Index.
- Nur lesen: `--read-only` / `WB_READONLY=1` (Index muss dann außerhalb des Vaults liegen). Für einen fremden
  Bestand, der unverändert bleiben muss: `--vault <ordner> --index <ordner außerhalb> --read-only`.
