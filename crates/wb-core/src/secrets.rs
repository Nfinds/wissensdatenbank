//! Verdacht auf Geheimnisse (Vertrag §13.9): Zeile und Klasse, nie der Wert.
//!
//! Bewusst eng gefasst, damit die Warnung ernst genommen wird: feste Schlüsselpräfixe, private
//! Schlüssel, JWT und Zuweisungen an Passwort-/Schlüsselnamen mit einem echten Wert. Nicht
//! gemeldet werden lange Hex-Ketten (im alten Wiki 99 von 111 Musterstellen, alles Prüfsummen und
//! Commits) und E-Mail-Adressen (persönliche Daten, aber kein Geheimnis).

/// Klassen in fester Reihenfolge (für Doku und Tests).
pub const CLASSES: [&str; 4] = ["private-key", "api-key", "jwt", "password-assign"];

/// Alle Verdachtsstellen eines Textes: (1-basierte Zeile, Klasse). Höchstens eine je Zeile.
pub fn scan(text: &str) -> Vec<(u32, &'static str)> {
    let mut out = Vec::new();
    for (i, line) in text.split('\n').enumerate() {
        if let Some(c) = classify_line(line) {
            out.push((i as u32 + 1, c));
        }
    }
    out
}

/// Klasse einer Zeile oder `None`.
pub fn classify_line(line: &str) -> Option<&'static str> {
    if line.contains("-----BEGIN ") && line.contains("PRIVATE KEY") {
        return Some("private-key");
    }
    if has_api_key(line) {
        return Some("api-key");
    }
    if has_jwt(line) {
        return Some("jwt");
    }
    if has_password_assign(line) {
        return Some("password-assign");
    }
    None
}

fn is_key_char(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || c == b'-'
}

/// Länge der Folge von Schlüsselzeichen ab `i`.
fn run_len(b: &[u8], i: usize, ok: impl Fn(u8) -> bool) -> usize {
    b[i.min(b.len())..].iter().take_while(|&&c| ok(c)).count()
}

/// Wortanfang: davor kein Schlüsselzeichen.
fn at_word_start(b: &[u8], i: usize) -> bool {
    i == 0 || !is_key_char(b[i - 1])
}

