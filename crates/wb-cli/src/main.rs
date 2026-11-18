//! `wb` — Wissensbank-CLI. Standardausgabe ist das Agent-Format (deterministisch, budgetiert);
//! `--json` für strenge Parser, `--human` für Menschen.

mod fmt;
mod write;

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

use clap::parser::ValueSource;
use clap::{ArgMatches, Args, CommandFactory, FromArgMatches, Parser, Subcommand};
use wb_core::{Error, OpenOptions, Vault, exit};

// Standardwerte, gemeinsam für clap und die `more:`-Zeilen (nur abweichende Werte werden mitgeführt).
pub(crate) const SEARCH_LIMIT: usize = 8;
pub(crate) const SEARCH_PER_NOTE: usize = 2;
pub(crate) const SEARCH_BUDGET: u32 = 700;
pub(crate) const READ_BUDGET: u32 = 2000;
pub(crate) const CONTEXT_BUDGET: u32 = 1500;
pub(crate) const CONTEXT_JOURNAL: usize = 5;
pub(crate) const CONTEXT_JOURNAL_MAX: usize = 50;
pub(crate) const CHECK_LIMIT: usize = 50;

const AFTER_HELP: &str = "\
Ausgabe: Standard ist das Agent-Format (UTF-8, LF, ohne Farben, feste Feldreihenfolge,
bytegleich bei gleichem Stand). Token werden geschätzt: ~t = Zeichen / 3,5, aufgerundet.
Jede Trefferzeile trägt eine Adresse für `wb read` und den Preis in Token; was das Budget
abschneidet, nennt eine Zeile `more:` mit dem nächsten Befehl; er ist unverändert in Git-Bash
und PowerShell ausführbar und führt alle ergebnisbestimmenden Schalter mit. Erläuterungen stehen
in `note:`-Zeilen. `--budget` ist eine harte Obergrenze, auch mit `--json` und `--links`;
Kopfzeile und erster Treffer bzw. erster Textteil erscheinen immer. Mindestbudgets: search 80,
read 60, context 200; ein kleineres oder zu knappes Budget wird angehoben und in der Kopfzeile
als `budget=N(angehoben)` genannt (JSON: `budget_raised`). Lange Zeilen werden an einer
Satz- oder Zeichengrenze geteilt und mit `wb read <notiz> --lines Z:S-…` ab Zeichen S
fortgesetzt. Steuerzeichen aus Notizen erscheinen sichtbar ersetzt (ESC als ␛).
`--json` ist reines ASCII (Nicht-ASCII als \\uXXXX). PowerShell 5.1 liest eingefangene
Ausgabe sonst in der OEM-Codepage: vorher `[Console]::OutputEncoding = [Text.Encoding]::UTF8` setzen.

Vault: --vault, sonst WB_VAULT, sonst %APPDATA%\\wb\\config.toml (vault = \"…\"),
sonst aufwärts vom Arbeitsordner der erste Ordner mit `.wb/`.
Index: <vault>/.wb/, oder --index / WB_INDEX. Nur-Lese: --read-only / WB_READONLY=1
(Index muss dann außerhalb liegen). Vor jedem Lesebefehl gleicht `wb` den Index an
(abschaltbar mit --no-sync / WB_NO_SYNC=1).

Exit-Codes: 0 ok, 1 nichts gefunden (leeres Ergebnis), 2 falscher Aufruf oder mehrdeutig,
3 Vault-/Indexfehler, 4 im Nur-Lese-Modus verweigert (auch: Index läge im Vault).
`wb check` endet mit 0, wenn die Prüfung lief (Text und JSON gleich); mit --strict 1 bei
Befunden ab Warnung. Es nennt Fehler vor Warnungen vor Hinweisen; über den ganzen Vault
stehen Hinweise (info) je Regel in einer Zeile, einzeln mit `wb check <Ordner>`, --all
oder --json. Fehler: stderr `error[<code>]: …` und `hint: …`.
`wb new` trägt eine Themennotiz selbst in die Themenliste der Übersicht ein (`linked:`
mit Zeile; --summary gibt dem Link die Beschreibung, die `wb context` zeigt). Nur wenn
das nicht geht (kein Abschnitt „Themen“, Unterabschnitte, --no-link), nennt `next:` den
Eintrag zum Nachtragen.
Suchbegriffe, die mit `-` beginnen, nach `--` angeben: wb search -- -begriff";

#[derive(Parser)]
#[command(name = "wb", version, about = "Wissensbank: lokale Wissenssuche für Agenten und Menschen", after_help = AFTER_HELP)]
struct Cli {
    #[command(flatten)]
    g: Global,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Args, Clone)]
