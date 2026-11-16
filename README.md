# ObiPlayer

A music player for [MPD](https://www.musicpd.org/) — local or remote — for people who care about
editions, pressings and sound quality. GTK4/libadwaita, written in Rust. Linux port of DrPlayer (macOS).

- **Library**: album grid, artists, labels, composers, producers, tracks table, search (Ctrl+F),
  editions of the same record, versions of a track with a quality score, ratings, preferred versions.
- **Exact-pressing metadata**: MusicBrainz + Discogs cross-checked against your files' label and
  catalogue number; Wikipedia, Last.fm (bios, tags, similar artists); certified pressing sidecars.
- **Analysis**: DR14 dynamic range, waveform seek bar, oscilloscope/spectrum/circular visualizers,
  bit-perfect signal path indicator, Loudness War DB comparison.
- **Listening**: synced lyrics (LRCLIB), radio, smart playlists, stats, home shelves, MPRIS media keys.
- English and Spanish.

## Requirements
GTK 4.12+, libadwaita 1.5+, ffmpeg/ffprobe (analysis). An MPD server (`mpd` locally, or any host on
your network). For covers, DR, waveforms and visualizers with a remote server, mount its music
folder (NFS/SMB) and point *Preferences › Music folder* at it (*Detect* finds it among your mounts).

## Build & run
```
cargo run -p obiplayer --release
```
First run opens a setup assistant. Configuration: `~/.config/obiplayer/config.toml`
(`MPD_HOST`/`MPD_PORT` override it). Caches: `~/.cache/obiplayer`. Play history and smart
playlists: `~/.local/share/obiplayer`.

## Packages
- Arch: `cd packaging/arch && makepkg -si`
- Flatpak: `packaging/flatpak/us.priet.ObiPlayer.yml` (needs `cargo-sources.json` from flatpak-cargo-generator)

## Translations
UI strings are English in the code. `python3 scripts/extract_strings.py` refreshes
`po/obiplayer.pot`; translations live in `po/<lang>.po` and are embedded at build time.

License: GPL-3.0-or-later.
