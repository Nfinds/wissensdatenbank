//! Startet die Oberfläche mit den synthetischen Fixture-Daten.
//! `cargo run -p wb-tui --example demo [-- --no-color --ascii]`
use wb_tui::{fixture::FixtureSource, Theme};

fn main() -> std::io::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut theme = if args.iter().any(|a| a == "--no-color") { Theme::mono() } else { Theme::from_env() };
    if args.iter().any(|a| a == "--ascii") {
        theme = theme.with_ascii(true);
    }
    let src = FixtureSource::new();
    wb_tui::run(&src, theme)
}
