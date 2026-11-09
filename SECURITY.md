# Security

MimirDLP downloads yt-dlp, FFmpeg and Deno and runs them, so a problem in how it fetches, checks or starts them matters.

## Reporting a problem

Please don't open a public issue. Use [Report a vulnerability](https://github.com/indep-arg/MimirDLP/security/advisories/new) on GitHub, which keeps the report private until it is fixed. Say what you found, how to reproduce it and which version you used.

Only the latest release gets fixes.

## What the app checks

- Each tool is compared with the SHA256 its project publishes before it is installed.
- yt-dlp's checksum file must also be signed with yt-dlp's own key, which is built into the app. FFmpeg-Builds and Deno publish checksums only, so for those two the check proves the file arrived whole, not who made it.
- yt-dlp's configuration files are ignored, since one could make it run commands.
- A video password is handed to yt-dlp privately and never saved or put on its command line.

## Checking a release

Each archive on the release page has a `.sha256` next to it, and a build attestation saying it was built by this repository's release workflow:

```
gh attestation verify mimirdlp-<version>-windows-x86_64.zip --repo indep-arg/MimirDLP
```

The Windows build isn't code signed yet, which is why SmartScreen warns the first time.
