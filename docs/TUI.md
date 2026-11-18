# Menschen-Ebene (`wb-tui`): Entwurf

Besitzer: Sitzung B. Bezug: Vertrag §6 (vier Ansichten plus statische Übersicht) und §7 (Datenschnittstelle). Stand: 2026-11-10, gebaut; der Entwurf gilt, Abweichungen und Prüfstand stehen im Abschnitt „Stand des Baus“ am Ende.

## Leitsatz

Ein Mensch öffnet `wb ui` mit einer von drei Fragen: **Wo liegt was und was lebt?** (Übersicht, Projektkarte), **Wo stand das noch?** (Suche, Leser), **Wie hängt das zusammen?** (Graph). Jede Ansicht beantwortet genau eine dieser Fragen und sagt die Antwort in der **Kopfzeile als ganzen Satz** („48 Projekte · 14 aktiv · 12 in 7 Tagen geändert · Befunde in 8“). Alles darunter belegt den Satz. Was den Satz nicht belegt, fehlt.

Wichtigste Ansicht: **die Übersicht.** Sie ist der Einstieg, trägt die eigentliche Neuigkeit gegenüber Dateibaum und Obsidian (Verrottung und Aktivität auf einen Blick) und ist zugleich die statische Ausgabe `wb stats`. Die Suche ist die häufigste, aber keine neue Fähigkeit; sie muss nur schnell und lesbar sein.

## Befunde am echten Bestand, die das Design bestimmen

Quelle: Auswertung eines echten Wikis mit rund 1200 Notizen in 48 Projekten (nur lesend, nur aggregierte Zahlen).

| Befund | Folge für die Ansicht |
|---|---|
| 48 Projekte, sehr ungleich: das größte 227 Notizen / 1,34 MB, 24 Projekte haben höchstens 5 Notizen | Tabelle statt Kacheln, sortierbar, scrollbar; Größenbalken mit **Wurzelskala**, sonst sind 40 Projekte unsichtbar; exakte Zahl steht daneben |
| Aktivität extrem schief: 3 bis 157 Journalzeilen je Tag, 536 Einträge in 9 Tagen | Heatmap-Stufen nach **Quantilen der Tage mit Aktivität**, nicht linear zum Maximum; Sparkline je Projekt auf das **eigene** Maximum skaliert (Form), die Summe steht als Zahl daneben |
| Datenspanne nur 7 Wochen (2026-09-23 bis 2026-11-09) | Sparkline 12 Wochen, Heatmap 16 Wochen: leere Wochen links sind ehrlich, nicht ausgefüllt |
| Linkgraph sternförmig: Median 1 ausgehender Link, Hub `00 Web Sources` mit 440, eine Fachnotiz mit 77 eingehenden, 45 Waisen | Nie Gesamtgraph über Notizen. Nachbarschaft (1 bis 2 Schritte) mit **Obergrenze je Spalte** („+63 weitere“) |
| Projektebene: 31 von 48 Projekten sind verbunden (37 Paare über Querverweise, 13 über gemeinsame Quellen), 17 sind Inseln | Projektgraph ist klein genug zum Zeichnen; die Inseln sind selbst eine Aussage und werden als Liste gezeigt, nicht als verstreute Punkte |
| 441 Quellen, 103 nie verlinkt | Quellen sind Typ-Filter in der Suche und Marke im Graphen, keine eigene Ansicht |
| Absatz = eine Zeile, Zeilen über 300 Byte | Leser und Vorschau **brechen Umbrüche selbst um** (Wortumbruch nach Anzeigebreite); Treffer-Ausschnitte kommen satz- oder fensterweise aus dem Kern |
| 49 × `00 Overview.md` | Anzeige nie nur mit Dateinamen: Überschrift oder `Projekt › Titel`, Adresse pfadqualifiziert |
| `status` ist im alten Wiki Freitext (über 100 Werte), im neuen Vault die Aufzählung `active, paused, done, superseded` (Vertrag §13.5) | Die Aufzählung darf Spalte und Marke sein; der Freitext (`state`) nie, er steht nur als Textzeile im Kopf der Projektkarte |

## Gemeinsame Regeln

