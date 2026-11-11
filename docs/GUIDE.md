# Osprey guide

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

The name is the bird's: an osprey dives for what it wants and carries it home.

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

Grab the latest build for your platform from the [Releases page](https://github.com/indep-arg/Osprey/releases):

| Your computer | Download |
|---|---|
| Windows (x86_64) | `osprey-<version>-windows-x86_64.zip` |
| Linux (x86_64) | `osprey-<version>-linux-x86_64.tar.gz` |

Each comes with a `.sha256` file to check the download. Extract it anywhere and run `osprey` (`osprey.exe` on Windows). There's no installer: the folder works the same from a USB stick as from your user folder.

In a folder it can't write to (`Program Files`, or installed through a package manager), it keeps its tools, settings and history in your user's data folder instead (`~/.local/share/Osprey` on Linux, `%LOCALAPPDATA%\Osprey` on Windows) and saves to your Downloads folder.

The Linux build needs glibc 2.39 or newer (Ubuntu 24.04, Debian 13, Fedora 40 or later) and WebKitGTK 4.1, which draws the window: it comes with GNOME, and elsewhere it is the package `libwebkit2gtk-4.1-0` (Debian, Ubuntu), `webkit2gtk4.1` (Fedora) or `webkit2gtk-4.1` (Arch). On Windows the window is drawn by WebView2, part of Windows 11 and of an up-to-date Windows 10.

## The Flatpak

A Flatpak for Flathub is on its way. How it differs from the portable build:

| | Portable (`.tar.gz`) | Flatpak |
|---|---|---|
| Runs on | x86_64 distributions with glibc 2.39 or newer | Any x86_64 distribution with Flatpak, old or new |
| yt-dlp, FFmpeg, Deno | Downloaded by the app on first launch, into `bin/` next to it | Inside the package |
| yt-dlp channel | Nightly, as yt-dlp recommends | Stable releases, which Flathub packages |
| FFmpeg | FFmpeg-Builds (BtbN) | The one in the GNOME runtime |
| Updates | Setup, "Check for updates", from inside the app | With the package: your software center, or `flatpak update` |
| Settings, history, queue | Next to the app | `~/.var/app/io.github.indep_arg.Osprey/data/Osprey` |
| Saves to | `downloads` next to the app | Your Downloads folder |
| Browser cookies (signing in) | Yes | Not yet: the sandbox can't read the browsers' folders |

In the Flatpak the ☰ menu has **Components** instead of "Setup and updates", showing the tools' versions. Stable yt-dlp releases can be months apart, so the "yt-dlp is old" reminder waits 90 days instead of two weeks. If a site stops working before that, the portable build gets the fix first.

## First launch

The first time, Osprey welcomes you and installs yt-dlp, FFmpeg and Deno (about 200 MB) by itself, with one progress bar for the whole install; "Show details" opens the full Setup screen with every step. Then it moves on to the Download screen. If the install fails (no internet, say), it says why, and "Try again" picks up from there. The Flatpak skips this step.

To check for updates later, use ☰, **Setup and updates**; only what's out of date is downloaded again. When yt-dlp is more than two weeks old the Download screen reminds you, since an old yt-dlp is the usual reason a site stops working.

When a new version of Osprey itself is out, the Download screen says so, with a button to its release page. To know, the app looks at that page on GitHub once a day when it starts; nothing about you or your downloads is sent, and nothing is installed by itself. Settings, "Updates" turns it off. The Flatpak doesn't look: Flathub updates it.

The same Setup screen chooses which yt-dlp builds to use. **Nightly**, the default and what yt-dlp recommends, gets fixes for sites that changed within a day. **Stable** releases are tested for longer but come weeks apart, so with them the reminder waits 45 days. Switching replaces the installed yt-dlp right away, with the same signature check as always.

## Downloading

Works on any site yt-dlp supports. On the Download screen, paste one link or several (separated by spaces or new lines). A link you copied elsewhere appears in the empty box when you switch to Osprey; it's only queued when you press Add, and one you clear isn't put back. Each link is looked up and added to the queue as a card, where you can pick its format and quality until it starts.

Press Download and they run one at a time, each with its own progress bar. A finished card can open its file or show it in its folder, any card can copy its link, and a failed one says why, with Retry under it. Stop also stops the FFmpeg that yt-dlp may have started, and keeps the partial file so starting again resumes it. Closing the window does the same.

Subtitles, thumbnails, SponsorBlock, file naming and everything else that isn't on the card lives in **Settings** (in the ☰ menu), and applies to whatever you queue next; links already in the queue keep what they were queued with. Anything yt-dlp would refuse (an empty custom format string, an invalid SponsorBlock category list, a malformed speed limit) is caught before yt-dlp is started: that card is marked failed with the reason, the rest of the queue carries on, and Retry runs it again once it's fixed.

## SponsorBlock

Sponsor segments can be marked as chapters, so you can skip them yourself, or cut out of the file. It uses the community database at [sponsor.ajay.app](https://sponsor.ajay.app), so it only does anything on YouTube.

By default the cuts are made without re-encoding, which is fast but can leave a small glitch right at the seam. There is an option to force keyframes at the cuts instead, which looks cleaner but re-encodes the whole video and takes much longer.

If the SponsorBlock server can't be reached, yt-dlp fails the download instead of skipping the segments. If that happens, turn SponsorBlock off and try again.

## Things worth knowing

- On Linux, Settings has a switch to show Osprey in the applications menu. It is off by default, since the app installs nothing on its own; turned on, it writes a launcher and the icon to `~/.local/share` (which is also what lets KDE and GNOME show the icon in the taskbar on Wayland), and turning it off removes them. If you move the app's folder, the launcher follows it the next time you open the app; another copy that is still in place keeps it.
- Settings, the window's size and the download history live next to the application (`osprey.config`, `osprey.window`, `download_archive.txt`), not in `~/.config` or `%APPDATA%`, so the folder stays portable. A relative output folder or history file name is next to the application too.
- Subtitles in "all" languages leave out YouTube's live chat replay, which isn't subtitles at all. Many YouTube videos only have the auto-generated kind; there's a toggle for those. With that toggle on, write each language code (`en`, or `en,es`) instead of leaving it empty or using a pattern like `en.*`: YouTube offers every subtitle machine translated into every language, thousands of tracks on a single video, and asking for all of them gets the download blocked (HTTP 429). Settings warns about it, and such a link isn't started. Subtitles are saved in whatever format the site serves (usually `.vtt`).
- "Best single file" takes a file that already has video and audio together, which is faster when the site has one. YouTube doesn't always offer one; then the best video and audio are downloaded and merged, as with "Best quality".
- Converting the container comes in two flavours. Remux just rewraps the existing streams, which is nearly instant but fails if the codecs don't fit the target container. Re-encode always works but is slow and costs some quality.
- Audio extraction is turned off when you pick a container conversion or a video format other than a custom string, and the other way around, since extracting audio throws away the video those would apply to.
- Thumbnails only fit in some file types. WAV can't hold one, so the thumbnail is skipped for it. When nothing else decides the container, a webm result is rewrapped into `.mkv` (or `.opus` for audio, or `.mp4` if MP4 is the chosen container) so the thumbnail fits, without re-encoding anything.
- A video protected by a password (on Vimeo, for example) needs it in Settings, "Advanced options", "Signing in", before its link is added. The password is never saved, neither in the settings nor in the queue, and isn't put on yt-dlp's command line, where other programs could read it: it is handed over privately and forgotten when the app closes.
- A live stream records until it ends, or until you press **Stop and save** (the Stop button turns into it once yt-dlp says the link is live). What was recorded so far is then copied into a normal file with FFmpeg, without re-encoding, and the card links to it. Stopping a video that isn't live still keeps its partial file, so starting again resumes it.
- On an unstable connection, "Retries after a network error" (Settings, "Advanced options") lets yt-dlp try a failed part again more times. Empty keeps yt-dlp's default of 10.
- Only one Osprey runs at a time: starting it again brings the window that is already open to the front, so two of them never write the same queue.
- yt-dlp's own configuration files (`yt-dlp.conf`, `~/.config/yt-dlp/config` and the like) are ignored: yt-dlp would otherwise read one even from the download folder, and such a file can run commands. Everything the app does is set in its Settings.

## When something goes wrong

- **The app stays on Setup**: a component couldn't be installed. The activity log there says why; "Install missing components" tries again.
- **A component shows "Installed but not responding"**: it downloaded but won't run (a corrupted download, or the wrong build for your machine). Use "Install missing components" again to replace it.
- **A download just fails**: read the card first. For the common problems (a site that wants you signed in or checks you're not a bot, a region lock, too many requests, a removed video, a full disk) it says what happened and what to do; yt-dlp's own message is in the activity log. Otherwise, make sure yt-dlp is up to date (☰, "Setup and updates", "Check for updates"; in the Flatpak, update Osprey itself). Then open Settings, "Advanced options", turn on Verbose, and read what yt-dlp actually says under "Show activity" on the Download screen. Half the time it's a region lock or a site that wants a login; for the latter, pick your browser in Settings, "Advanced options", "Signing in" (Firefox works best; this isn't available in the Flatpak). Chromium browsers may need to be closed first, and recent ones on Windows can't be read at all.
- **Reporting a bug**: ☰, "Setup and updates", "Copy diagnostic information" copies the versions of Osprey and its tools, the system and how the app was installed, to paste into the report. It holds no folders, links or user names.
- **Windows SmartScreen warns on first run**: the app isn't code signed yet. Choose "More info", then "Run anyway".

## Building from source

Requires a current stable [Rust toolchain](https://rustup.rs) and [Node.js](https://nodejs.org) 22 or newer, plus on Linux the [libraries Tauri needs](https://tauri.app/start/prerequisites/) (WebKitGTK 4.1 and GTK 3, with their development files).

```
npm install
npx tauri build --no-bundle
```

The binary lands in `target/release/`, with the interface inside it. `npm run tauri dev` runs the app while you work on it, reloading the interface as it changes; with `OSPREY_APP_DIR=<dir>` in the environment it uses a scratch folder instead of the executable's own directory.

The code is in three places:

- `crates/core`: everything that isn't the window. What yt-dlp is asked for, running it, installing the tools, the queue, the settings and the translations. It has no dependency on the interface and holds nearly all the tests.
- `crates/desktop`: the window ([Tauri](https://tauri.app)) and the commands the interface can call.
- `ui`: the interface itself (Vue and TypeScript). It only shows the state the core sends and asks for actions; it never touches files, the network or the clipboard by itself.

## Contributing

Issues and pull requests are welcome. If you're changing behaviour rather than fixing a typo, these should all pass first; they are exactly the checks CI runs on every push:

```
npm run build
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --all-targets --locked
```

If you touched what gets asked of yt-dlp, also run the real-download tests, which check the resulting files with ffprobe: `cargo test -p osprey-core --test e2e -- --ignored` (a couple of minutes, needs internet). The "E2E" workflow runs them on Linux and Windows from the Actions tab.

On Windows, `scripts/stress-launch.ps1` opens and closes a build many times and reports slow starts, hangs on close, crashes, processes left behind, idle CPU and memory. Its header lists the options.
