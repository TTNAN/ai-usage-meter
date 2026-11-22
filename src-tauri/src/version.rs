//! Semantic-version comparison for the update check.
//!
//! The Releases API hands back a tag like `v0.7.2` and the app knows its own
//! `CARGO_PKG_VERSION`; the only question is whether the release is newer.
//! Pure functions, no I/O — the fetch lives in `updater.rs`.

/// One semantic version: the numeric triple plus any pre-release identifiers.
/// Build metadata (`+…`) is parsed away — it never changes precedence.
#[derive(Debug, PartialEq, Eq)]
pub struct Version {
    major: u64,
    minor: u64,
    patch: u64,
    pre: Vec<PreId>,
}

/// A dot-separated pre-release identifier. Semver's rule is numeric ones
/// always rank below alphanumeric ones, which enum variant order gives us.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
enum PreId {
    Num(u64),
    Str(String),
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.major
            .cmp(&other.major)
            .then(self.minor.cmp(&other.minor))
            .then(self.patch.cmp(&other.patch))
            // A pre-release is older than the release itself: an empty list
            // ranks highest, which a `None > Some` comparison gives.
            .then_with(|| match (self.pre.is_empty(), other.pre.is_empty()) {
                (true, true) => std::cmp::Ordering::Equal,
                (true, false) => std::cmp::Ordering::Greater,
                (false, true) => std::cmp::Ordering::Less,
                (false, false) => self.pre.cmp(&other.pre),
            })
    }
}

/// Parse a version like `v0.7.2`, `0.8.0-beta.1` or `1.2.3+build.4`.
/// Lenient where it cannot hurt: leading zeros and a bare `V` are accepted.
fn parse_version(s: &str) -> Option<Version> {
    let s = s.trim().trim_start_matches(['v', 'V']);
    let (core, pre) = match s.split_once(['-', '+']) {
        Some((core, _)) => {
            // A pre-release must come before build metadata; if the cut landed
            // on `+` there is no pre-release at all.
            match s.split_once('-') {
                Some((core, pre)) => (core, pre.split_once('+').map(|(p, _)| p).unwrap_or(pre)),
                None => (s.split_once('+').map(|(c, _)| c).unwrap_or(s), ""),
            }
        }
        None => (s, ""),
    };
    let mut nums = core.split('.');
    let (major, minor, patch) = match (nums.next(), nums.next(), nums.next(), nums.next()) {
        (Some(m), Some(n), Some(p), None) => (m, n, p),
        _ => return None,
    };
    let pre = if pre.is_empty() {
        Vec::new()
    } else {
        let mut ids = Vec::new();
        for id in pre.split('.') {
            if id.is_empty() {
                return None;
            }
            match id.parse::<u64>() {
                Ok(n) => ids.push(PreId::Num(n)),
                Err(_) => ids.push(PreId::Str(id.to_string())),
            }
        }
        ids
    };
    Some(Version {
        major: major.parse().ok()?,
        minor: minor.parse().ok()?,
        patch: patch.parse().ok()?,
        pre,
    })
}

/// Is `latest` a real upgrade over `current`? Anything either side cannot
/// parse is not an upgrade — a bad tag must never nag the user.
pub fn update_available(current: &str, latest: &str) -> bool {
    match (parse_version(current), parse_version(latest)) {
        (Some(c), Some(l)) => l > c,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_newer_patch_is_an_update() {
        assert!(update_available("0.7.1", "0.7.2"));
        assert!(update_available("v0.7.1", "v0.7.2"));
        assert!(!update_available("0.7.2", "0.7.2"));
        assert!(!update_available("0.7.2", "0.7.1"));
    }

    #[test]
    fn versions_compare_numerically_not_as_strings() {
        // `v0.7.10` sorts before `v0.7.2` as text; as a version it is newer.
        assert!(update_available("0.7.2", "v0.7.10"));
        assert!(!update_available("0.7.10", "v0.7.2"));
        assert!(update_available("0.9.9", "0.10.0"));
        assert!(!update_available("0.10.0", "0.9.9"));
        assert!(update_available("0.9.1", "1.0.0"));
    }

    #[test]
    fn a_release_is_newer_than_its_own_prereleases() {
        assert!(update_available("0.8.0-beta.1", "0.8.0"));
        assert!(!update_available("0.8.0", "0.8.0-beta.1"));
        assert!(!update_available("0.8.0-beta.1", "0.8.0-beta.1"));
        assert!(update_available("0.8.0-beta.1", "0.8.0-beta.2"));
        assert!(update_available("0.8.0-alpha", "0.8.0-beta"));
        // A numeric identifier ranks below an alphanumeric one.
        assert!(update_available("0.8.0-1", "0.8.0-alpha"));
        assert!(!update_available("0.8.0-alpha", "0.8.0-1"));
        // More identifiers, same prefix: the larger set is newer.
        assert!(update_available("0.8.0-beta.1", "0.8.0-beta.1.1"));
    }

    #[test]
    fn build_metadata_never_changes_precedence() {
        assert!(!update_available("0.7.1+build.5", "0.7.1"));
        assert!(update_available("0.7.1", "0.7.1+build.9") == false);
    }

    #[test]
    fn nonsense_is_never_an_update() {
        assert!(!update_available("0.7.1", ""));
        assert!(!update_available("0.7.1", "nonsense"));
        assert!(!update_available("nonsense", "0.8.0"));
        assert!(!update_available("", "0.8.0"));
        assert!(!update_available("0.7", "0.8.0"));
        assert!(!update_available("0.7.1", "0.7.x"));
    }
}