**Tastatur** (keine Maus). Überall außer beim Tippen in der Suche: `1` Übersicht, `2` Projektkarte, `3` Suche, `4` Graph (auch `F1` bis `F4`, diese auch beim Tippen), `/` Suche öffnen, `Esc` zurück (Verlauf), `?` Hilfe, `q` beenden, `e` Notiz im Editor des Nutzers öffnen (`$VISUAL`, `$EDITOR`, sonst `notepad`), `r` Daten neu laden. Auf/ab: Pfeile **und** `j`/`k`; Seite: `PgUp`/`PgDn`; Anfang/Ende: `Pos1`/`Ende` (`g` heißt Graph, nicht „Anfang“). **`Strg-C` beendet in jedem Zustand**, auch bei offener Hilfe und im zu kleinen Fenster. In der Suche sind `?`, `q` und alle Zeichen Eingabe; dort öffnet `F5` die Hilfe (`F5` tut das auch sonst). AltGr-Zeichen (`@ { } [ ] ~ |`, Backslash und Euro; Windows meldet sie als `Strg+Alt`) sind in der Suche tippbar, `Strg` allein bleibt Befehl (`Strg-U`, `Strg-N`, `Strg-P`). Die Fußzeile und die Hilfe nennen **nur Tasten, die im aktuellen Zustand wirken**; die Hilfe ist je Ansicht verschieden und scrollt mit `↑↓` und `PgUp`/`PgDn`, damit sie auch bei 60×16 ganz erreichbar ist. `r` und die Rückkehr aus dem Editor gleichen den Index mit den Dateien ab und verwerfen alle Zwischenspeicher der Oberfläche; das ist auch im Nur-Lese-Modus erlaubt (der Index liegt außerhalb des Vaults).

**Frische** (Alter der letzten Änderung): `●` frisch bis 7 Tage (grün), `◕` bis 30 Tage (gelbgrün), `◐` bis 90 Tage (bernstein), `○` älter (rot), `·` unbekannt (grau); in ASCII `*` `+` `o` `-` `.`. Der Zustand steckt **auch im Zeichen**, nicht nur in der Farbe; Zustände, die sich nur in der Farbe unterscheiden, gibt es nicht (ein Test prüft die fünf Zeichen auf Verschiedenheit). „Heute“ und alle Alter rechnen in **Ortszeit** (unter Windows aus der Zeitzone des Systems, `localtime.rs`), nicht in UTC, und jeder Zeitstempel mit der **Sommerzeitregel seines eigenen Datums**: eine Änderung am 2026-08-01 um 00:30 Uhr zählt als 1. August, eine am 2026-11-10 um 00:30 Uhr als heute (Tests mit einer festen Zonenregel, unabhängig von der Zone des Rechners). Die Schwellen sind ein Anfangswert und stehen an einer Stelle (`model::Freshness`).

**Befunde** (aus `check`, Vertrag §5): vier Spalten mit festem Platz, Spaltenkopf `V G T W` = **V**eraltet (`checked_on` zu alt), **G**roß (Notiz oder Übersicht zu lang), **T**ote Links, **W**aise (nichts verlinkt hierher). Zelle = Anzahl (`1` bis `9`, `+` ab 10; die Legende sagt „+ = 10 und mehr“), `·` bei null, Farbe nach Schwere. Gezählt wird wie bei `wb check`: jeder Befund ab Warnung zählt, dazu die Hinweise V, G und W. Eine zu lange Übersicht (`overview-long`) zählt unter G. Arten ohne eigene Spalte (etwa mehrdeutiger Link, leere Notiz, Geheimnisverdacht, Fehler im Kopf der Notiz) gehen in die Summe ein und stehen als Zahl („5 Befunde, 1 andere“) im Detailfeld der Übersicht und in der Kopfzeile der Projektkarte; bei der gewählten Notiz nennt die Detailzeile ihre „andere Befunde“. „Befunde in N“ zählt Projekte mit mindestens einem solchen Befund. Die Legende steht neben der Heatmap (in `wb stats` bei schmaler Breite in einer Zeile darunter), in der Projektkarte unter der Liste und in der Hilfe.

**Größe**: Balken in Achtelblöcken (`▏▎▍▌▋▊▉█`), nie in Prozent beschriftet; daneben `~Tausend Token` (Zahl wie in der Agent-Ebene, Schätzformel des Kerns).

