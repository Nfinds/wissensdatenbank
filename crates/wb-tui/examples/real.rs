//! Echter Vault, nur lesend: `cargo run -p wb-tui --features core --example real -- <vault> <indexordner> [snapshots <zielordner>] [stats]`
//! Ohne Zusatz startet die interaktive Oberfläche.
use wb_core::{OpenOptions, Vault};
use wb_tui::core_source::CoreSource;

fn main() -> std::io::Result<()> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    if a.len() < 2 {
        eprintln!("Aufruf: real <vault> <indexordner> [snapshots <ordner> | stats]");
        std::process::exit(2);
    }
    let vault = Vault::open(OpenOptions::new(&a[0]).index_dir(&a[1]).read_only(true).create_index(true))
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    let rep = vault.sync().map_err(|e| std::io::Error::other(e.to_string()))?;
    eprintln!("sync: {} gelesen, {} neu, {} geändert, {} ms", rep.scanned, rep.added, rep.changed, rep.duration_ms);
    let src = CoreSource::new(&vault);
    match a.get(2).map(String::as_str) {
        Some("snapshots") => {
            let dir = a.get(3).cloned().unwrap_or_else(|| "Saved/B/snapshots-real".into());
            let t = std::time::Instant::now();
            let files = wb_tui::snapshot::write_all(&src, std::path::Path::new(&dir), "real-")?;
            println!("{} Dateien nach {dir} in {:?}", files.len(), t.elapsed());
        }
        Some("keys") => {
            // Zeit je Taste samt Neuzeichnen (§16.6: keine Taste außer `r` über 100 ms)
            let mut app = wb_tui::App::new(&src, wb_tui::Theme::mono());
            let _ = wb_tui::snapshot::render_app(&app, 160, 45);
            let seq = "j j Enter j Enter Esc Esc 3 s t r o m Backspace Down Down Enter Esc Esc F1 g Right Down Esc F1 F2 F4 d p F3 / F1 r";
            let mut worst = std::time::Duration::ZERO;
            for k in seq.split_whitespace() {
                let t = std::time::Instant::now();
                app.press(k);
                let _ = wb_tui::snapshot::render_app(&app, 160, 45);
                let d = t.elapsed();
                if k != "r" {
                    worst = worst.max(d);
                }
                println!("{k:>9}  {:>7.1} ms  {:?}", d.as_secs_f64() * 1000.0, app.view());
            }
            println!("langsamste Taste außer r: {:.1} ms", worst.as_secs_f64() * 1000.0);
        }
        Some("nextproject") => {
            // §18.9: Taste „nächstes Projekt“ (Tab in der Projektkarte) zehnmal, jeweils samt Neuzeichnen bei 160×45
            let mut app = wb_tui::App::new(&src, wb_tui::Theme::mono());
            let _ = wb_tui::snapshot::render_app(&app, 160, 45);
            app.press("Enter");
            let _ = wb_tui::snapshot::render_app(&app, 160, 45);
            let mut ms: Vec<f64> = Vec::new();
            for i in 0..10 {
                let t = std::time::Instant::now();
                app.press("Tab");
                let _ = wb_tui::snapshot::render_app(&app, 160, 45);
                let d = t.elapsed().as_secs_f64() * 1000.0;
                println!("{:>2}  {d:>7.1} ms  {:?}", i + 1, app.view());
                ms.push(d);
            }
            ms.sort_by(|x, y| x.partial_cmp(y).unwrap());
            println!("Median {:.1} ms, Maximum {:.1} ms, Minimum {:.1} ms", (ms[4] + ms[5]) / 2.0, ms[9], ms[0]);
        }
        Some("stats") => {
            let w = std::env::var("WB_WIDTH").ok().and_then(|v| v.parse().ok()).unwrap_or(100);
            print!("{}", wb_tui::stats::render(&src, w, false))
        }
        _ => wb_tui::run(&src, wb_tui::Theme::from_env())?,
    }
    if let Some(e) = src.last_error() {
        eprintln!("letzter Fehler des Kerns: {e}");
    }
    Ok(())
}
