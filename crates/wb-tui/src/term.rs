//! Terminal-Treiber: Schleife, Editor-Start. Alles Darstellerische liegt in `App::render`.

use crate::app::App;
use crate::source::DataSource;
use crate::theme::Theme;
use ratatui::crossterm::event::{self, Event};
use std::io;

/// Startet die interaktive Oberfläche und kehrt beim Beenden zurück.
pub fn run(src: &dyn DataSource, theme: Theme) -> io::Result<()> {
    let mut terminal = ratatui::try_init()?;
    let mut app = App::new(src, theme);
    let result = event_loop(&mut terminal, &mut app, src);
    ratatui::restore();
    result
}

fn event_loop(terminal: &mut ratatui::DefaultTerminal, app: &mut App, src: &dyn DataSource) -> io::Result<()> {
    loop {
        terminal.draw(|f| {
            let area = f.area();
            app.render(area, f.buffer_mut());
        })?;
        match event::read()? {
            Event::Key(k) => app.handle_key(k),
            Event::Resize(_, _) => {}
            _ => {}
        }
        if let Some(note) = app.pending_editor.take() {
            match src.file_path(&note) {
                Some(file) => {
                    ratatui::restore();
                    let status = open_in_editor(&file);
                    *terminal = ratatui::try_init()?;
                    terminal.clear()?;
                    app.refresh();
                    if let Err(e) = status {
                        app.set_message(format!("Editor ließ sich nicht starten: {e}"));
                    }
                }
                None => app.set_message("Für diese Datenquelle gibt es keine Datei zum Öffnen".to_string()),
            }
        }
        if app.quit {
            return Ok(());
        }
    }
}

/// `$VISUAL`, `$EDITOR`, sonst Notepad; wartet auf das Ende.
pub fn open_in_editor(file: &std::path::Path) -> io::Result<()> {
    let cmd = std::env::var("VISUAL")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| std::env::var("EDITOR").ok().filter(|s| !s.trim().is_empty()))
        .unwrap_or_else(|| if cfg!(windows) { "notepad".to_string() } else { "vi".to_string() });
    let path_var = std::env::var_os("PATH");
    let pathext = std::env::var("PATHEXT").ok();
    let status = editor_command(&cmd, file, path_var.as_deref(), pathext.as_deref())?.status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!("Beendet mit {status}")))
    }
}

/// Zerlegt eine Editor-Zeile wie `"C:\Program Files\X\x.exe" --wait`: Leerraum trennt,
/// doppelte (und einfache) Anführungszeichen fassen zusammen, Rückstriche bleiben unverändert
/// (Windows-Pfade).
pub fn split_command_line(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut has = false;
    let mut quote: Option<char> = None;
    for c in s.chars() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => cur.push(c),
            None if c == '"' || c == '\'' => {
                quote = Some(c);
                has = true;
            }
            None if c.is_whitespace() => {
                if has || !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                    has = false;
                }
            }
            None => cur.push(c),
        }
    }
    if has || !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Sucht ein Programm im PATH, unter Windows auch mit den Endungen aus PATHEXT
/// (`.cmd`, `.bat` usw.), weil `Command::new` nur `.exe` ergänzt.
fn resolve_program(prog: &str, path: Option<&std::ffi::OsStr>, pathext: Option<&str>) -> Option<std::path::PathBuf> {
    use std::path::{Path, PathBuf};
    let exts: Vec<String> = if cfg!(windows) {
        let raw = pathext.filter(|s| !s.is_empty()).unwrap_or(".COM;.EXE;.BAT;.CMD");
        raw.split(';').filter(|e| !e.is_empty()).map(|e| e.to_ascii_lowercase()).collect()
    } else {
        Vec::new()
    };
    let try_in = |dir: &Path| -> Option<PathBuf> {
        let base = dir.join(prog);
        if base.is_file() && (!cfg!(windows) || Path::new(prog).extension().is_some()) {
            return Some(base);
        }
        for e in &exts {
            let mut name = std::ffi::OsString::from(base.as_os_str());
            name.push(e);
            let cand = PathBuf::from(name);
            if cand.is_file() {
                return Some(cand);
            }
        }
        None
    };
    let p = Path::new(prog);
    if p.components().count() > 1 || p.is_absolute() {
        return try_in(Path::new(""));
    }
    std::env::split_paths(path?).find_map(|d| try_in(&d))
}

/// Baut den Startbefehl für den Editor. Ist die ganze Zeile selbst eine Datei (Pfad mit
/// Leerzeichen ohne Anführungszeichen), gilt sie als Programm. `.cmd` und `.bat` werden über PATHEXT
/// gefunden und mit ausdrücklicher Endung gestartet.
pub fn editor_command(
    line: &str,
    file: &std::path::Path,
    path: Option<&std::ffi::OsStr>,
    pathext: Option<&str>,
) -> io::Result<std::process::Command> {
    let whole = line.trim();
    let mut parts = if std::path::Path::new(whole).is_file() { vec![whole.to_string()] } else { split_command_line(whole) };
    if parts.is_empty() {
        parts.push(if cfg!(windows) { "notepad".into() } else { "vi".into() });
    }
    let prog = parts.remove(0);
    let resolved = resolve_program(&prog, path, pathext).unwrap_or_else(|| std::path::PathBuf::from(&prog));
    // Rust startet `.cmd` und `.bat` mit ausdrücklicher Endung selbst über cmd.exe, mit richtigem Quoting
    let mut c = std::process::Command::new(&resolved);
    c.args(parts).arg(file);
    Ok(c)
}
