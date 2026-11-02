# Changelog

All notable changes to MimirDLP are listed here, newest first. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the version numbers follow [Semantic Versioning](https://semver.org/). Dates match the ones in the Linux package's metainfo.

## [Unreleased]

## [0.8.0] - 2026-11-01

### Added

- A setting to draw the window with the processor instead of the graphics card, for when the card gives you trouble (glitches, a remote desktop, a virtual machine). Turn off "Draw with the graphics card" in Settings > Advanced options > Network and other. It takes effect on the next launch and uses about 11 MB of memory. The graphics card stays the default.

### Changed

- Much lighter on Windows: the window is now drawn with DirectX 12 instead of Vulkan. On the PC we tested, memory in Task Manager went from about 160 MB to about 60 MB.
- Opens faster on Windows: in about half a second instead of one and a half.

### Fixed

- "Show in folder" works when the path has a space in it. On Windows it used to open Documents instead of the folder with your file selected.
- On Windows the window no longer lingers for a moment after you close it.
- A settings file saved by some Windows editors (with a byte order mark) lost its first setting. It is read correctly now.

## [0.7.0] - 2026-11-01

### Changed

- Redesigned interface: icons throughout, the link field moved up into the header bar, and every screen shares the same width, spacing and text sizes, in both the light and dark themes.
- Clearer download cards: a larger thumbnail, a bold title, and the status at a glance with an icon and color (queued, downloading, recording live, completed or failed).
- The overall progress bar now counts the video in progress too, so it no longer sits at 0% halfway through a download.
- Each card shows only the actions that apply right now: open the folder when it's done, retry when it failed, remove it unless it's running.
- The first launch is now a welcome screen instead of a technical setup screen. It explains what MimirDLP needs and shows one progress bar while it gets yt-dlp, FFmpeg and Deno. "Show details" still opens the full view.
- Tidier Settings: switches sit on the right like every other row, all dropdowns have the same width, and the download folder is shown so you can always read its name.

### Fixed

- Opens faster: MimirDLP no longer starts yt-dlp, FFmpeg and Deno on every launch just to check their versions.
- No more white flash on Windows while the window opens.

## [0.6.0] - 2026-11-01

### Added

- Clear error messages. When a download fails for a common reason, the card says what happened and what to do: sign in with your browser's cookies, update yt-dlp, wait after too many requests, check the link, free some disk space, and so on. yt-dlp's own message is still in the activity log.
- "Stop and save" for live streams. Once yt-dlp says a link is live, the Stop button saves what was recorded so far as a normal video file.
- A field for video passwords (Vimeo and others) in Settings > Advanced options > Signing in. The password is never saved to disk and never put on the command line.
- A choice between nightly and stable yt-dlp in ☰ > Setup and updates. Nightly stays the default, as yt-dlp recommends.
- A setting for how many times a failed part is retried, useful on an unstable connection.
- A notice when a newer version of MimirDLP is out. It checks the releases page once a day and sends nothing else. You can turn it off in Settings > Updates.
- A button in ☰ > Setup and updates that copies your MimirDLP and tool versions for bug reports, without any personal details.

### Changed

- Stopping a regular video still keeps its partial file, so starting again resumes it.
- Updated libraries, including security fixes for the TLS connections used to download yt-dlp, FFmpeg and Deno.

### Fixed

- The activity log no longer shows your IP address, which YouTube puts in its media links. Worth knowing if you ever pasted a log into a bug report.

## [0.5.0] - 2026-10-12

### Added

- MimirDLP opens the way you left it: same size, and maximized if it was. On Windows it also remembers where the window was on the screen.

### Changed

- Updated guide and screenshots.

## [0.4.1] - 2026-10-11

### Changed

- The Download screen no longer repeats "MimirDLP" in its header, since the window's title bar already shows it.
- The AppStream description no longer says the first launch downloads yt-dlp, FFmpeg and Deno: in the Flatpak they come included.
- Updated translations.

### Fixed

- Automatic subtitles and single-file downloads.

### Removed

- The Scoop manifest from `packaging/`.

## [0.3.0] - 2026-10-10

### Changed

- If MimirDLP is somewhere it can't write to (like Program Files), it now keeps its files in your user folder and saves to your Downloads folder, instead of stopping with an error.
- When a video was already downloaded, its card now says so instead of "Completed".
- Settings are kept when updating through Scoop.

## [0.2.0] - 2026-10-10

### Added

- If you copy a link and switch to MimirDLP, it already shows up in the box. You still press Add yourself.

## [0.1.0] - 2026-10-10

First release. Paste a link, choose video or audio, and download with yt-dlp. You can queue several links at once, and the queue is still there the next time you open it. Portable: extract it anywhere and run it. The first launch downloads yt-dlp, FFmpeg and Deno (around 200 MB) into a `bin` folder next to it. Available in English and Spanish.

[Unreleased]: https://github.com/indep-arg/MimirDLP/compare/v0.8.0...HEAD
[0.8.0]: https://github.com/indep-arg/MimirDLP/compare/v0.7.0...v0.8.0
[0.7.0]: https://github.com/indep-arg/MimirDLP/compare/v0.6.0...v0.7.0
[0.6.0]: https://github.com/indep-arg/MimirDLP/compare/v0.5.0...v0.6.0
[0.5.0]: https://github.com/indep-arg/MimirDLP/compare/v0.4.1...v0.5.0
[0.4.1]: https://github.com/indep-arg/MimirDLP/compare/v0.3.0...v0.4.1
[0.3.0]: https://github.com/indep-arg/MimirDLP/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/indep-arg/MimirDLP/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/indep-arg/MimirDLP/releases/tag/v0.1.0
