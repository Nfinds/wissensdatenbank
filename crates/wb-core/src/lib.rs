//! # wb-core
//!
//! Kern der Wissensbank: Notizmodell, Frontmatter- und Link-Parser, Abschnitte, Index
//! (SQLite FTS5), inkrementeller Sync, Passagensuche, Graph, Prüfregeln, Journal.
//!
//! Markdown-Dateien sind die Quelle der Wahrheit; der Index unter `<vault>/.wb/` (oder
//! `--index`) ist abgeleitet und darf jederzeit gelöscht werden.
//!
//! Einstieg: [`Vault::open`] mit [`OpenOptions`], dann [`Vault::sync`] und die Abfragen.
//! Reine Funktionen ohne Index (für Migration und Fixtures): [`note::parse_note`],
//! [`frontmatter`], [`links`], [`markdown`], [`journal`], [`fsutil::write_atomic`].

pub mod error;
pub mod frontmatter;
pub mod fsutil;
pub mod journal;
pub mod links;
pub mod markdown;
pub mod model;
pub mod note;
pub mod secrets;
pub mod text;
mod vault;

mod check;
mod index;
mod query;
mod search;
mod sync;
mod write;

pub use write::{is_bidi_control, local_time, local_today};

pub use error::{Error, Result, exit};
pub use frontmatter::{FmEntry, FmProblem, FmValue, Frontmatter, FrontmatterState};
pub use model::*;
/// UTC-Datum `JJJJ-MM-TT` zu Sekunden seit 1970.
pub use query::date_of_mtime as date_of_secs;
pub use note::{ParsedNote, parse_note};
pub use vault::{OpenOptions, ReadView, Vault, split_address};

/// Version des Kerns (für `wb --version` und Indexmetadaten).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