**Überholte Notizen** (`status: superseded`): überall gleich gekennzeichnet, auch ohne Farbe: Marke `×` (ASCII `x`) vor dem Titel, Zeile abgedunkelt, in der Suche zusätzlich „überholt“ in der zweiten Zeile und in der Vorschau, in der Projektkarte „überholt“ in der Detailzeile. Sie bleiben sichtbar (man muss sie finden können), drängen sich aber nicht auf.

**Status und Stand** (Vertrag §13.5): `status` ist die Aufzählung `aktiv | pausiert | fertig | überholt`; die Liste zeigt nur sie. Der alte Freitext (`state`, etwa „rumpf geschliffen, lackierung offen“) steht nur im Detailfeld bzw. in der Zeile „Stand“ der Projektkarte.

**Kleines Fenster (80×24)**: es wird nichts stillschweigend abgeschnitten. Spalten fallen in fester Reihenfolge weg (zuerst Größenangabe, Balken, Σ4W, Offen, Sparkline, Alter, Notizen; Name, Frische-Punkt und Befundspalten `V G T W` bleiben immer) und was nicht passt, wird mit `…` gekürzt (Namen, Detailzeilen, Zusammenfassungen); der Kopfsatz der Übersicht wählt die längste seiner Fassungen, die in die Breite passt. Die Legende der Befunde steht ab 60 Spalten (unter 26 freien Spalten neben der Heatmap in einer Zeile darunter, auch interaktiv). Im Leser unter 100 Spalten gehört die letzte Zeile dem Linkzähler („Tab: Links (n aus, m ein)“); der Text endet eine Zeile davor und wird nie überschrieben (Test bei 60 bis 99 Spalten). Die Spaltenwahl richtet sich nach der Breite, nie nach dem Inhalt; die Anzeigebreite (nicht Bytes) hat ein Test für alle Breiten von 40 bis 200 (`wb stats`) und von 60 bis 200 (alle Ansichten, Höhen 16, 24, 45). Unter 60×16 zeigt die Oberfläche statt der Ansicht einen Hinweis „Fenster zu klein, mindestens 60×16“, ohne zu stürzen; in diesem Zustand wirken nur `Strg-C` und `q` (dieselbe Grenze wie beim Zeichnen, getestet ab 1×1). **Ohne Farbe** (`NO_COLOR` gesetzt und nicht leer, in `wb ui` und `wb stats` gleich, `theme::no_color_env`): nur Modifikatoren (fett, invertiert, unterstrichen) und Zeichen. **ASCII-Rückfall** mit `--ascii` (`wb ui`, `wb stats`; alte Konsolenschriften): jede Zelle des fertigen Bildes wird auf ASCII gesetzt (`text::asciify_buffer`), also auch feste Texte, Umlaute (zu `a o u s`) und Notizinhalte; ein Test prüft alle Ansichten und `wb stats`. **Bekannte Grenze:** die Abbildung ersetzt Umlaute durch einen Buchstaben, feste Texte lesen sich daher verstümmelt („Größe“ wird „Grose“); ein Umbau der festen Texte auf Zweibuchstabenfassungen ist nicht vorgesehen.

**Bearbeiten**: nicht Aufgabe der TUI. `e` startet den Editor (`$VISUAL`, sonst `$EDITOR`, sonst `notepad` unter Windows, `vi` sonst), die TUI wartet und lädt danach neu. Die Zeile wird wie in einer Shell zerlegt: Leerraum trennt, doppelte und einfache Anführungszeichen fassen zusammen (`"C:\Program Files\Editor\ed.exe" --wait`), eine Zeile, die selbst eine vorhandene Datei ist, gilt als Programmpfad (Pfad mit Leerzeichen ohne Anführungszeichen). Das Programm wird im `PATH` gesucht, unter Windows mit den Endungen aus `PATHEXT`, damit `code --wait` (eine `.cmd`) startet. Getestet mit einem erfundenen Skript (`tests/review18.rs`); mit `notepad`, `code` und einem Programm unter „C:\Program Files“ in einem echten Terminal **nicht** geprüft (siehe Stand des Baus).

## Ansicht 1: Übersicht

**Frage:** Wo liegt mein Wissen, was lebt, was verrottet?

