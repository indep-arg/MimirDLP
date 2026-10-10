<div align="center">

<img src="branding/logo.svg" alt="MimirDLP logo" height="120"/>

# MimirDLP

Download videos and music from YouTube and most other sites, with a simple window instead of a command line.

![Latest release](https://img.shields.io/github/v/release/indep-arg/MimirDLP?label=release&color=3584e4)
![Platform: Windows | Linux](https://img.shields.io/badge/platform-Windows%20%7C%20Linux-3584e4)
![License: MIT](https://img.shields.io/github/license/indep-arg/MimirDLP?color=3584e4)
[![Translation status](https://hosted.weblate.org/widget/mimirdlp/svg-badge.svg)](https://hosted.weblate.org/engage/mimirdlp/)

<img src="screenshots/download.png" alt="MimirDLP downloading a video" width="100%"/>

</div>

## Download

<p align="center">
  <a href="https://github.com/indep-arg/MimirDLP/releases/latest"><img src="https://img.shields.io/badge/Download_for-Windows-3584e4?style=for-the-badge" alt="Download for Windows"/></a>
  <a href="https://github.com/indep-arg/MimirDLP/releases/latest"><img src="https://img.shields.io/badge/Download_for-Linux-3584e4?style=for-the-badge" alt="Download for Linux"/></a>
</p>

On the release page, take the `.zip` for Windows (10 or later) or the `.tar.gz` for Linux (a recent distribution, such as Ubuntu 24.04 or later). There's no installer: extract it anywhere and run `mimirdlp`. Everything it needs stays in that folder (or in your user folder, if it's somewhere it can't write to, like `Program Files`).

## How to use it

1. Open MimirDLP. The first time, it downloads the tools it needs by itself (about 200 MB).
2. Copy a link and switch to MimirDLP: it shows up in the box by itself (or paste it with Ctrl+V). Press **Add**, then pick video or audio and the quality.
3. Press **Download**.

Subtitles, SponsorBlock, playlists and signed-in downloads are in **Settings**, in the ☰ menu. It runs on [yt-dlp](https://github.com/yt-dlp/yt-dlp), so it works wherever yt-dlp does.

## Something not working?

Open ☰, **Setup and updates**, and check for updates first: an old yt-dlp is the usual reason a site stops working. More help, every option explained, and how to build it yourself are in the [guide](docs/GUIDE.md).

## Languages

English and Spanish so far, following your system's language (or pick one in the ☰ menu). Translations are done on [Weblate](https://hosted.weblate.org/engage/mimirdlp/): anyone can help there, no programming needed. More in [translating](docs/TRANSLATING.md).

## License

[MIT](LICENSE). yt-dlp, FFmpeg and Deno have their own licenses. Use it responsibly and within the law and each site's terms; you are responsible for what you download.
