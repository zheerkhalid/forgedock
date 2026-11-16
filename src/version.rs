//! Version numbers: `v1.2.3`, `1.2`, `1.2.3-beta`.

use std::cmp::Ordering;

/// The numeric parts of a version. A leading `v` and any suffix after the
/// numbers (`-beta`, `+build`) are ignored.
pub fn parse(text: &str) -> Vec<u64> {
    let t = text.trim().trim_start_matches(['v', 'V']);
    let mut parts = Vec::new();
    for piece in t.split('.') {
        let digits: String = piece.chars().take_while(|c| c.is_ascii_digit()).collect();
        match digits.parse::<u64>() {
            Ok(n) => parts.push(n),
            Err(_) => break,
        }
        if digits.len() != piece.len() {
            break;
        }
    }
    parts
}

pub fn compare(a: &str, b: &str) -> Ordering {
    let (a, b) = (parse(a), parse(b));
    for i in 0..a.len().max(b.len()) {
        let x = a.get(i).copied().unwrap_or(0);
        let y = b.get(i).copied().unwrap_or(0);
        match x.cmp(&y) {
            Ordering::Equal => {}
            other => return other,
        }
    }
    Ordering::Equal
}

/// True when `latest` is a higher version than `installed`. An unreadable
/// version on either side never counts as an update.
pub fn is_newer(latest: &str, installed: &str) -> bool {
    !parse(latest).is_empty()
        && !parse(installed).is_empty()
        && compare(latest, installed) == Ordering::Greater
}

/// `v1.2.3` -> `1.2.3`.
pub fn display(tag: &str) -> String {
    tag.trim().trim_start_matches(['v', 'V']).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses() {
        assert_eq!(parse("v0.1.2"), vec![0, 1, 2]);
        assert_eq!(parse("1.0"), vec![1, 0]);
        assert_eq!(parse("2.3.4-beta.1"), vec![2, 3, 4]);
        assert_eq!(parse(" V10 "), vec![10]);
        assert!(parse("latest").is_empty());
    }

    #[test]
    fn compares_numerically() {
        assert!(is_newer("v0.1.10", "0.1.9"));
        assert!(is_newer("1.0.1", "1.0.0"));
        assert!(is_newer("2.0", "1.9.9"));
        assert!(!is_newer("1.0.0", "1.0.0"));
        assert!(!is_newer("1.0", "1.0.0"));
        assert!(!is_newer("0.9.0", "1.0.0"));
        assert_eq!(compare("1.0", "1.0.0"), Ordering::Equal);
    }

    #[test]
    fn unreadable_is_never_an_update() {
        assert!(!is_newer("nightly", "1.0.0"));
        assert!(!is_newer("1.0.0", ""));
    }
}