**Kopfzeile (Kernaussage):** `48 Projekte · 749 Notizen · 441 Quellen · 4.1M · 15 aktiv · 12 in 7 Tagen geändert · Befunde in 8`. **Ein Wort, eine Bedeutung (Vertrag §18.3):** „aktiv“ heißt nur `status: active` (die Aufzählung der Projekte); die zweite Zahl nennt ihren Zeitraum („in 7 Tagen geändert“). Der Filter `f` heißt in der Fußzeile „30 Tage“ und hängt „nur Änderung in 30 Tagen“ an den Kopfsatz. Bei Platzmangel fallen zuerst Größe und Quellen weg, dann kürzt sich „Tagen“ zu „T.“, zuletzt steht „N Projekte · A aktiv · W in 7 T. · B mit Befund“; Einzahl bei eins („1 Projekt“, „1 Notiz“, „1 Quelle“). Die Quellen stehen als eigene Zahl (Vertrag §13.10), weil sie mehr als die Hälfte der Notizen sind und sonst die Zahl der Themen verdecken. `wb stats` zeigt denselben Satz.

**Inhalt:**
- Tabelle aller Projekte, Standard sortiert nach Aktivität der letzten 4 Wochen. Spalten: Frische-Punkt, Name, Sparkline über 12 Wochen, `Σ4W`, Anzahl Notizen, Größenbalken samt Größe, Alter der letzten Änderung, offene Punkte (`Offen`-Abschnitte), Befundspalten `V G T W` und (ab 85 Spalten, nur wenn Projekte einen `status` tragen) die Spalte `Status` mit der Aufzählung.
- Darunter die **Aktivitäts-Heatmap**: Zeilen Mo bis So, Spalten Wochen (16, ab 110 Spalten 26), Zelle zwei Zeichen breit mit Schattierung `· ░ ▒ ▓ █` (fünf Stufen nach Quantilen), Monatsnamen über den Wochen (jeder Monat, der Platz hat; ein Monat ohne Platz wird nicht übersprungen, sondern in der nächsten Woche beschriftet). Eine Zeile Unterschrift mit Gesamtzahl, Spitzentag und Tagen mit Einträgen; bei schmaler Breite wird sie stufenweise kürzer (Spitze, Tage, Wochen), nie mitten im Wort abgeschnitten.
- Breites Fenster (ab 120 Spalten): rechts neben der Tabelle ein Panel zum markierten Projekt (`Stand`-Freitext, Projektwurzeln aus `paths`, das Feld „Offen“, Befundzahlen ausgeschrieben). **Offen** nennt dieselbe Zahl wie die Spalte (alle offenen Punkte des Projekts) und dahinter die erste Zeile des Offen-Abschnitts der Übersicht, auch wenn der Abschnitt keine Aufzählung ist: „Offen   72 · Kalibrierung nach dem Umbau …“.

**Tastatur:** `j`/`k`, `Enter` öffnet die Projektkarte des markierten Projekts, `s` wechselt die Sortierung (Aktivität, Größe, Alter, Name, Befunde), `S` kehrt die Richtung um, `f` blendet Projekte ohne Änderung in 30 Tagen aus/ein (Fußzeile „f 30 Tage“) (ohne Treffer steht „Keine Projekte mit Änderung in den letzten 30 Tagen“, ohne Projekte überhaupt „Noch keine Projekte“), `g` springt zum Graph (Projektebene) mit diesem Projekt.

**Bewusst weggelassen:**
- Tortendiagramm der Größen: schlechter lesbar als sortierte Balken und sagt nichts über Frische.
- Zeitreihen-Liniendiagramm der Gesamtaktivität: die Heatmap zeigt dasselbe mit Wochentag, und 7 Wochen Daten tragen keine Linie.
- Farbverlauf nach Projekt (jedes Projekt eine eigene Farbe): Farbe gehört dem Zustand, nicht der Identität. 48 Farben sind keine Information.
- Animationen, Lade-Spinner: Daten kommen in Millisekunden aus dem Index.

## Ansicht 2: Projektkarte

**Frage:** Was steckt in diesem Projekt, was ist frisch, was ist groß, was fehlt?

**Kopfzeile:** `Example · 227 Notizen · 1,3 MB · zuletzt vor 6 T · 3 offene Punkte`, darunter die Zeile `Stand [aktiv]  <Freitext>` (Aufzählung in Klammern, Freitext gekürzt).

