# Apple Music RPC

A small Windows tray app that shows what you're playing in Apple Music as your
Discord status ("Listening to Apple Music"), with song, artist, album art and a
progress bar. The tray menu doubles as a mini readout: current song, artist and
album.

## Setup

1. In the [Discord Developer Portal](https://discord.com/developers/applications)
   create a **New Application** named `Apple Music` and copy its **Application ID**.
2. Download `apple-music-rpc.exe` from the
   [Releases](https://github.com/laynem/apple-music-rpc/releases/latest) page, put it in a folder
   you'll keep, and run it.
3. With the ID on your clipboard, click OK in the setup prompt. (If a
   `config.json` from the earlier script version sits next to the exe, setup is skipped.)

Needs the Apple Music app for Windows (or iTunes), the Discord desktop app, and
Discord's *Settings > Activity Privacy > Share my activity* turned on.

## Tray menu

- **Apple Music** - green dot and "connected" while Apple Music has a song loaded,
  grey dot and "not connected" otherwise. Click it to open Apple Music.
- **Now Playing** - the song, with the artist and album below it. The header
  reads "Paused:" while playback is paused.
- **Start / Stop** - turn the Discord status on or off (icon is red / gray).
- **Show under my name** - the text Discord puts beneath your name:
  Artist, Song, Artist - Song, or Apple Music.
- **Start with Windows** - launch at login (on by default).
- **About** - version, who made it, a link to the download page, what's new,
  and **Check for updates** (also checked quietly at startup).
- **Open log**, **Exit**.

## config.json

Created next to the exe: `clientId`, `storefront` (two-letter country used for
album art lookups, default `us`), `showButton` (the "Play on Apple Music" button),
`statusLine` (`artist`, `song`, `both`, `app`).

## Build

`cargo build --release` on Windows.

To publish a version: set `version` in `Cargo.toml`, add it to `CHANGELOG.md`,
then push a tag such as `v1.2.0`. GitHub Actions builds the exe and publishes it
as that version's release, which is what the in-app update check looks at.
