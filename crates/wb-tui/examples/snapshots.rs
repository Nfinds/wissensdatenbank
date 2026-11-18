//! Schreibt alle Schnappschüsse (Text, ohne Farbe, HTML) nach `Saved/B/snapshots/`.
//! `cargo run -p wb-tui --example snapshots [-- <Zielordner>]`
use wb_tui::{fixture::FixtureSource, snapshot, stats};

fn main() -> std::io::Result<()> {
    let dir = std::env::args().nth(1).unwrap_or_else(|| "Saved/B/snapshots".to_string());
    let src = FixtureSource::new();
    let files = snapshot::write_all(&src, std::path::Path::new(&dir), "fixture-")?;
    for w in [80u16, 120] {
        let p = std::path::Path::new(&dir).join(format!("fixture-stats-{w}.txt"));
        std::fs::write(&p, stats::render(&src, w, false))?;
    }
    println!("{} Dateien nach {dir}", files.len());
    Ok(())
}