struct Global {
    /// Vault-Ordner
    #[arg(long, global = true, env = "WB_VAULT", value_name = "ORDNER")]
    vault: Option<PathBuf>,
    /// Indexordner (Standard <vault>/.wb)
    #[arg(long, global = true, env = "WB_INDEX", value_name = "ORDNER")]
    index: Option<PathBuf>,
    /// Nur lesen: schreibende Befehle verweigern, Index muss außerhalb liegen
    #[arg(long, global = true, env = "WB_READONLY", value_parser = clap::builder::FalseyValueParser::new())]
    read_only: bool,
    /// Index vor dem Befehl nicht angleichen
    #[arg(long, global = true, env = "WB_NO_SYNC", value_parser = clap::builder::FalseyValueParser::new())]
    no_sync: bool,
    /// JSON statt Agent-Text
    #[arg(long, global = true, conflicts_with = "human")]
    json: bool,
    /// Lesbar für Menschen (mit Farben, wenn ein Terminal angeschlossen ist)
    #[arg(long, global = true)]
    human: bool,
    /// Laufzeiten auf stderr ausgeben (für Messungen)
    #[arg(long, global = true, env = "WB_TIMING", value_parser = clap::builder::FalseyValueParser::new(), hide = true)]
    timing: bool,
}

#[derive(Subcommand)]
enum Cmd {
    /// Index an die Dateien angleichen (nur geänderte Notizen)
    Sync {
        /// Index verwerfen und neu aufbauen
        #[arg(long)]
        full: bool,
        /// <vault>/.wb anlegen, falls er fehlt
        #[arg(long)]
        init: bool,
    },
    /// Passagensuche (Abschnitte mit satzgenauem Ausschnitt)
    Search(SearchArgs),
    /// Notiz, Abschnitt, Abschnittsart oder Zeilenbereich lesen
    Read(ReadArgs),
    /// Einstieg in ein Projekt: Stand, Offenes, Themen, Journal
    Context(ContextArgs),
    /// Prüfregeln: Frontmatter, Links, Größe, Verwaisung, Alter
    Check(CheckArgs),
    /// Neue Notiz mit gültigem Frontmatter anlegen (nie überschreiben)
    New(NewArgs),
    /// Journalzeile anhängen (Text als Argument oder über stdin)
    Log(LogArgs),
    /// Notiz umbenennen/verschieben und alle eingehenden Links mitziehen
    Mv {
        /// Voller Pfad oder voller Name der Notiz (kein Namensanfang); Journaldateien nicht
        from: String,
        /// Neuer Name (gleicher Ordner), `Ordner/` (gleicher Name) oder Pfad ab Vault-Wurzel
        to: String,
    },
    /// Terminal-Ansichten für Menschen
    Ui {
        /// Nur ASCII-Zeichen (auch in Notizinhalten), für Konsolen ohne Unicode-Schrift
        #[arg(long)]
        ascii: bool,
    },
    /// Statische Übersicht aller Projekte (ohne Interaktion)
    Stats {
        /// Breite in Spalten, 40 bis 240 (kleinere Werte gelten als 40, größere als 240)
        #[cfg_attr(feature = "ui", arg(help = wb_tui::stats::STATS_WIDTH_HELP))]
        #[arg(long, default_value_t = 100)]
        width: u16,
        /// Alles als ASCII: Rahmen, Balken, Heatmap und jeder Text samt Projektnamen (Umlaute ohne Punkte)
        #[arg(long)]
        ascii: bool,
    },
}