**Inhalt:**
- Liste der Themen mit Frische-Punkt, Titel, Größenbalken (relativ zur größten Notiz **dieses** Projekts, damit bei kleinen Projekten die Form lesbar bleibt), Token, Alter und **Abschnittsmarken** `S O E L P Q` (**S**tand, **O**ffen, **E**ntscheidungen, **L**ehren/Sackgassen, **P**rüfungen, **Q**uellen): vorhandene Marke hell, fehlende `·`. Die Übersicht steht zuerst und trägt `Ü`. Befundspalten wie in Ansicht 1, ein Zeichen je Art. Unter der Liste stehen drei Detailzeilen zur Auswahl: Titel mit Einzeiler, Linkzahlen mit den vorhandenen Abschnitten (und „überholt“), und **die Legende der Abschnittsmarken** (`S Stand · O Offen · E Entscheidungen · L Lehren · P Prüfungen · Q Quellen · × überholt`; bei schmalem Fenster in Kurzform), damit niemand die Hilfe öffnen muss, um `SOELPQ` zu lesen.
- Sortierung: Standard „Übersicht, dann nach Änderung absteigend“; `s` wechselt (Änderung, Größe, Name, Befunde).
- Breites Fenster (ab 100 Spalten): rechts der **Nachbarschaftsgraph der markierten Notiz** (Eingehend | Notiz | Ausgehend, 1 Schritt) und darunter der Einzeiler (`summary`). Schmal: unter der Liste eine Zeile mit Einzeiler und Linkzahlen; `g` öffnet den Graph.

**Tastatur:** `j`/`k`, `Enter` öffnet die Notiz im Leser, `g` Graph der markierten Notiz, `s` Sortierung, `Tab` zwischen Projekten wechseln (nächstes in der Reihenfolge der Übersicht), `Esc` zur Übersicht.

**Bewusst weggelassen:**
- Verzeichnisbaum: die Projekte sind flach (Pfadtiefe 2 bei 1176 von 1194 Notizen), ein Baum hätte nur eine Ebene.
- Treemap der Notizgrößen: im Terminal zu grob für die Beschriftung; der sortierte Balken trägt dieselbe Aussage mit lesbaren Titeln.
- Eigene Quellenansicht je Projekt: Quellen sind Notizen mit Typ `source`, sie stehen in der Liste (Typ-Marke `Q` im Titel) und im Graph.

## Ansicht 3: Suche mit Vorschau (und Leser)

**Frage:** Wo stand das noch?

**Kopfzeile (Kernaussage):** `„backup restore“ · 3 von 17 Treffern in 2 Projekten`; ohne Anfrage die zuletzt geänderten Notizen als Ausgangsliste (damit die Ansicht nie leer ist).

**Inhalt Suche:** oben die Eingabezeile mit Cursor. Darunter die Trefferliste: Rang, Titel, `Projekt › Abschnitt`, Alter-Punkt. Rechts (ab 100 Spalten, sonst unter der Liste) die **Vorschau**: Fußzeile der Passage (`Projekt › Titel › Abschnitt · L21–34 · ~310 T`), darunter **die Passage aus der Notiz**: der Abschnitt des Treffers ab der ersten Zeile mit einem Suchwort (eine Zeile Vorlauf, `…` davor, wenn davor Text entfällt), umgebrochen bis zum Rand des Panels, die Suchwörter hervorgehoben (invertiert/gelb; Umlaute und Groß/Klein gefaltet, `ä` trifft `a`). Fehlt der Text, bleibt der Ausschnitt des Kerns. Notizen ohne Abschnittstreffer (Ausgangsliste) zeigen den Anfang nach dem Frontmatter. Die Anfrage wird bei jedem Tastendruck neu gestellt (Index ist schnell genug; die Ansicht bleibt dabei beim markierten Treffer, solange er noch vorkommt).

