# Packaging

Files for installing MimirDLP through package managers. The app itself is the same everywhere: a portable copy keeps everything next to the executable, and a copy installed where it can't write (a Flatpak, or `Program Files`) keeps its files in the user's data folder instead (`~/.local/share/MimirDLP`, or `%LOCALAPPDATA%\MimirDLP` on Windows) and downloads to the user's Downloads folder.

## Scoop (Windows)

`scoop/mimirdlp.json` is the manifest. It persists `bin`, `downloads` and the settings, history and queue files, so updates keep the tools and the user's videos.

Scoop reads manifests from a bucket, a repository of its own: [indep-arg/scoop-bucket](https://github.com/indep-arg/scoop-bucket), made like this:

1. Create `indep-arg/scoop-bucket` from [ScoopInstaller/BucketTemplate](https://github.com/ScoopInstaller/BucketTemplate) ("Use this template").
2. Put `mimirdlp.json` in its `bucket/` folder.

Its scheduled workflow (Excavator) reads `checkver` and `autoupdate`, so new releases are picked up without touching the manifest again. People install with:

```
scoop bucket add mimirdlp https://github.com/indep-arg/scoop-bucket
scoop install mimirdlp
```

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

Not done yet. Flathub builds everything the app runs from source, so the Flatpak would have to bring its own yt-dlp, FFmpeg and Deno rather than download them, and the app would need to use those.
