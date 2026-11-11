//! Plain words for the yt-dlp errors people run into most often, each with
//! what to do about it. yt-dlp's own message stays in the activity log; the
//! explanation is what the item's card shows.

use tr::tr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Problem {
    NotABot,
    WrongPassword,
    PasswordNeeded,
    MembersOnly,
    SignIn,
    GeoBlocked,
    NotStarted,
    TooManyRequests,
    Forbidden,
    NotFound,
    ServerError,
    FormatUnavailable,
    Unsupported,
    DiskFull,
    Unavailable,
    Offline,
}

/// Lowercase pieces of yt-dlp's wording. The first rule that matches wins,
/// so the order matters: "Sign in to confirm you're not a bot" is a bot
/// check rather than a sign-in, a members-only video is also "unavailable",
/// and an HTTP code says more than the "Unable to download webpage" around
/// it.
const RULES: &[(Problem, &[&str])] = &[
    (Problem::NotABot, &["not a bot"]),
    (
        Problem::WrongPassword,
        &[
            "wrong password",
            "wrong video password",
            "incorrect password",
        ],
    ),
    (
        Problem::PasswordNeeded,
        &[
            "--video-password",
            "protected by a password",
            "password-protected",
            "password protected",
        ],
    ),
    (
        Problem::MembersOnly,
        &["members-only", "available to this channel's members"],
    ),
    (
        Problem::SignIn,
        &[
            "sign in to confirm your age",
            "age-restricted",
            "inappropriate for some users",
            "private video",
            "this video is private",
            "login required",
            "requires authentication",
        ],
    ),
    (
        Problem::GeoBlocked,
        &[
            "available in your country",
            "not available from your location",
            "geo restriction",
            "geo-restricted",
        ],
    ),
    (
        Problem::NotStarted,
        &["this live event will begin", "premieres in"],
    ),
    (
        Problem::TooManyRequests,
        &["http error 429", "too many requests"],
    ),
    (Problem::Forbidden, &["http error 403", "403: forbidden"]),
    (Problem::NotFound, &["http error 404", "404: not found"]),
    (
        Problem::ServerError,
        &[
            "http error 500",
            "http error 502",
            "http error 503",
            "http error 504",
        ],
    ),
    (
        Problem::FormatUnavailable,
        &["requested format is not available"],
    ),
    (Problem::Unsupported, &["unsupported url"]),
    (
        Problem::DiskFull,
        &["no space left on device", "[errno 28]"],
    ),
    (
        Problem::Unavailable,
        &[
            "video unavailable",
            "this video is unavailable",
            "this video has been removed",
            "this video is no longer available",
            "has been terminated",
        ],
    ),
    (
        Problem::Offline,
        &[
            "getaddrinfo failed",
            "name or service not known",
            "temporary failure in name resolution",
            "nodename nor servname",
            "network is unreachable",
            "no route to host",
            "timed out",
        ],
    ),
];

fn classify(error: &str) -> Option<Problem> {
    let error = error.to_lowercase();
    RULES
        .iter()
        .find(|(_, pieces)| pieces.iter().any(|piece| error.contains(piece)))
        .map(|(problem, _)| *problem)
}