**Leser:** `Enter` auf einem Treffer öffnet die Notiz an der Passage. Markdown lesbar gesetzt (Überschriften, Listen, Zitate, Code, `**fett**`, `*kursiv*`, `` `code` ``, Wikilinks unterstrichen mit Nummer), umgebrochen auf Panelbreite. Rechts oder (schmal) auf `Tab` die Gliederung mit Abschnittsmarken und Token sowie die Linkliste (`Vorwärts` und `Rück`). In der Gliederung steht vor dem Abschnitt, in dem man gerade liest, ein Pfeil (`▸`, ASCII `>`), zusätzlich zur Abdunkelung der noch nicht erreichten Abschnitte. Links sind nummeriert und mit `Tab` (nächster), `Enter` (folgen) anspringbar; `Backspace` geht im Verlauf zurück. `]`/`[` springen zum nächsten/vorigen Abschnitt, `Leertaste` blättert.

**Tastatur Suche:** Tippen, `Backspace`, `↑`/`↓` (oder `Strg-N`/`Strg-P`, da Buchstaben in die Eingabe gehen), `Tab` wechselt den Typfilter (alle, Themen, Quellen), `Enter` öffnet, `Esc` leert die Anfrage, ein zweites `Esc` geht zurück.

**Bewusst weggelassen:**
- Fuzzy-Hervorhebung einzelner Buchstaben: der Kern liefert Trefferstellen als Wortbereiche; Buchstabengenauigkeit täuscht Präzision vor, die der Index (Stämme, Umlaut-Faltung) nicht hat.
- Suchverlauf, Lesezeichen, Tags: der Bestand hat keine; wer sie bräuchte, braucht sie im Vault, nicht in der Oberfläche.
- Syntax-Hervorhebung für Code: 14 Codeblöcke im ganzen Wiki.
- Bilder und Tabellen als Grafik: es gibt keine Anhänge; Tabellen erscheinen als abgesetzte Rohzeilen.

## Ansicht 4: Graph

**Frage:** Wie hängt das zusammen, was verweist auf was?

**Zwei Ebenen, Umschalter `p`:**

1. **Notizebene** (Standard, wenn man von einer Notiz kommt): Nachbarschaft der Fokusnotiz. Spalten: `Eingehend | Fokus | Ausgehend`, bei Tiefe 2 (`d`) eine weitere Spalte für die zweite Stufe, aber nur, wenn jede Spalte mindestens 14 Zeichen behält (sonst bleibt es bei Tiefe 1). Kanten sind Sammelschienen aus Linienzeichen (`─ │ ┬ ┴ ├ ┤ ┼ ┌ └ ┐ ┘`) in 5 Zeichen breiten Zwischenräumen mit Pfeilspitze `▸`, kein freies Layout: jede Beschriftung steht in einer Zeile und lässt ein Zeichen Abstand zur Verbindungslinie, zu lange Beschriftungen enden mit `…`; Beschriftungen überlagern sich nicht. Jede Spalte zeigt höchstens so viele Knoten, wie Zeilen da sind, nach Aktualität gereiht (zuletzt geänderte zuerst); der Rest steht als `… +N weitere`. Knoten tragen Frische-Punkt und Typmarke (`Ü` Übersicht, `Q` Quelle, `×` überholt, abgedunkelt). Die Köpfe der zweiten Stufe sind kurz: `Eingehend ·2`, `Ausgehend ·2`, `Verbunden ·2`. Pfeiltasten wählen, `Enter` setzt die Auswahl als neuen Fokus, `o` öffnet sie im Leser.
2. **Projektebene (Gesamtbild):** links (ab 100 Spalten) alle verbundenen Projekte nach Verbindungsstärke (Balken, Zahl), rechts das Ego-Netz des markierten Projekts: Nachbarprojekte nach Stärke, am Knoten die Art der Verbindung (`7L 3Q` = 7 Querverweise, 3 gemeinsame Quellen). Keine Linienstärken: die Zahl am Knoten trägt die Aussage, Linienstärke wäre doppelte Kodierung. Darunter eine Zeile `Ohne Verbindung (N): Namen …`, soweit sie passen. Das ist das Gesamtbild: nicht alle Kanten auf einmal, sondern die Rangfolge und von jedem Projekt aus die Nachbarn.

**Tastatur:** Pfeile/`hjkl` wählen einen sichtbaren Knoten (links/rechts wechselt die Spalte), `Enter` setzt den gewählten Knoten als Fokus (ohne Auswahl, am Fokus selbst, tut es nichts und steht nicht im Fuß; zurück mit `Esc`), `o` öffnet im Leser, `d` Tiefe, `p` Ebene, `Esc` zurück.