#[derive(Args)]
pub(crate) struct SearchArgs {
    /// Suchbegriffe (deutsch/englisch, Umlaute egal)
    #[arg(required = true, num_args = 1..)]
    terms: Vec<String>,
    /// Nur in diesem Projekt
    #[arg(short, long)]
    project: Option<String>,
    /// Nur Notizart: overview | topic | source | other
    #[arg(short = 't', long = "type")]
    note_type: Option<String>,
    /// Nur Notizen mit diesem status: active | paused | done | superseded
    #[arg(short = 's', long)]
    status: Option<String>,
    /// Nur Abschnittsart: state | open | decisions | lessons | evidence | sources | paths | topics
    #[arg(short, long)]
    kind: Option<String>,
    /// Nur unter diesem Pfadpräfix, z. B. Sources/Web/
    #[arg(long)]
    prefix: Option<String>,
    /// Treffer überspringen
    #[arg(long, default_value_t = 0)]
    offset: usize,
    /// Höchstzahl Treffer
    #[arg(long, default_value_t = SEARCH_LIMIT)]
    limit: usize,
    /// Höchstens so viele Abschnitte je Notiz
    #[arg(long, default_value_t = SEARCH_PER_NOTE)]
    per_note: usize,
    /// Tokenbudget der Ausgabe (mindestens 80)
    #[arg(long, default_value_t = SEARCH_BUDGET)]
    budget: u32,
}

#[derive(Args)]
pub(crate) struct ReadArgs {
    /// Notiz[#Abschnitt]; Abschnitt als Titel, L<Zeile> oder Nummer
    address: String,
    /// Alle Abschnitte dieser Art: state | open | decisions | lessons | evidence | sources | paths | topics
    #[arg(long)]
    part: Option<String>,
    /// Zeilenbereiche A-B, A, A- oder A:S-B (ab Zeichen S der Zeile A), mehrere mit Komma
    #[arg(long, value_name = "A-B")]
    lines: Option<String>,
    /// Nur die Gliederung
    #[arg(long)]
    outline: bool,
    /// Ein- und ausgehende Links anhängen
    #[arg(long)]
    links: bool,
    /// Tokenbudget der Ausgabe (mindestens 60)
    #[arg(long, default_value_t = READ_BUDGET)]
    budget: u32,
}

#[derive(Args)]
pub(crate) struct ContextArgs {
    /// Projektname (eindeutiger Anfang genügt)
    project: Option<String>,
    /// Arbeitsordner statt Projektname (Standard: aktueller Ordner)
    #[arg(long, value_name = "ORDNER")]
    cwd: Option<String>,
    /// Tokenbudget der Ausgabe (mindestens 200)
    #[arg(long, default_value_t = CONTEXT_BUDGET)]
    budget: u32,
    /// Höchstzahl Journalzeilen (höchstens 50)
    #[arg(long, default_value_t = CONTEXT_JOURNAL)]
    journal: usize,
}

#[derive(Args)]
pub(crate) struct NewArgs {
    /// Titel (H1 und Dateiname); für `-t overview` optional
    pub(crate) title: Option<String>,
    /// Projekt (Pflicht für topic und overview)
    #[arg(short, long)]
    pub(crate) project: Option<String>,
    /// Notizart: topic | overview | source
    #[arg(short = 't', long = "type", default_value = "topic")]
    pub(crate) note_type: String,
    /// status: active | paused | done | superseded
    #[arg(short = 's', long)]
    pub(crate) status: Option<String>,
    /// overview: Projektwurzel auf der Platte (mehrfach möglich)
    #[arg(long = "path", value_name = "ORDNER")]
    pub(crate) paths: Vec<String>,
    /// source: Adresse der Quelle
    #[arg(long)]
    pub(crate) url: Option<String>,
    /// checked_on (Standard: heute)
    #[arg(long, value_name = "JJJJ-MM-TT")]
    pub(crate) date: Option<String>,
    /// topic: Beschreibung für den Link in der Übersicht (ein kurzer Satz; `wb context` zeigt ihn)
    #[arg(long, value_name = "SATZ")]
    pub(crate) summary: Option<String>,
    /// topic: die Übersicht nicht ändern, nur den Link nennen
    #[arg(long)]
    pub(crate) no_link: bool,
}

#[derive(Args)]
pub(crate) struct LogArgs {
    /// Text (ohne Angabe von stdin)
    pub(crate) text: Vec<String>,
    /// Betroffene Notiz: voller Pfad oder voller Name (kein Namensanfang)
    #[arg(short, long)]
    pub(crate) note: Option<String>,
    /// Datum (Standard: heute)
    #[arg(long, value_name = "JJJJ-MM-TT")]
    pub(crate) date: Option<String>,
    /// Uhrzeit mitschreiben (HH:MM, ohne Wert: jetzt)
    #[arg(long, value_name = "HH:MM", num_args = 0..=1, default_missing_value = "now")]
    pub(crate) time: Option<String>,
}