/// What to tell the user about `error`, one of yt-dlp's error lines, or
/// `None` when it isn't a problem this knows. `bundled_tools` is the
/// Flatpak, which can neither read browser cookies nor update yt-dlp, so
/// the advice there is different.
pub fn explain(error: &str, bundled_tools: bool) -> Option<String> {
    let text = match classify(error)? {
        Problem::NotABot if bundled_tools => {
            tr!("YouTube wants to make sure you're not a bot. Wait a while and try again.")
        }
        Problem::NotABot => tr!(
            "YouTube wants to make sure you're not a bot. Use the cookies of a browser where you're signed in to YouTube: Settings, Advanced options, Signing in."
        ),
        Problem::WrongPassword => tr!(
            "The video password isn't right. Check it in Settings, Advanced options, Signing in, and add the link again."
        ),
        Problem::PasswordNeeded => tr!(
            "This video is protected by a password. Type it in Settings, Advanced options, Signing in, and add the link again."
        ),
        Problem::MembersOnly if bundled_tools => tr!(
            "Only the channel's members can watch this video. The Flatpak can't read your browser's cookies to sign in; the portable version can."
        ),
        Problem::MembersOnly => tr!(
            "Only the channel's members can watch this video. Use the cookies of a browser where you're signed in with a member account: Settings, Advanced options, Signing in."
        ),
        Problem::SignIn if bundled_tools => tr!(
            "This video needs you signed in (it's private or age-restricted). The Flatpak can't read your browser's cookies to sign in; the portable version can."
        ),
        Problem::SignIn => tr!(
            "This video needs you signed in (it's private or age-restricted). Use the cookies of a browser where you're signed in: Settings, Advanced options, Signing in."
        ),
        Problem::GeoBlocked => tr!("This video isn't available in your country."),
        Problem::NotStarted => tr!(
            "This stream or premiere hasn't started yet. Try again when it starts, or set a wait for scheduled streams in Settings, Advanced options."
        ),
        Problem::TooManyRequests => tr!(
            "The site got too many requests from you and is refusing more for now. Wait a while (an hour usually does it) and try again."
        ),
        Problem::Forbidden if bundled_tools => tr!(
            "The site refused the download (error 403). A newer yt-dlp usually fixes it, and it comes with the next update of the app."
        ),
        Problem::Forbidden => tr!(
            "The site refused the download (error 403). Update yt-dlp in ☰, Setup and updates, and try again."
        ),
        Problem::NotFound => tr!(
            "Nothing was found at this link (error 404). Check that it's right and that the video still exists."
        ),
        Problem::ServerError => tr!("The site had a problem on its side. Try again later."),
        Problem::FormatUnavailable => tr!(
            "The quality or format you picked isn't available for this video. Pick another one on its card."
        ),
        Problem::Unsupported => tr!("yt-dlp doesn't support this site or kind of link."),
        Problem::DiskFull => tr!("The disk is full. Free some space and try again."),
        Problem::Unavailable => {
            tr!("This video is unavailable: it was removed or made private, or the link is wrong.")
        }
        Problem::Offline => {
            tr!("Couldn't reach the site. Check your internet connection and try again.")
        }
    };
    Some(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// yt-dlp's wording as it reaches the app: the text after "ERROR: ".
    #[test]
    fn common_errors_are_recognised() {
        let cases = [
            (
                "[youtube] dQw4w9WgXcQ: Sign in to confirm you're not a bot. Use --cookies-from-browser or --cookies for the authentication.",
                Problem::NotABot,
            ),
            (
                "[youtube] abc: Join this channel to get access to members-only content like this video, and other exclusive perks.",
                Problem::MembersOnly,
            ),
            (
                "[youtube] abc: This video is available to this channel's members on level: Tier 1",
                Problem::MembersOnly,
            ),
            (
                "[youtube] abc: Sign in to confirm your age. This video may be inappropriate for some users.",
                Problem::SignIn,
            ),
            (
                "[youtube] abc: Private video. Sign in if you've been granted access to this video",
                Problem::SignIn,
            ),
            (
                "[vimeo] 123: This video is protected by a password, use the --video-password option",
                Problem::PasswordNeeded,
            ),
            ("[vimeo] 123: Wrong password", Problem::WrongPassword),
            (
                "[youtube] abc: Video unavailable. The uploader has not made this video available in your country",
                Problem::GeoBlocked,
            ),
            (
                "[BBC] p0abc: This video is not available in your country due to geo restriction",
                Problem::GeoBlocked,
            ),
            (
                "[youtube] abc: This live event will begin in 3 hours.",
                Problem::NotStarted,
            ),
            ("[youtube] abc: Premieres in 2 days", Problem::NotStarted),
            (
                "[youtube] abc: Unable to download webpage: HTTP Error 429: Too Many Requests (caused by <HTTPError 429: Too Many Requests>)",
                Problem::TooManyRequests,
            ),
            (
                "unable to download video data: HTTP Error 403: Forbidden",
                Problem::Forbidden,
            ),
            (
                "[generic] Unable to download webpage: HTTP Error 404: Not Found",
                Problem::NotFound,
            ),
            (
                "[twitter] 1: Unable to download JSON metadata: HTTP Error 503: Service Unavailable",
                Problem::ServerError,
            ),
            (
                "[youtube] abc: Requested format is not available. Use --list-formats for a list of available formats",
                Problem::FormatUnavailable,
            ),
            (
                "Unsupported URL: https://example.com/page",
                Problem::Unsupported,
            ),
            (
                "unable to write data: [Errno 28] No space left on device",
                Problem::DiskFull,
            ),
            (
                "[youtube] abc: Video unavailable. This video has been removed by the uploader",
                Problem::Unavailable,
            ),
            (
                "[youtube] abc: Video unavailable. This video is no longer available because the YouTube account associated with this video has been terminated.",
                Problem::Unavailable,
            ),
            (
                "[youtube] abc: Unable to download API page: <urlopen error [Errno -2] Name or service not known> (caused by TransportError('<urlopen error [Errno -2] Name or service not known>'))",
                Problem::Offline,
            ),
            (
                "[generic] Unable to download webpage: <urlopen error [Errno 11001] getaddrinfo failed>",
                Problem::Offline,
            ),
            (
                "[youtube] abc: Unable to download webpage: The read operation timed out",
                Problem::Offline,
            ),
        ];
        for (error, expected) in cases {
            assert_eq!(classify(error), Some(expected), "{error}");
        }
    }

    /// The match ignores case, since yt-dlp's capitals vary between sites.
    #[test]
    fn case_does_not_matter() {
        assert_eq!(classify("HTTP ERROR 429"), Some(Problem::TooManyRequests));
        assert_eq!(classify("http error 429"), Some(Problem::TooManyRequests));
    }

    #[test]
    fn an_unknown_error_gets_no_explanation() {
        assert_eq!(explain("Postprocessing: Conversion failed!", false), None);
        assert_eq!(explain("", false), None);
    }

    /// The portable copy can point to browser cookies and to updating
    /// yt-dlp; the Flatpak has neither (only the video password), so it
    /// must not send people looking for them.
    #[test]
    fn the_advice_fits_where_the_app_runs() {
        let bot = "Sign in to confirm you're not a bot";
        let portable = explain(bot, false).unwrap();
        let flatpak = explain(bot, true).unwrap();
        assert!(portable.contains("cookies"), "{portable}");
        assert!(!flatpak.contains("cookies"), "{flatpak}");

        let forbidden = "HTTP Error 403: Forbidden";
        assert!(
            explain(forbidden, false)
                .unwrap()
                .contains("Setup and updates")
        );
        assert!(
            !explain(forbidden, true)
                .unwrap()
                .contains("Setup and updates")
        );

        for (problem, pieces) in RULES {
            for piece in *pieces {
                let flatpak = explain(piece, true).unwrap();
                assert!(
                    !flatpak.contains("Use the cookies") && !flatpak.contains("Setup and updates"),
                    "{problem:?} sends the Flatpak to a setting it lacks: {flatpak}"
                );
            }
        }
    }

    /// Every rule can be reached: none is hidden behind an earlier one
    /// that matches the same words.
    #[test]
    fn every_rule_matches_its_own_words() {
        for (problem, pieces) in RULES {
            for piece in *pieces {
                assert_eq!(classify(piece), Some(*problem), "{piece}");
            }
        }
    }
}