**Bewusst weggelassen:**
- Kräftebasiertes Layout mit allen 1194 Notizen: ein Haarball, der bei Median 1 Link je Notiz nichts zeigt und nicht reproduzierbar ist (jeder Lauf anders, Schnappschüsse nicht vergleichbar). Das Schienenlayout ist **deterministisch**.
- Zoomen, Verschieben, Mausziehen: Fokuswechsel ersetzt das Verschieben, und eine Taste je Schritt ist im Terminal präziser.
- Farbe nach Cluster oder Community: bei 17 Inseln und 31 locker verbundenen Projekten gibt es keine Cluster, die ein Algorithmus fände und ein Mensch nicht sähe.
- Kantenbeschriftung mit Linktext (Alias): 1450 Aliase, meist Titel; belegt nichts, was der Zielname nicht sagt.

## Statische Übersicht (`wb stats`)

Bibliotheksfunktionen, die A in die CLI einhängt: **`stats::render_for(source, width, ascii, tty) -> String`** (für `wb stats`; Farbe nur am Terminal und ohne `NO_COLOR`, `ascii` für `--ascii`), darunter `render_with(source, width, theme, ansi)` und das ältere `render(source, width, color)`. Ausgabe: Kopfsatz, die obersten 12 Projekte der Übersicht (ohne Detailpanel; sind es mehr, steht darunter „12 von 48 Projekten (nach Aktivität); `wb ui` zeigt alle“), die Heatmap bei jeder Projektzahl, die Legende (neben der Heatmap oder darunter). Breite: `--width N` mit **40 bis 240** (`stats::STATS_MIN_WIDTH`, `STATS_MAX_WIDTH`; kleinere Werte gelten als 40, größere als 240, der Hilfetext steht in `stats::STATS_WIDTH_HELP`); keine Zeile ist breiter als N. Ohne Angabe nimmt A die Terminalbreite oder 100, die Höhe ergibt sich aus dem Inhalt. Ohne Farbe keine Escape-Folgen; sonst ANSI mit 24-Bit-Farben. Deterministisch, sofern die Datenquelle es ist (Zeitpunkt „heute“ kommt aus der Datenquelle, nicht aus der Systemuhr).

## Schnappschüsse (Headless)

`snapshot::render_view(source, view, w, h, theme) -> Buffer`, daraus `to_text` (reiner Text, Zeilen rechts beschnitten) und `to_html` (farbtreu: je Zelle Vorder-/Hintergrund, fett/invertiert/unterstrichen, in einer `<pre>`-Seite mit Raster, ohne Skripte, ohne Netz). Das Beispielprogramm `snapshots` schreibt alle Ansichten bei 80×24 und 160×45 in einen Ordner. Die Golden-Tests unter `crates/wb-tui/tests/snapshots/` halten die Textform fest (`WB_TUI_BLESS=1` schreibt neu).

## Architektur in einem Absatz

`DataSource` ist die einzige Sicht auf Daten (angelehnt an Vertrag §7: Projektliste mit Kennzahlen, Notizliste, Notiz mit Abschnitten, Suche, Links, Aktivität je Tag, Projektkanten). `App` hält nur Oberflächenzustand (aktive Ansicht, Auswahl, Verlauf, Eingabe) und holt Daten bei Bedarf; Ansichten sind reine Funktionen `(Zustand, Daten, Rect) → Buffer`. Dadurch rendert der Headless-Pfad dieselbe Zeichenroutine wie das Terminal. Fixture-Quelle: eigene deterministische Zufallsfolge, erfundene Projekte, keine echten Inhalte.

## Stand

- `cargo test -p wb-tui` (mit und ohne `--features core`) und `cargo clippy` ohne Hinweise. Schnappschüsse mit erfundenen Daten: `crates/wb-tui/tests/snapshots/` (Text, `WB_TUI_BLESS=1` schreibt neu) und `cargo run -p wb-tui --example snapshots <ordner>` (farbtreues HTML); Beispiel zum Starten: `cargo run -p wb-tui --example demo`.
- Ortszeit über `GetTimeZoneInformation` (Windows), auf anderen Systemen derzeit UTC.
- Nicht geprüft: Windows Terminal und klassische Konsole mit echten Tasten, AltGr auf echter Tastatur, Mausbedienung, Linux und macOS.
