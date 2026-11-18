//! Dateisystemhilfen: atomares Schreiben, Pfadprüfungen.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Liest eine Datei, die ein anderes Programm gerade nicht atomar neu schreiben könnte (erst
/// leeren, dann füllen). Passt der erste Inhalt (`accept_now`), gilt er sofort. Sonst wird alle
/// 5 ms neu gelesen, bis zwei Lesevorgänge denselben nicht leeren Inhalt liefern (höchstens
/// 500 ms). Leer gilt die Datei erst, wenn sie 300 ms lang nur leer war; sonst gewinnt der
/// letzte nicht leere Stand.
pub fn read_settled(file: &Path, accept_now: impl Fn(&[u8]) -> bool) -> std::io::Result<Vec<u8>> {
    let mut bytes = fs::read(file)?;
    if accept_now(&bytes) {
        return Ok(bytes);
    }
    let mut last_full: Option<Vec<u8>> = (!bytes.is_empty()).then(|| bytes.clone());
    for i in 0..100 {
        std::thread::sleep(std::time::Duration::from_millis(5));
        let again = fs::read(file)?;
        if !again.is_empty() {
            if again == bytes {
                return Ok(again);
            }
            last_full = Some(again.clone());
        } else if last_full.is_none() && i >= 60 {
            return Ok(again);
        }
        bytes = again;
    }
    Ok(last_full.unwrap_or(bytes))
}

/// Schreibt atomar: temporäre Datei im selben Ordner, `fsync`, dann Umbenennen über das Ziel.
/// Unter Windows wird bei kurz gesperrtem Ziel (Virenscanner, Editor) bis zu 2 s wiederholt.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let dir = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    fs::create_dir_all(dir)?;
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0);
    let tmp = dir.join(format!(".{name}.wbtmp-{}-{nonce}", std::process::id()));
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    let mut last = None;
    for attempt in 0..20 {
        match fs::rename(&tmp, path) {
            Ok(()) => return Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                last = Some(e);
                std::thread::sleep(std::time::Duration::from_millis(50 + attempt * 5));
            }
            Err(e) => {
                let _ = fs::remove_file(&tmp);
                return Err(e);
            }
        }
    }
    let _ = fs::remove_file(&tmp);
    Err(last.unwrap_or_else(|| std::io::Error::other("rename fehlgeschlagen")))
}

/// Pfad für Vergleiche: absolut (ohne Dateisystemzugriff, wenn möglich), `/`, Kleinbuchstaben,
/// ohne `\\?\`-Präfix und ohne abschließenden `/`.
pub fn compare_key(p: &Path) -> String {
    let abs: PathBuf = if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir().map(|c| c.join(p)).unwrap_or_else(|_| p.to_path_buf())
    };
    let s = abs.to_string_lossy().replace('\\', "/");
    let s = s.strip_prefix("//?/").unwrap_or(&s).to_string();
    // . und .. auflösen
    let mut parts: Vec<&str> = Vec::new();
    for seg in s.split('/') {
        match seg {
            "." | "" if !parts.is_empty() => {}
            ".." => {
                parts.pop();
            }
            _ => parts.push(seg),
        }
    }
    let joined = parts.join("/");
    crate::text::nfc(joined.trim_end_matches('/')).to_lowercase()
}

/// Liegt `inner` innerhalb von `outer` (oder ist gleich)? Vergleich wie unter Windows ohne Groß/Klein.
/// Verknüpfungen (Symlinks, Junctions) werden aufgelöst, auch wenn `inner` noch nicht existiert:
/// dann über den nächsten vorhandenen Vorfahr (Vertrag §15.2). Derselbe Ordner unter einer anderen
/// Adresse (UNC-Freigabe `\\localhost\G$\…`, `\\127.0.0.1\…`, `\\?\UNC\…`, zweiter Laufwerksbuchstabe)
/// zählt ebenfalls: dafür wird die Dateikennung (Datenträger und Dateinummer) jedes vorhandenen
/// Vorfahren von `inner` mit der von `outer` verglichen (Vertrag §20.1).
pub fn is_within(inner: &Path, outer: &Path) -> bool {
    let (ri, ro) = (resolve_existing(inner), resolve_existing(outer));
    let (i, o) = (compare_key(&ri), compare_key(&ro));
    if i == o || i.starts_with(&format!("{o}/")) {
        return true;
    }
    let Ok(target) = same_file::Handle::from_path(&ro) else { return false };
    std::iter::successors(Some(ri.as_path()), |p| p.parent()).any(|p| same_file::Handle::from_path(p).is_ok_and(|h| h == target))
}

