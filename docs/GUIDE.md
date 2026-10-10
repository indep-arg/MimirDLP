# MimirDLP guide

Everything the [README](../README.md) leaves out. If you just want to download a video, the README is enough.

- [Features](#features)
- [Installing](#installing)
- [First launch](#first-launch)
- [Downloading](#downloading)
- [SponsorBlock](#sponsorblock)
- [Things worth knowing](#things-worth-knowing)
- [When something goes wrong](#when-something-goes-wrong)
- [Building from source](#building-from-source)
- [Contributing](#contributing)

In Norse mythology, Mimir guards a well of wisdom so deep that Odin gave up an eye just to drink from it. MimirDLP is a smaller trade: paste a link, and it goes and fetches the knowledge (and the video) for you, no sacrifice required.

## Features

- **Preview before you commit**: pasting a link looks it up right away, title, thumbnail and duration included (or, for a playlist, its name and how many videos it holds).
- **A queue**: paste one link or a whole batch at once. They download one after another, each with its own progress and its own format/quality pick. A failed one can be retried, finished ones cleared away, and whatever is still waiting is there again the next time you open the app.
- **Fully portable**: yt-dlp, FFmpeg and Deno live in a `bin/` folder next to the app. No installer, no admin/root rights, nothing added to your system or your PATH. Copy the folder anywhere and it still works.
- **Verified installs**: every binary is downloaded from its own official releases and checked against the published SHA256 before it's allowed to run. For yt-dlp, which signs its checksums, the signature is checked too, against yt-dlp's own key built into the app, so it's not just "the file arrived whole" but "yt-dlp published it". Nothing extra to install for that (no GnuPG), and nothing left behind.
- **Subtitles, thumbnails, metadata, chapters**: embed what you want, skip what you don't.
- **SponsorBlock**: mark sponsor segments as chapters, or cut them out of the file entirely.
- **Playlists**: grab the whole thing, just one video from it, or a specific range.
- **Signed-in downloads**: videos that need an account (age restricted, members only) can use your browser's cookies.
- **A history you control**: skip what you've already downloaded, stop as soon as something already in the history shows up, or just keep a plain download count.
- **Light and dark themes**, following your system by default.
- **Windows and Linux**, from the same codebase, both covered by the same tests.

## Installing

Grab the latest build for your platform from the [Releases page](https://github.com/indep-arg/MimirDLP/releases):

| Your computer | Download |
|---|---|
| Windows (x86_64) | `mimirdlp-<version>-windows-x86_64.zip` |
| Linux (x86_64) | `mimirdlp-<version>-linux-x86_64.tar.gz` |

Both come with a `.sha256` file, so you can check the download wasn't corrupted or tampered with before running it. Extract the archive anywhere you can write to and run `mimirdlp` (or `mimirdlp.exe` on Windows). There's no installer, and the whole folder is portable, so it works just as well from a USB stick as from your user folder.

Not from `Program Files`, though: the app keeps its tools, settings and history in its own folder, and Windows doesn't let a normal user write there (the Setup screen says so if that's where it is).

The Linux build needs a recent distribution (glibc 2.39 or newer, for example Ubuntu 24.04).

## First launch

On first launch MimirDLP opens on **Setup** and installs yt-dlp, FFmpeg and Deno by itself (about 200 MB). Every binary gets checksummed against what the upstream project publishes before it's allowed to run. Once everything is in place it moves on to the Download screen. If the install fails (no internet, say), "Install missing components" tries again.

Come back any time through the ☰ menu, **Setup and updates**, to check for updates. If yt-dlp gets more than two weeks old, the Download screen reminds you, since an old yt-dlp is the usual reason a site suddenly stops working. It only re-downloads what's actually out of date, and yt-dlp comes from the nightly channel, since that's the one yt-dlp itself recommends for getting extractor fixes quickly.

## Downloading

Works on YouTube, Twitch, and pretty much anywhere else yt-dlp does, which by now is most of the internet. On the Download screen, paste a link (or several, separated by spaces or new lines). Each one is looked up right away and added to the queue as a card, where you can pick its format and quality until it starts.

Press Download and they run one at a time, each with its own progress bar. A finished card gets its own "Open folder" straight to that file. Stop really stops, including the FFmpeg that yt-dlp may have started, and the partial file is kept so starting again resumes it. Closing the window does the same.

Subtitles, thumbnails, SponsorBlock, file naming and everything else that isn't on the card itself lives in **Settings** (in the ☰ menu), and applies to whatever you queue next; links already in the queue keep what they were queued with. Anything yt-dlp would refuse (an empty custom format string, an invalid SponsorBlock category list, a malformed speed limit, that kind of thing) is caught before yt-dlp is started: that card is marked failed with the reason, the rest of the queue carries on, and Retry runs it again once it's fixed.

## SponsorBlock

Sponsor segments can be either marked as chapters, so you can skip them yourself, or cut straight out of the file. It uses the community database at [sponsor.ajay.app](https://sponsor.ajay.app), so it only does anything on YouTube.

Cutting really does re-cut the file, so an 18 minute video with a minute of sponsor reads comes out a minute shorter. By default the cuts are made without re-encoding, which is fast but can leave a small glitch right at the seam. There is an option to force keyframes at the cuts instead, which looks cleaner but re-encodes the whole video and takes much longer.

One caveat that comes from yt-dlp itself: if the SponsorBlock server can't be reached, the download fails instead of just skipping the segments. If that happens, turn SponsorBlock off and try again.

## Things worth knowing

- On Linux, Settings has a switch to show MimirDLP in the applications menu. It is off by default, since the app installs nothing on its own; turned on, it writes a launcher and the icon to `~/.local/share` (which is also what lets KDE and GNOME show the icon in the taskbar on Wayland), and turning it off removes them. If you move the app's folder, the launcher follows it the next time you open the app; another copy that is still in place keeps it.
- Settings and the download history live next to the application (`mimirdlp.config`, `download_archive.txt`), not in `~/.config` or `%APPDATA%`, so the whole folder stays portable. A relative output folder or history file name is next to the application too.
- Subtitles in "all" languages leave out YouTube's live chat replay, which isn't subtitles at all. Many YouTube videos only have the auto-generated kind; there's a toggle for those. Subtitles are saved in whatever format the site serves (usually `.vtt`).
- Converting the container comes in two flavours. Remux just rewraps the existing streams, which is nearly instant but fails if the codecs don't fit the target container. Re-encode always works but is slow and costs some quality.
- Audio extraction is turned off when you pick a container conversion or a video format other than a custom string, and the other way around, since extracting audio throws away the video those would apply to.
- Thumbnails only fit in some file types. WAV can't hold one, so the thumbnail is simply skipped for it. When nothing else decides the container, a webm result is rewrapped into `.mkv` (or `.opus` for audio, or `.mp4` if MP4 is the chosen container) so the thumbnail fits, without re-encoding anything.
- yt-dlp's own configuration files (`yt-dlp.conf`, `~/.config/yt-dlp/config` and the like) are ignored: yt-dlp would otherwise read one even from the download folder, and such a file can run commands. Everything the app does is set in its Settings.
- Everything yt-dlp itself already defaults to sensibly, this app leaves alone. The options here are the ones that are actually worth having an opinion about.

## When something goes wrong

- **The app stays on Setup**: a component couldn't be installed. The activity log there says why; "Install missing components" tries again.
- **A component shows "Installed but not responding"**: it downloaded but won't run (a corrupted download, or the wrong build for your machine). Use "Install missing components" again to replace it.
- **A download just fails**: first make sure yt-dlp is up to date (☰, "Setup and updates", "Check for updates"). Then open Settings, "Advanced options", turn on Verbose, and read what yt-dlp actually says under "Show activity" on the Download screen. Half the time it's a region lock or a site that wants a login; for the latter, pick your browser in Settings, "Advanced options", "Signing in" (Firefox works best). Chromium browsers may need to be closed first, and recent ones on Windows can't be read at all.
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