#[derive(Args)]
pub(crate) struct CheckArgs {
    /// Nur Notizen unter diesem Pfadpräfix
    prefix: Option<String>,
    /// Bezugsdatum für „veraltet“ (JJJJ-MM-TT); ohne Angabe keine Altersregel
    #[arg(long)]
    today: Option<String>,
    /// Höchstzahl Befunde in der Ausgabe
    #[arg(long, default_value_t = CHECK_LIMIT)]
    limit: usize,
    /// Befunde überspringen (Fortsetzung)
    #[arg(long, default_value_t = 0)]
    offset: usize,
    /// Exit 1, wenn es Befunde ab Warnung gibt
    #[arg(long)]
    strict: bool,
    /// Auch Hinweise (info) einzeln listen; ohne PREFIX stehen sie sonst je Regel in einer Zeile
    #[arg(long)]
    all: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    Agent,
    Json,
    Human,
}

/// Fehler von clap im Format `error[usage]: …` mit `hint:` (§15.6); Hilfe und Version normal.
type Globals = Vec<(String, Option<String>)>;

fn parse_cli() -> Result<(Cli, Globals), ExitCode> {
    let matches = match Cli::command().try_get_matches() {
        Ok(m) => m,
        Err(e) => return Err(clap_error(e)),
    };
    let cli = match Cli::from_arg_matches(&matches) {
        Ok(c) => c,
        Err(e) => return Err(clap_error(e)),
    };
    // Globale Schalter, die auf der Kommandozeile standen, für `more:`-Zeilen.
    let sub = matches.subcommand().map(|(_, m)| m);
    let from_cli = |id: &str| {
        let src = |m: &ArgMatches| m.try_get_raw(id).ok().flatten().is_some() && m.value_source(id) == Some(ValueSource::CommandLine);
        src(&matches) || sub.is_some_and(src)
    };
    let mut global = Vec::new();
    if from_cli("vault")
        && let Some(v) = &cli.g.vault
    {
        global.push(("--vault".to_string(), Some(v.display().to_string())));
    }
    if from_cli("index")
        && let Some(v) = &cli.g.index
    {
        global.push(("--index".to_string(), Some(v.display().to_string())));
    }
    if from_cli("read_only") && cli.g.read_only {
        global.push(("--read-only".to_string(), None));
    }
    if from_cli("no_sync") && cli.g.no_sync {
        global.push(("--no-sync".to_string(), None));
    }
    Ok((cli, global))
}

fn clap_error(e: clap::Error) -> ExitCode {
    use clap::error::ErrorKind;
    match e.kind() {
        ErrorKind::DisplayHelp | ErrorKind::DisplayVersion => {
            let _ = e.print();
            return ExitCode::from(exit::OK as u8);
        }
        _ => {}
    }
    let text = e.render().to_string();
    let first = text.lines().find(|l| !l.trim().is_empty()).unwrap_or("falscher Aufruf");
    let first = first.trim().strip_prefix("error:").unwrap_or(first).trim();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let sub = args.iter().find(|a| !a.starts_with('-')).cloned();
    let leading_dash = args.iter().any(|a| a.starts_with('-') && a.len() > 1 && !a.starts_with("--"))
        || matches!(e.kind(), ErrorKind::UnknownArgument);
    let hint = if sub.as_deref() == Some("search") && leading_dash && e.kind() == ErrorKind::UnknownArgument {
        "Suchbegriffe mit führendem `-` nach `--` angeben: wb search -- -begriff".to_string()
    } else if let Some(tip) = text.lines().find_map(|l| l.trim().strip_prefix("tip:")) {
        tip.trim().to_string()
    } else {
        match &sub {
            Some(s) if Cli::command().find_subcommand(s).is_some() => format!("wb {s} --help"),
            _ => "wb --help".to_string(),
        }
    };
    let mut se = std::io::stderr().lock();
    let _ = writeln!(se, "error[usage]: {}", fmt::clean(first));
    let _ = writeln!(se, "hint: {}", fmt::clean(&hint));
    ExitCode::from(exit::USAGE as u8)
}

fn main() -> ExitCode {
    let (cli, global) = match parse_cli() {
        Ok(x) => x,
        Err(code) => return code,
    };
    let mode = if cli.g.json {
        Mode::Json
    } else if cli.g.human {
        Mode::Human
    } else {
        Mode::Agent
    };
    let color = mode == Mode::Human && {
        use std::io::IsTerminal;
        std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none_or(|v| v.is_empty())
    };
    let ctx = fmt::Ctx { mode, color, global };
    let mut out = Vec::<u8>::new();
    let code = match run(&cli, &ctx, &mut out) {
        Ok(c) => c,
        Err(e) => {
            let _ = std::io::stdout().write_all(&out);
            report_error(&e);
            return ExitCode::from(e.exit_code() as u8);
        }
    };
    let mut so = std::io::stdout().lock();
    let _ = so.write_all(&out);
    let _ = so.flush();
    ExitCode::from(code as u8)
}

