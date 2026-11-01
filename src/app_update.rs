//! Tells whether a newer MimirDLP has been released.
//!
//! GitHub answers `releases/latest` with a redirect to the latest release's
//! tag (`.../releases/tag/v0.6.0`), so reading where it points is enough: no
//! API, no token and no rate limit. One request a day at most; the answer
//! is kept in `mimirdlp.update` next to the settings, so a launch without
//! internet still knows about a release seen earlier.
//!
//! Nothing is downloaded or installed here. The Flatpak never asks: Flathub
//! updates it.

use std::fs;
use std::time::Duration;

use crate::provision::Paths;

/// Where the notice sends the user, and what is asked for the version.
pub const RELEASES: &str = "https://github.com/indep-arg/MimirDLP/releases/latest";

const STATE_NAME: &str = "mimirdlp.update";

/// The version of this build.
pub const CURRENT: &str = env!("CARGO_PKG_VERSION");

/// The newer release's version, if there is one. Asks GitHub at most once
/// a day; any failure counts as "nothing newer".
pub fn check(paths: &Paths) -> Option<String> {
    check_with(paths, crate::provision::today(), fetch_latest)
}

/// [`check`] with the day and the request given, for the tests.
fn check_with(
    paths: &Paths,
    today: i64,
    fetch: impl FnOnce() -> Result<String, String>,
) -> Option<String> {
    let state = paths.app_dir.join(STATE_NAME);
    let saved = fs::read_to_string(&state).unwrap_or_default();
    let (checked, known) = parse_state(&saved);
    let latest = if checked == Some(today) {
        known
    } else {
        match fetch() {
            Ok(latest) => {
                // A folder that can't be written to only means asking again
                // next time.
                let _ = fs::write(&state, format!("checked={today}\nlatest={latest}\n"));
                Some(latest)
            }
            Err(_) => known,
        }
    };
    latest.filter(|latest| newer(latest, CURRENT))
}

fn parse_state(text: &str) -> (Option<i64>, Option<String>) {
    let value = |key: &str| {
        text.lines()
            .find_map(|line| line.strip_prefix(key)?.strip_prefix('='))
            .map(str::trim)
            .filter(|v| !v.is_empty())
    };
    (
        value("checked").and_then(|day| day.parse().ok()),
        value("latest").map(str::to_string),
    )
}

/// Asks GitHub where `releases/latest` points, without following it.
fn fetch_latest() -> Result<String, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(15)))
        .max_redirects(0)
        .user_agent(concat!(
            env!("CARGO_PKG_NAME"),
            "/",
            env!("CARGO_PKG_VERSION")
        ))
        .build()
        .into();
    let response = agent.head(RELEASES).call().map_err(|e| e.to_string())?;
    let location = response
        .headers()
        .get("location")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| "no redirect to a release".to_string())?;
    version_from_location(location).ok_or_else(|| format!("not a release tag: {location}"))
}

/// `0.6.0` from `https://github.com/.../releases/tag/v0.6.0`.
fn version_from_location(location: &str) -> Option<String> {
    let tag = location.rsplit_once("/releases/tag/")?.1;
    let version = tag.strip_prefix('v').unwrap_or(tag);
    parse(version).map(|_| version.to_string())
}

/// `MAJOR.MINOR.PATCH`, the form the release workflow's tags have. Anything
/// else (a pre-release, a stray tag) is not a version to offer.
fn parse(version: &str) -> Option<(u64, u64, u64)> {
    let mut parts = version.split('.');
    let mut next = || parts.next()?.parse::<u64>().ok();
    let parsed = (next()?, next()?, next()?);
    parts.next().is_none().then_some(parsed)
}

/// Whether `candidate` is a later version than `current`.
pub fn newer(candidate: &str, current: &str) -> bool {
    match (parse(candidate), parse(current)) {
        (Some(candidate), Some(current)) => candidate > current,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::tempdir;

    #[test]
    fn versions_compare_by_number_not_by_text() {
        assert!(newer("0.6.0", "0.5.0"));
        assert!(newer("0.10.0", "0.9.3"));
        assert!(newer("1.0.0", "0.99.99"));
        assert!(!newer("0.5.0", "0.5.0"));
        assert!(!newer("0.4.1", "0.5.0"));
        assert!(!newer("0.6.0-beta", "0.5.0"), "pre-releases aren't offered");
        assert!(!newer("garbage", "0.5.0"));
    }

    /// What GitHub answered for this repository on 2026-11-01.
    #[test]
    fn the_version_is_read_from_the_redirect() {
        assert_eq!(
            version_from_location("https://github.com/indep-arg/MimirDLP/releases/tag/v0.5.0")
                .as_deref(),
            Some("0.5.0")
        );
        assert_eq!(
            version_from_location("/indep-arg/MimirDLP/releases/tag/0.6.1").as_deref(),
            Some("0.6.1")
        );
        // No release yet: GitHub sends the releases list instead.
        assert_eq!(
            version_from_location("https://github.com/indep-arg/MimirDLP/releases"),
            None
        );
        assert_eq!(version_from_location(".../releases/tag/nightly"), None);
    }

    /// GitHub is asked once a day; the rest of that day, and on a day
    /// without internet, the saved answer is used.
    #[test]
    fn github_is_asked_at_most_once_a_day() {
        let dir = tempdir();
        let paths = Paths::new(dir.clone());
        let asked = std::cell::Cell::new(0);
        let fetch = |answer: &'static str| {
            let asked = &asked;
            move || {
                asked.set(asked.get() + 1);
                Ok(answer.to_string())
            }
        };

        assert_eq!(
            check_with(&paths, 100, fetch("99.0.0")).as_deref(),
            Some("99.0.0")
        );
        assert_eq!(
            check_with(&paths, 100, fetch("98.0.0")).as_deref(),
            Some("99.0.0")
        );
        assert_eq!(asked.get(), 1, "the same day doesn't ask again");

        let offline = || Err::<String, String>("no network".into());
        assert_eq!(check_with(&paths, 101, offline).as_deref(), Some("99.0.0"));

        assert_eq!(check_with(&paths, 102, fetch(CURRENT)), None);
        assert_eq!(asked.get(), 2);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn nothing_is_offered_when_this_is_the_latest_or_nothing_is_known() {
        let dir = tempdir();
        let paths = Paths::new(dir.clone());
        assert_eq!(check_with(&paths, 5, || Err("offline".into())), None);
        assert_eq!(check_with(&paths, 6, || Ok("0.0.1".into())), None);
        fs::remove_dir_all(dir).unwrap();
    }

    /// Not part of the normal run: asks GitHub for real.
    #[test]
    #[ignore = "hits the network"]
    fn github_answers_with_a_release_tag() {
        let latest = fetch_latest().unwrap();
        assert!(parse(&latest).is_some(), "{latest}");
    }
}
