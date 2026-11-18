//! Fehler mit stabilem Code, Hinweis und Exit-Code (Vertrag §5.5).

use std::fmt;
use std::path::PathBuf;

pub type Result<T> = std::result::Result<T, Error>;

/// Exit-Codes von `wb` (in `wb --help` dokumentiert).
pub mod exit {
    pub const OK: i32 = 0;
    /// Nichts gefunden (Suche ohne Treffer, Notiz/Abschnitt/Projekt unbekannt).
    pub const NOT_FOUND: i32 = 1;
    /// Falscher Aufruf (auch mehrdeutige Adresse).
    pub const USAGE: i32 = 2;
    /// Vault- oder Indexfehler, Ein-/Ausgabe.
    pub const VAULT: i32 = 3;
    /// Schreibender Befehl im Nur-Lese-Modus verweigert.
    pub const READ_ONLY: i32 = 4;
}

#[derive(Debug)]
pub enum Error {
    /// Falscher Aufruf oder ungültige Angabe.
    Usage { message: String, hint: Option<String> },
    /// Etwas wurde nicht gefunden (`what` z. B. „Notiz“, `query` die Eingabe).
    NotFound { what: &'static str, query: String, suggestions: Vec<String> },
    /// Adresse passt auf mehrere Ziele.
    Ambiguous { what: &'static str, query: String, candidates: Vec<String> },
    /// Schreibversuch im Nur-Lese-Modus (auch: Index läge im Vault). `hint` ersetzt den Standardhinweis.
    ReadOnly { action: String, hint: Option<String> },
    /// Vault nicht gefunden oder ungültig.
    Vault { message: String, hint: Option<String> },
    /// Index fehlt, ist beschädigt oder liegt an verbotener Stelle.
    Index { message: String, hint: Option<String> },
    Io { path: PathBuf, source: std::io::Error },
    Sqlite(rusqlite::Error),
}

impl Error {
    /// Stabiler Kurzcode für `error[<code>]`.
    pub fn code(&self) -> &'static str {
        match self {
            Error::Usage { .. } => "usage",
            Error::NotFound { .. } => "not-found",
            Error::Ambiguous { .. } => "ambiguous",
            Error::ReadOnly { .. } => "read-only",
            Error::Vault { .. } => "vault",
            Error::Index { .. } => "index",
            Error::Io { .. } => "io",
            Error::Sqlite(_) => "index-db",
        }
    }

    pub fn exit_code(&self) -> i32 {
        match self {
            Error::NotFound { .. } => exit::NOT_FOUND,
            Error::Usage { .. } | Error::Ambiguous { .. } => exit::USAGE,
            Error::ReadOnly { .. } => exit::READ_ONLY,
            _ => exit::VAULT,
        }
    }

    /// Wie man den Fehler behebt (für `hint:`).
    pub fn hint(&self) -> Option<String> {
        match self {
            Error::Usage { hint, .. } | Error::Vault { hint, .. } | Error::Index { hint, .. } => hint.clone(),
            Error::NotFound { suggestions, .. } if !suggestions.is_empty() => {
                Some(format!("meintest du: {}", suggestions.join(" | ")))
            }
            Error::NotFound { what, .. } => Some(format!("{what} mit `wb search <Begriffe>` suchen")),
            Error::Ambiguous { candidates, .. } => Some(format!("genauer angeben: {}", candidates.join(" | "))),
            Error::ReadOnly { hint: Some(h), .. } => Some(h.clone()),
            Error::ReadOnly { .. } => Some("ohne --read-only bzw. WB_READONLY in einem beschreibbaren Vault ausführen".into()),
            Error::Io { source, .. } => Some(
                match source.kind() {
                    std::io::ErrorKind::NotFound => "Pfad prüfen (Datei oder Ordner fehlt); bei einer Notiz erst `wb sync`",
                    std::io::ErrorKind::PermissionDenied => {
                        "Zugriff verweigert: Datei offen in einem anderen Programm oder schreibgeschützt; schließen und erneut ausführen"
                    }
                    std::io::ErrorKind::AlreadyExists => "Ziel gibt es schon; anderen Namen wählen",
                    _ => "erneut ausführen; bleibt der Fehler, Pfad, Rechte und freien Platz prüfen",
                }
                .into(),
            ),
            Error::Sqlite(e) => Some(
                match e.sqlite_error_code() {
                    Some(rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked) => {
                        "ein anderer wb-Prozess schreibt gerade: erneut ausführen"
                    }
                    Some(rusqlite::ErrorCode::DatabaseCorrupt | rusqlite::ErrorCode::NotADatabase) => {
                        "Index beschädigt: `wb sync --full` verwirft und baut ihn neu (der Index ist abgeleitet)"
                    }
                    _ => "erneut ausführen; hilft das nicht, baut `wb sync --full` den Index neu (er ist abgeleitet)",
                }
                .into(),
            ),
        }
    }

    pub(crate) fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Error {
        Error::Io { path: path.into(), source }
    }
    pub(crate) fn usage(message: impl Into<String>) -> Error {
        Error::Usage { message: message.into(), hint: None }
    }
    pub(crate) fn index(message: impl Into<String>, hint: impl Into<String>) -> Error {
        Error::Index { message: message.into(), hint: Some(hint.into()) }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Usage { message, .. } => write!(f, "{message}"),
            Error::NotFound { what, query, .. } => write!(f, "{what} nicht gefunden: {query}"),
            Error::Ambiguous { what, query, candidates } => {
                write!(f, "{what} mehrdeutig: {query} ({} Kandidaten)", candidates.len())
            }
            Error::ReadOnly { action, .. } => write!(f, "Nur-Lese-Modus: {action} verweigert"),
            Error::Vault { message, .. } => write!(f, "{message}"),
            Error::Index { message, .. } => write!(f, "{message}"),
            Error::Io { path, source } => write!(f, "{}: {source}", path.display()),
            Error::Sqlite(e) => write!(f, "Indexdatenbank: {e}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io { source, .. } => Some(source),
            Error::Sqlite(e) => Some(e),
            _ => None,
        }
    }
}

impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        Error::Sqlite(e)
    }
}