fn report_error(e: &Error) {
    let mut se = std::io::stderr().lock();
    let _ = writeln!(se, "error[{}]: {}", e.code(), fmt::clean(&e.to_string()));
    let hint = e.hint().unwrap_or_else(|| "wb --help".to_string());
    let _ = writeln!(se, "hint: {}", fmt::clean(&hint));
}

struct Config {
    vault: Option<PathBuf>,
    index: Option<PathBuf>,
    read_only: bool,
}

/// `%APPDATA%\wb\config.toml`: nur `schlüssel = "wert"`-Zeilen (vault, index, read_only).
fn user_config() -> Option<Config> {
    let base = std::env::var_os("APPDATA").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    let text = std::fs::read_to_string(base.join("wb").join("config.toml")).ok()?;
    let mut c = Config { vault: None, index: None, read_only: false };
    for line in text.lines() {
        let Some((k, v)) = line.split_once('=') else { continue };
        let v = v.trim().trim_matches('"').trim_matches('\'');
        match k.trim() {
            "vault" => c.vault = Some(PathBuf::from(v)),
            "index" => c.index = Some(PathBuf::from(v)),
            "read_only" => c.read_only = v == "true",
            _ => {}
        }
    }
    Some(c)
}

fn open_vault(g: &Global, create: bool, discard_broken: bool) -> Result<Vault, Error> {
    let cfg = user_config();
    let (root, index, ro) = if let Some(v) = &g.vault {
        (v.clone(), g.index.clone(), g.read_only)
    } else if let Some(c) = cfg.as_ref().filter(|c| c.vault.is_some()) {
        (c.vault.clone().unwrap(), g.index.clone().or(c.index.clone()), g.read_only || c.read_only)
    } else {
        let mut dir = std::env::current_dir().map_err(|e| Error::Io { path: ".".into(), source: e })?;
        loop {
            if dir.join(".wb").is_dir() {
                break (dir, g.index.clone(), g.read_only);
            }
            if !dir.pop() {
                return Err(Error::Vault {
                    message: "kein Vault gefunden".into(),
                    hint: Some("--vault <Ordner> oder WB_VAULT setzen, oder im Vault `wb sync --init` ausführen".into()),
                });
            }
        }
    };
    let mut o = OpenOptions::new(root).read_only(ro).create_index(create).discard_broken(discard_broken);
    if let Some(i) = index {
        o = o.index_dir(i);
    }
    Vault::open(o)
}

#[cfg(feature = "ui")]
fn ui(v: &Vault, ascii: bool) -> Result<i32, Error> {
    use std::io::IsTerminal;
    if !std::io::stdout().is_terminal() {
        return Err(Error::Usage {
            message: "`wb ui` braucht ein Terminal".into(),
            hint: Some("für Agenten und Pipes `wb context`, `wb search` oder `wb check`".into()),
        });
    }
    wb_tui::run_vault_with(v, ascii).map_err(|e| Error::Io { path: v.root().to_path_buf(), source: e })?;
    Ok(exit::OK)
}

/// `wb stats`: Bs statische Übersicht; Farbe nur mit `--human` am Terminal ohne `NO_COLOR`,
/// `--json` liefert die Projektliste. Die Breite klemmt `wb_tui::stats` (§16.4).
fn stats(out: &mut Vec<u8>, mode: Mode, v: &Vault, width: u16, ascii: bool) -> Result<i32, Error> {
    if mode == Mode::Json {
        fmt::json(out, &v.projects()?);
        return Ok(exit::OK);
    }
    stats_text(out, mode, v, width, ascii)
}

#[cfg(feature = "ui")]
fn stats_text(out: &mut Vec<u8>, mode: Mode, v: &Vault, width: u16, ascii: bool) -> Result<i32, Error> {
    use std::io::IsTerminal;
    // Agent-Format bleibt ohne Farbe; `render_for` beachtet zusätzlich `NO_COLOR` (§16.7).
    let tty = mode == Mode::Human && std::io::stdout().is_terminal();
    let mut s = wb_tui::stats::render_for(&wb_tui::core_source::CoreSource::new(v), width, ascii, tty);
    if !s.ends_with('\n') {
        s.push('\n');
    }
    out.extend_from_slice(s.as_bytes());
    Ok(exit::OK)
}

