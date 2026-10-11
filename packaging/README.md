# Packaging

Files for installing MimirDLP through package managers. The app itself is the same everywhere: a portable copy keeps everything next to the executable, and a copy installed where it can't write (a Flatpak, or `Program Files`) keeps its files in the user's data folder instead (`~/.local/share/MimirDLP`, `~/.var/app/io.github.indep_arg.MimirDLP/data/MimirDLP` inside the Flatpak, or `%LOCALAPPDATA%\MimirDLP` on Windows) and downloads to the user's Downloads folder.

## winget (Windows)

`winget/<version>/` holds the three manifests. The first version goes in by pull request to [microsoft/winget-pkgs](https://github.com/microsoft/winget-pkgs), as `manifests/i/indep-arg/MimirDLP/<version>/`. Its bots install the package in a sandbox before a maintainer merges it, which takes a few days.

Later versions: `wingetcreate update indep-arg.MimirDLP --version <version> --urls <zip url> --submit`, or the same three files with the version, URL, hash and date changed. The hash is the `.sha256` next to the release's zip, in capitals.

People install with `winget install indep-arg.MimirDLP`.

## Linux desktop files

`linux/` has the launcher and the AppStream metainfo (what Flathub, GNOME Software and Discover show), named after the app id `io.github.indep_arg.MimirDLP`. Add a `<release>` to the metainfo for every version. Check both with:

```
desktop-file-validate linux/io.github.indep_arg.MimirDLP.desktop
appstreamcli validate linux/io.github.indep_arg.MimirDLP.metainfo.xml
```

## Flathub

Not submitted yet. Flathub wants everything the app runs in the manifest, so the Flatpak brings its own yt-dlp and Deno and uses FFmpeg from the runtime, instead of downloading them at launch. The app already knows how to run that way; the manifest and the submission are written by the maintainer (Flathub doesn't accept AI-written manifests or submission pull requests).

What the app expects from the Flatpak:

- **App id `io.github.indep_arg.MimirDLP`.** The app switches to bundled tools only when `FLATPAK_ID` is exactly this id. Any other id (a portable copy started from another Flatpak's terminal, say) keeps the normal behaviour.
- **The tools on `PATH`.** It looks up `yt-dlp`, `ffmpeg`, `ffprobe` and `deno` in each `PATH` folder in turn, which inside a Flatpak is `/app/bin`, then the runtime's `/usr/bin`. Whatever the package puts in `/app/bin` wins over the runtime's.
- **yt-dlp:** the stable release from PyPI with the `default` and `curl-cffi` extras (`yt-dlp[default,curl-cffi]`). `default` brings `yt-dlp-ejs`, the scripts Deno runs for YouTube's challenges; `curl-cffi` lets yt-dlp impersonate a browser, which YouTube asks for. The standalone binaries carry both inside, the bare Python package neither. `flatpak-pip-generator` needs `--prefer-wheels=curl_cffi,cffi` for them: `curl_cffi`'s source package downloads a library while it builds, which a Flatpak build can't. The command it calls is `yt-dlp`.
- **Deno:** 2.3.0 or newer (yt-dlp's floor), as `deno`.
- **FFmpeg:** the runtime's `ffmpeg` and `ffprobe` (GNOME 50: FFmpeg 7.1). The base runtime leaves out the H.264 and HEVC decoders; the `org.freedesktop.Platform.codecs-extra` extension adds them. Remuxing doesn't need them, re-encoding does.
- **x86_64 only** to begin with.

What it does in that mode (`Paths::bundled` in `src/provision/mod.rs`):

- Settings, history and queue go to `$XDG_DATA_HOME/MimirDLP` (`~/.var/app/io.github.indep_arg.MimirDLP/data/MimirDLP`), downloads to the Downloads folder.
- It never downloads, installs or updates a tool. The ☰ menu shows **Components** instead of "Setup and updates", with the versions and no buttons.
- The "yt-dlp is old" notice waits 90 days instead of 14 (stable releases can be months apart) and says to update MimirDLP.
- "Signing in" (cookies from a browser) is hidden and ignored, since the sandbox can't read the browsers' folders.
- The menu-entry switch is hidden, as for any installed copy: the Flatpak brings its own `.desktop` (`linux/`).

Permissions it uses: the network, the display (Wayland, with X11 as a fallback), the GPU (without it iced draws in software, slower) and the Downloads folder. The folder picker goes through the desktop portal: a folder outside Downloads comes back as a document portal path (`/run/user/1000/doc/<id>/<folder>`), which is what Settings then shows, and downloads into it work.