/// Pfad mit aufgelösten Verknüpfungen: der längste vorhandene Anfang wird kanonisiert, der Rest
/// (noch nicht vorhanden, also keine Verknüpfung) angehängt; `..` im Rest löst `compare_key` auf.
pub fn resolve_existing(p: &Path) -> std::path::PathBuf {
    let abs = std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf());
    let mut head = abs.clone();
    let mut rest: Vec<std::ffi::OsString> = Vec::new();
    loop {
        if let Ok(c) = fs::canonicalize(&head) {
            let mut out = c;
            for r in rest.iter().rev() {
                out.push(r);
            }
            return out;
        }
        match (head.file_name().map(|n| n.to_os_string()), head.parent().map(|x| x.to_path_buf())) {
            (Some(name), Some(parent)) => {
                rest.push(name);
                head = parent;
            }
            _ => {
                // `..` oder Wurzel: Komponente übernehmen und weiter nach oben
                let comps: Vec<_> = head.components().collect();
                if comps.len() <= 1 {
                    return abs;
                }
                rest.push(comps[comps.len() - 1].as_os_str().to_os_string());
                head = comps[..comps.len() - 1].iter().collect();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn within() {
        assert!(is_within(Path::new("C:/A/B/.wb"), Path::new("c:\\a\\b")));
        assert!(!is_within(Path::new("C:/A/BC"), Path::new("C:/A/B")));
        assert!(is_within(Path::new("C:/A/B/../B/x"), Path::new("C:/A/B")));
    }

    /// Derselbe Ordner über eine UNC-Freigabe (§20.1). Überspringt sich, wenn die
    /// Verwaltungsfreigabe des Laufwerks (`\\localhost\C$`) fehlt.
    #[cfg(windows)]
    #[test]
    fn within_over_unc_share() {
        let d = tempfile::tempdir().unwrap();
        let local = fs::canonicalize(d.path()).unwrap();
        fs::create_dir_all(local.join("vault/sub")).unwrap();
        fs::create_dir_all(local.join("daneben")).unwrap();
        let plain = local.to_string_lossy().trim_start_matches(r"\\?\").to_string();
        let (drive, rest) = plain.split_once(':').unwrap();
        let unc = |host: &str, tail: &str| std::path::PathBuf::from(format!(r"\\{host}\{drive}${rest}\{tail}"));
        if fs::metadata(unc("localhost", "vault")).is_err() {
            eprintln!("übersprungen: keine Freigabe {}", unc("localhost", "vault").display());
            return;
        }
        let vault = local.join("vault");
        assert!(is_within(&unc("localhost", "vault/.wb"), &vault), "noch nicht vorhandener Index über UNC");
        assert!(is_within(&unc("127.0.0.1", "vault/sub"), &vault));
        assert!(is_within(&std::path::PathBuf::from(format!(r"\\?\UNC\localhost\{drive}${rest}\vault\sub\x.md")), &vault));
        assert!(is_within(&vault.join("sub"), &unc("localhost", "vault")), "umgekehrt: äußerer Ordner über UNC");
        assert!(is_within(&unc("localhost", "vault"), &vault), "gleich");
        assert!(!is_within(&unc("localhost", "daneben"), &vault));
        assert!(!is_within(&unc("localhost", ""), &vault), "Vorfahr liegt nicht innerhalb");
    }

    #[test]
    fn atomic_write_replaces() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("ä ö.md");
        write_atomic(&p, b"eins").unwrap();
        write_atomic(&p, b"zwei").unwrap();
        assert_eq!(fs::read(&p).unwrap(), b"zwei");
        assert_eq!(fs::read_dir(d.path()).unwrap().count(), 1);
    }
}