#[cfg(not(feature = "ui"))]
fn stats_text(_: &mut Vec<u8>, _: Mode, _: &Vault, _: u16, _: bool) -> Result<i32, Error> {
    Err(Error::Usage { message: "`wb stats` ist in diesem Build nicht enthalten".into(), hint: Some("mit dem Feature `ui` bauen".into()) })
}

#[cfg(not(feature = "ui"))]
fn ui(_: &Vault, _: bool) -> Result<i32, Error> {
    Err(Error::Usage { message: "`wb ui` ist in diesem Build nicht enthalten".into(), hint: Some("mit dem Feature `ui` bauen".into()) })
}

fn timing(g: &Global, what: &str, t: Instant) {
    if g.timing {
        eprintln!("timing {what} {:.1}ms", t.elapsed().as_secs_f64() * 1000.0);
    }
}

fn run(cli: &Cli, ctx: &fmt::Ctx, out: &mut Vec<u8>) -> Result<i32, Error> {
    let g = &cli.g;
    let mode = ctx.mode;
    if matches!(cli.cmd, Cmd::Ui { .. }) && (g.json || g.human) {
        return Err(Error::Usage {
            message: format!("`wb ui` kennt kein {}", if g.json { "--json" } else { "--human" }),
            hint: Some("`wb ui` ist interaktiv; für JSON `wb stats --json`, Text `wb stats`".into()),
        });
    }
    let t0 = Instant::now();
    let init = matches!(cli.cmd, Cmd::Sync { init: true, .. });
    let full = matches!(cli.cmd, Cmd::Sync { full: true, .. });
    let v = open_vault(g, init, full)?;
    timing(g, "open", t0);
    // Hängt ein unterbrochenes `wb mv`, laufen Lesebefehle weiter und sagen es (§19.1);
    // Schreibbefehle verweigern mit einem Fehler, der Datei und heilenden Befehl nennt.
    if let Some(p) = v.pending_problem()
        && !matches!(cli.cmd, Cmd::New(_) | Cmd::Log(_) | Cmd::Mv { .. })
    {
        eprintln!("note: {p}");
    }
    if let Cmd::Ui { ascii } = cli.cmd {
        if !g.no_sync {
            v.sync()?;
        }
        return ui(&v, ascii);
    }
    if let Cmd::Sync { full, .. } = cli.cmd {
        let t = Instant::now();
        let r = if full { v.rebuild()? } else { v.sync()? };
        timing(g, "sync", t);
        fmt::sync(out, mode, &r);
        for (p, e) in &r.errors {
            eprintln!("warn: {p}: {e}");
        }
        return Ok(exit::OK);
    }
    let writes = matches!(cli.cmd, Cmd::New(_) | Cmd::Log(_) | Cmd::Mv { .. });
    if !g.no_sync && !writes {
        let t = Instant::now();
        let r = v.sync()?;
        timing(g, "autosync", t);
        if g.timing {
            eprintln!("timing autosync-report added={} changed={} removed={} touched={}", r.added, r.changed, r.removed, r.touched);
        }
    }
    let t = Instant::now();
    // Lesebefehle sehen von der ersten Abfrage bis zur fertigen Ausgabe einen Indexstand (§15.1).
    let _view = if writes { None } else { Some(v.read_view()?) };
    let code = match &cli.cmd {
        Cmd::Search(a) => fmt::search(out, ctx, &v, a)?,
        Cmd::Read(a) => fmt::read(out, ctx, &v, a)?,
        Cmd::Context(a) => fmt::context(out, ctx, &v, a)?,
        Cmd::Check(a) => fmt::check(out, ctx, &v, a)?,
        Cmd::Stats { width, ascii } => stats(out, mode, &v, *width, *ascii)?,
        Cmd::New(a) => write::new(out, ctx, &v, a)?,
        Cmd::Log(a) => write::log(out, ctx, &v, a)?,
        Cmd::Mv { from, to } => write::mv(out, ctx, &v, from, to)?,
        Cmd::Sync { .. } | Cmd::Ui { .. } => unreachable!(),
    };
    timing(g, "command", t);
    timing(g, "total", t0);
    Ok(code)
}
