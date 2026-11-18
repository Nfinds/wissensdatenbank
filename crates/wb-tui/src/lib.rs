//! Menschen-Ebene der Wissensbank: ratatui-Ansichten, Headless-Schnappschüsse, statische Übersicht.
//! Entwurf und Begründungen: `docs/TUI.md`.

mod app;
#[cfg(feature = "core")]
pub mod core_source;
pub mod fixture;
pub mod localtime;
pub mod md;
pub mod model;
pub mod snapshot;
pub mod source;
pub mod stats;
pub mod term;
pub mod text;
pub mod theme;
mod ui;
mod views;

pub use app::{App, Level, View};
pub use model::*;
pub use source::DataSource;
pub use term::run;
pub use theme::Theme;

/// Einstieg für `wb ui`: startet die Oberfläche auf einem offenen Vault.
/// Der Aufrufer synchronisiert vorher (`vault.sync()`); die Oberfläche liest nur.
#[cfg(feature = "core")]
pub fn run_vault(vault: &wb_core::Vault) -> std::io::Result<()> {
    run_vault_with(vault, false)
}

/// Wie [`run_vault`]; `ascii` für `wb ui --ascii` (reines ASCII, auch in festen Texten). Farbe folgt `NO_COLOR`.
#[cfg(feature = "core")]
pub fn run_vault_with(vault: &wb_core::Vault, ascii: bool) -> std::io::Result<()> {
    let src = core_source::CoreSource::new(vault);
    term::run(&src, Theme::from_env().with_ascii(ascii))
}
