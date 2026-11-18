//! Screenshots for the README: renders the views of `wb ui` against a vault (read-only) as
//! colour-faithful HTML pages, which `docs/screenshots.ps1` turns into PNG.
//! `cargo run --release -p wb-tui --features core --example readme_shots -- <vault> <index dir> <out dir>`
use wb_core::{OpenOptions, Vault};
use wb_tui::core_source::CoreSource;
use wb_tui::{snapshot, App, Theme};

const W: u16 = 124;
const H: u16 = 30;

fn main() -> std::io::Result<()> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    if a.len() < 3 {
        eprintln!("usage: readme_shots <vault> <index dir> <out dir>");
        std::process::exit(2);
    }
    let vault = Vault::open(OpenOptions::new(&a[0]).index_dir(&a[1]).read_only(true).create_index(true))
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    vault.sync().map_err(|e| std::io::Error::other(e.to_string()))?;
    let src = CoreSource::new(&vault);
    let out = std::path::Path::new(&a[2]);
    std::fs::create_dir_all(out)?;

    let shots: [(&str, &dyn Fn(&mut App)); 4] = [
        ("ui-overview", &|_app: &mut App| {}),
        ("ui-project", &|app: &mut App| app.press("Enter")),
        ("ui-search", &|app: &mut App| app.open_search("backup restore")),
        ("ui-reader", &|app: &mut App| app.open_reader("Projects/Home Server/Backup")),
    ];
    for (name, setup) in shots {
        let mut app = App::new(&src, Theme::color());
        setup(&mut app);
        let buf = snapshot::render_app(&app, W, H);
        let path = out.join(format!("{name}.html"));
        std::fs::write(&path, snapshot::to_html(&buf, name))?;
        println!("{}", path.display());
    }
    Ok(())
}