fn has_api_key(line: &str) -> bool {
    let b = line.as_bytes();
    // (Präfix, Mindestlänge des Rests, Zeichenklasse des Rests)
    type Rule = (&'static str, usize, fn(u8) -> bool);
    let rules: [Rule; 11] = [
        ("sk-ant-", 20, is_key_char),
        ("sk-proj-", 20, is_key_char),
        ("sk-", 32, |c| c.is_ascii_alphanumeric()),
        ("ghp_", 36, |c| c.is_ascii_alphanumeric()),
        ("gho_", 36, |c| c.is_ascii_alphanumeric()),
        ("ghs_", 36, |c| c.is_ascii_alphanumeric()),
        ("github_pat_", 40, |c| c.is_ascii_alphanumeric() || c == b'_'),
        ("glpat-", 20, is_key_char),
        ("xoxb-", 20, is_key_char),
        ("AKIA", 16, |c| c.is_ascii_uppercase() || c.is_ascii_digit()),
        ("AIza", 35, is_key_char),
    ];
    for (prefix, min, ok) in rules {
        let mut from = 0;
        while let Some(pos) = line[from..].find(prefix) {
            let i = from + pos;
            let rest = i + prefix.len();
            let n = run_len(b, rest, ok);
            // AKIA: genau 16 Zeichen, sonst ist es ein gewöhnliches Wort in Großbuchstaben.
            let len_ok = if prefix == "AKIA" { n == 16 } else { n >= min };
            let rest_mixed = mixed(&b[rest..rest + n]);
            if at_word_start(b, i) && len_ok && rest_mixed {
                return true;
            }
            from = rest;
        }
    }
    false
}

/// Echter Schlüssel mischt Buchstaben und Ziffern; `sk-xxxxxxxx…` oder reine Wörter nicht.
fn mixed(s: &[u8]) -> bool {
    s.iter().any(u8::is_ascii_digit) && s.iter().any(u8::is_ascii_alphabetic)
}

fn has_jwt(line: &str) -> bool {
    let b = line.as_bytes();
    let mut from = 0;
    while let Some(pos) = line[from..].find("eyJ") {
        let i = from + pos;
        if at_word_start(b, i) {
            let seg = |s: usize| run_len(b, s, |c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-');
            let a = seg(i);
            if a >= 12 && b.get(i + a) == Some(&b'.') {
                let c = seg(i + a + 1);
                if c >= 12 && b.get(i + a + 1 + c) == Some(&b'.') && seg(i + a + c + 2) >= 12 {
                    return true;
                }
            }
        }
        from = i + 3;
    }
    false
}

const SECRET_NAMES: [&str; 12] = [
    "password",
    "passwort",
    "passwd",
    "pwd",
    "secret",
    "client_secret",
    "api_key",
    "apikey",
    "api-key",
    "access_token",
    "auth_token",
    "token",
];

fn has_password_assign(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    let b = lower.as_bytes();
    for name in SECRET_NAMES {
        let mut from = 0;
        while let Some(pos) = lower[from..].find(name) {
            let i = from + pos;
            from = i + name.len();
            // Name als ganzes Wort (`MY_API_KEY` zählt, `tokenizer` nicht).
            let end = i + name.len();
            let before_ok = i == 0 || !b[i - 1].is_ascii_alphanumeric();
            let after_ok = end >= b.len() || !b[end].is_ascii_alphanumeric();
            if !before_ok || !after_ok {
                continue;
            }
            let mut j = end;
            while j < b.len() && matches!(b[j], b'"' | b'\'' | b' ' | b'\t' | b'`') {
                j += 1;
            }
            if j >= b.len() || !matches!(b[j], b'=' | b':') {
                continue;
            }
            j += 1;
            if b.get(j) == Some(&b'=') {
                continue; // `==` ist ein Vergleich
            }
            while j < b.len() && matches!(b[j], b' ' | b'\t' | b'"' | b'\'' | b'`') {
                j += 1;
            }
            let v = &line[j..];
            let value: String = v.chars().take_while(|c| !c.is_whitespace() && !matches!(c, '"' | '\'' | '`' | ',' | ';' | ')')).collect();
            if is_real_value(&value) {
                return true;
            }
        }
    }
    false
}

/// Ein Wert, der wie ein echtes Geheimnis aussieht: mindestens 8 Zeichen, kein Platzhalter, keine
/// Umgebungsvariable, kein Prosa-Wort, und Ziffern oder Sonderzeichen neben Buchstaben.
fn is_real_value(v: &str) -> bool {
    if v.chars().count() < 8 {
        return false;
    }
    let l = v.to_ascii_lowercase();
    let placeholder = v.starts_with('<')
        || v.starts_with('$')
        || v.starts_with('%')
        || v.starts_with('{')
        || v.starts_with('[')
        || v.contains("***")
        || v.contains('…')
        || v.contains("...")
        || l.starts_with("env:")
        || l.starts_with("http")
        || l.contains("example")
        || l.contains("beispiel")
        || l.contains("placeholder")
        || l.contains("changeme")
        || l.contains("your")
        || l.contains("dein")
        || l.contains("xxxx")
        || l == "required"
        || l == "optional"
        || l == "none"
        || l == "null"
        || l == "true"
        || l == "false";
    if placeholder {
        return false;
    }
    let letters = v.chars().any(|c| c.is_alphabetic());
    let other = v.chars().any(|c| c.is_ascii_digit() || (c.is_ascii_punctuation() && c != '.' && c != '-'));
    letters && other
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_classes_without_values() {
        let t = [
            "harmlos",
            "-----BEGIN OPENSSH PRIVATE KEY-----",
            &format!("key = {}{}", "sk-ant-api03-", "a1B2c3D4e5F6g7H8i9J0k1L2"),
            &format!("export GH={}{}", "ghp_", "a1B2c3D4e5F6g7H8i9J0k1L2m3N4o5P6q7R8"),
            &format!("{}{}", "AKIA", "ABCD1234EFGH5678"),
            &format!("Bearer {}.{}.{}", "eyJhbGciOiJIUzI1NiJ9", "eyJzdWIiOiIxMjM0NTY3ODkwIn0", "abcdefghijkl0123"),
            "password: Hunter2!xyz",
            "PASSWORT=geheim123",
        ]
        .join("\n");
        let got = scan(&t);
        let classes: Vec<_> = got.iter().map(|(l, c)| format!("{l}:{c}")).collect();
        assert_eq!(classes, ["2:private-key", "3:api-key", "4:api-key", "5:api-key", "6:jwt", "7:password-assign", "8:password-assign"]);
    }

    #[test]
    fn ignores_common_noise() {
        for l in [
            "SHA-256 3f786850e387550fdab836ed7e6dc881de23001b3f786850e387550fdab836ed",
            "Commit 853780a7c1e5d2b0f9a8c7e6d5b4a3f2e1d0c9b8",
            "kontakt@example.org",
            "password: <dein Passwort>",
            "token = $GITHUB_TOKEN",
            "api_key: env:OPENAI_API_KEY",
            "Das Token muss gesetzt sein: ja",
            "password: required",
            "tokenizer: unicode61",
            "if token == expected",
            "AKIAKOLLEKTIVE GROSSBUCHSTABEN",
            "sk-learn ist eine Bibliothek",
            "secret: ***",
        ] {
            assert_eq!(classify_line(l), None, "{l}");
        }
    }
}
