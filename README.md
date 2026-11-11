<div align="center">

<img src="ui/src/assets/logo.svg" alt="Osprey logo" height="120"/>

# Osprey

A window for [yt-dlp](https://github.com/yt-dlp/yt-dlp): paste a link, choose video or audio, press Download.

[![Latest release](https://img.shields.io/github/v/release/indep-arg/Osprey?label=release&color=2f6bea)](https://github.com/indep-arg/Osprey/releases/latest)
![Platform: Windows | Linux](https://img.shields.io/badge/platform-Windows%20%7C%20Linux-2f6bea)
![License: AGPL-3.0](https://img.shields.io/badge/license-AGPL--3.0-2f6bea)

<img src="screenshots/download.png" alt="Osprey downloading a video" width="100%"/>

</div>

## Install

Get the file for your system from the [latest release](https://github.com/indep-arg/Osprey/releases/latest), extract it wherever you like and run `osprey`. There is no installer.

| System  | File      | Needs |
|---------|-----------|-------|
| Windows | `.zip`    | Windows 10 or later (WebView2, which Windows 11 and an up-to-date Windows 10 already have) |
| Linux   | `.tar.gz` | glibc 2.39 or newer and WebKitGTK 4.1: Ubuntu 24.04, Mint 22, Fedora 40, Debian 13, Arch, openSUSE Tumbleweed |

Not sure about your Linux? `ldd --version` prints the glibc version on its first line. WebKitGTK comes with GNOME; elsewhere it is the package `libwebkit2gtk-4.1-0` (Debian, Ubuntu), `webkit2gtk4.1` (Fedora) or `webkit2gtk-4.1` (Arch).

The first time it opens, Osprey downloads yt-dlp, FFmpeg and Deno (about 200 MB) into its own folder. Nothing is added to your system. If that folder can't be written to (`Program Files`, for example), it uses your user folder instead.

Windows may warn that the app is unknown, because it isn't code signed yet: choose **More info**, then **Run anyway**.

## Use

1. Copy a link and switch to Osprey: the link is already in the box (Ctrl+V works too). Press **Add**.
2. Choose video or audio, and the quality, on the card that appears.
3. Press **Download**.

It works on every site yt-dlp supports. Subtitles, SponsorBlock, playlists and signed-in downloads are under **Settings**, in the ☰ menu.

## When a site stops working

It is nearly always an old yt-dlp. Open ☰, **Setup and updates**, and press **Check for updates**.

The [guide](docs/GUIDE.md) covers the rest: every option, common problems and building from source.

## Languages

English and Spanish so far. Osprey follows your system's language, and the ☰ menu lets you pick another. Each language is one file in [`crates/core/translations/`](crates/core/translations/); [translating](docs/TRANSLATING.md) says how to improve one or add yours.

## License

[AGPL-3.0-or-later](LICENSE). yt-dlp, FFmpeg and Deno have their own licenses. What you download is your responsibility: respect the law and each site's terms.
