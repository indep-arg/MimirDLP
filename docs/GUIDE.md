# MimirDLP guide

Everything the [README](../README.md) leaves out. If you just want to download a video, the README is enough.

- [Features](#features)
- [Installing](#installing)
- [The Flatpak](#the-flatpak)
- [First launch](#first-launch)
- [Downloading](#downloading)
- [SponsorBlock](#sponsorblock)
- [Things worth knowing](#things-worth-knowing)
- [When something goes wrong](#when-something-goes-wrong)
- [Building from source](#building-from-source)
- [Contributing](#contributing)

The name comes from Mimir, who guards the well of wisdom in Norse mythology.

## Features

- **Preview before you commit**: pasting a link looks it up right away: title, thumbnail and duration, or for a playlist its name and how many videos it has.
- **A queue**: paste one link or a whole batch at once. They download one after another, each with its own progress and its own format/quality pick. A failed one can be retried, finished ones cleared away, and whatever is still waiting is there again the next time you open the app.
- **Portable**: yt-dlp, FFmpeg and Deno live in a `bin/` folder next to the app. Nothing is added to your system or your PATH, and no admin/root rights are needed.
- **Verified tools**: each one is checked against the SHA256 its project publishes before it runs, and yt-dlp's signature against its key built into the app (no GnuPG needed).
- **Subtitles, thumbnails, metadata, chapters**: embed what you want, skip what you don't.
- **SponsorBlock**: mark sponsor segments as chapters, or cut them out of the file.
- **Playlists**: the whole thing, one video from it, or a range.
- **Signed-in downloads**: videos that need an account (age restricted, members only) can use your browser's cookies (not in the Flatpak).
- **A history you control**: skip what you've already downloaded, stop as soon as something already in the history shows up, or just keep a plain download count.
- **Light and dark themes**, following your system by default.

## Installing

Grab the latest build for your platform from the [Releases page](https://github.com/indep-arg/MimirDLP/releases):

| Your computer | Download |
|---|---|
| Windows (x86_64) | `mimirdlp-<version>-windows-x86_64.zip` |
| Linux (x86_64) | `mimirdlp-<version>-linux-x86_64.tar.gz` |

Each comes with a `.sha256` file to check the download. Extract it anywhere and run `mimirdlp` (`mimirdlp.exe` on Windows). There's no installer: the folder works the same from a USB stick as from your user folder.

In a folder it can't write to (`Program Files`, or installed through a package manager), it keeps its tools, settings and history in your user's data folder instead (`~/.local/share/MimirDLP` on Linux, `%LOCALAPPDATA%\MimirDLP` on Windows) and saves to your Downloads folder.

The Linux build needs glibc 2.39 or newer (Ubuntu 24.04, Debian 13, Fedora 40 or later).

## The Flatpak

A Flatpak for Flathub is on its way. How it differs from the portable build:

| | Portable (`.tar.gz`) | Flatpak |
|---|---|---|
| Runs on | x86_64 distributions with glibc 2.39 or newer | Any x86_64 distribution with Flatpak, old or new |
| yt-dlp, FFmpeg, Deno | Downloaded by the app on first launch, into `bin/` next to it | Inside the package |
| yt-dlp channel | Nightly, as yt-dlp recommends | Stable releases, which Flathub packages |
| FFmpeg | FFmpeg-Builds (BtbN) | The one in the GNOME runtime |
| Updates | Setup, "Check for updates", from inside the app | With the package: your software center, or `flatpak update` |
| Settings, history, queue | Next to the app | `~/.var/app/io.github.indep_arg.MimirDLP/data/MimirDLP` |
| Saves to | `downloads` next to the app | Your Downloads folder |
| Browser cookies (signing in) | Yes | Not yet: the sandbox can't read the browsers' folders |

In the Flatpak the ☰ menu has **Components** instead of "Setup and updates", showing the tools' versions. Stable yt-dlp releases can be months apart, so the "yt-dlp is old" reminder waits 90 days instead of two weeks. If a site stops working before that, the portable build gets the fix first.

## First launch

MimirDLP opens on **Setup** and installs yt-dlp, FFmpeg and Deno (about 200 MB), then moves on to the Download screen. If the install fails (no internet, say), "Install missing components" tries again. The Flatpak skips this step.

To check for updates later, use ☰, **Setup and updates**; only what's out of date is downloaded again. When yt-dlp is more than two weeks old the Download screen reminds you, since an old yt-dlp is the usual reason a site stops working.

## Downloading

Works on any site yt-dlp supports. On the Download screen, paste one link or several (separated by spaces or new lines). A link you copied elsewhere appears in the empty box when you switch to MimirDLP; it's only queued when you press Add, and one you clear isn't put back. Each link is looked up and added to the queue as a card, where you can pick its format and quality until it starts.

Press Download and they run one at a time, each with its own progress bar. A finished card gets its own "Open folder" straight to that file. Stop also stops the FFmpeg that yt-dlp may have started, and keeps the partial file so starting again resumes it. Closing the window does the same.

Subtitles, thumbnails, SponsorBlock, file naming and everything else that isn't on the card lives in **Settings** (in the ☰ menu), and applies to whatever you queue next; links already in the queue keep what they were queued with. Anything yt-dlp would refuse (an empty custom format string, an invalid SponsorBlock category list, a malformed speed limit) is caught before yt-dlp is started: that card is marked failed with the reason, the rest of the queue carries on, and Retry runs it again once it's fixed.

## SponsorBlock

Sponsor segments can be marked as chapters, so you can skip them yourself, or cut out of the file. It uses the community database at [sponsor.ajay.app](https://sponsor.ajay.app), so it only does anything on YouTube.

By default the cuts are made without re-encoding, which is fast but can leave a small glitch right at the seam. There is an option to force keyframes at the cuts instead, which looks cleaner but re-encodes the whole video and takes much longer.

If the SponsorBlock server can't be reached, yt-dlp fails the download instead of skipping the segments. If that happens, turn SponsorBlock off and try again.

## Things worth knowing

- On Linux, Settings has a switch to show MimirDLP in the applications menu. It is off by default, since the app installs nothing on its own; turned on, it writes a launcher and the icon to `~/.local/share` (which is also what lets KDE and GNOME show the icon in the taskbar on Wayland), and turning it off removes them. If you move the app's folder, the launcher follows it the next time you open the app; another copy that is still in place keeps it.
- Settings, the window's size and the download history live next to the application (`mimirdlp.config`, `mimirdlp.window`, `download_archive.txt`), not in `~/.config` or `%APPDATA%`, so the folder stays portable. A relative output folder or history file name is next to the application too.
- Subtitles in "all" languages leave out YouTube's live chat replay, which isn't subtitles at all. Many YouTube videos only have the auto-generated kind; there's a toggle for those. With that toggle on, write each language code (`en`, or `en,es`) instead of leaving it empty or using a pattern like `en.*`: YouTube offers every subtitle machine translated into every language, thousands of tracks on a single video, and asking for all of them gets the download blocked (HTTP 429). Settings warns about it, and such a link isn't started. Subtitles are saved in whatever format the site serves (usually `.vtt`).
- "Best single file" takes a file that already has video and audio together, which is faster when the site has one. YouTube doesn't always offer one; then the best video and audio are downloaded and merged, as with "Best quality".
- Converting the container comes in two flavours. Remux just rewraps the existing streams, which is nearly instant but fails if the codecs don't fit the target container. Re-encode always works but is slow and costs some quality.
- Audio extraction is turned off when you pick a container conversion or a video format other than a custom string, and the other way around, since extracting audio throws away the video those would apply to.
- Thumbnails only fit in some file types. WAV can't hold one, so the thumbnail is skipped for it. When nothing else decides the container, a webm result is rewrapped into `.mkv` (or `.opus` for audio, or `.mp4` if MP4 is the chosen container) so the thumbnail fits, without re-encoding anything.
- yt-dlp's own configuration files (`yt-dlp.conf`, `~/.config/yt-dlp/config` and the like) are ignored: yt-dlp would otherwise read one even from the download folder, and such a file can run commands. Everything the app does is set in its Settings.

## When something goes wrong

- **The app stays on Setup**: a component couldn't be installed. The activity log there says why; "Install missing components" tries again.
- **A component shows "Installed but not responding"**: it downloaded but won't run (a corrupted download, or the wrong build for your machine). Use "Install missing components" again to replace it.
- **A download just fails**: first make sure yt-dlp is up to date (☰, "Setup and updates", "Check for updates"; in the Flatpak, update MimirDLP itself). Then open Settings, "Advanced options", turn on Verbose, and read what yt-dlp actually says under "Show activity" on the Download screen. Half the time it's a region lock or a site that wants a login; for the latter, pick your browser in Settings, "Advanced options", "Signing in" (Firefox works best; this isn't available in the Flatpak). Chromium browsers may need to be closed first, and recent ones on Windows can't be read at all.
- **Windows SmartScreen warns on first run**: the app isn't code signed yet. Choose "More info", then "Run anyway".

## Building from source

Requires a current stable [Rust toolchain](https://rustup.rs).

```
cargo build --release
```

The binary lands in `target/release/`. Run it in place with `cargo run`; `YTP_APP_DIR=<dir> cargo run` points it at a scratch folder instead of the executable's own directory, which is convenient during development.

## Contributing

Issues and pull requests are welcome. If you're changing behaviour rather than fixing a typo, these should all pass first; they are exactly the checks CI runs on every push:

```
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --all-targets --locked
```

If you touched what gets asked of yt-dlp, also run the real-download tests, which check the resulting files with ffprobe: `cargo test --test e2e -- --ignored` (a couple of minutes, needs internet).
