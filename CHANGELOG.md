# Changelog

## 1.1.0

- No setup needed: the app now comes with a Discord application built in, so it
  works as soon as you run it. To use your own, put its Application ID in
  `config.json` as `clientId`.

## 1.0.2

- Tray menu now shows a "Now Playing:" (or "Paused:") header with the song and
  the artist and album below it. The progress row is gone, because it could not
  update while the menu was open.

## 1.0.1

- Fixed the album showing twice on the Discord card in "Artist - Song" mode.
- Clicking the Apple Music row in the tray menu opens Apple Music, or brings it
  to the front when it is already running.

## 1.0.0

First public release.

- Shows your Apple Music track as a Discord "Listening to" status, with song,
  artist, album art and a progress bar.
- Tray menu mini player: song, artist and album, progress, length and time left.
- Apple Music status row with a green dot when it is connected.
- Start / Stop for the Discord status, and Start with Windows.
- "Show under my name": Artist, Song, Artist - Song, or Apple Music.
- About menu with the version, a link to the download page, and Check for updates.
