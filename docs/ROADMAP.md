# Cadenza — Roadmap

Each milestone is a vertical slice that ends with a usable, tested app and a demo script
(`docs/demos/Mn.md`). "Done" means the following:
- features work end to end on every platform the milestone targets
- tests pass
- no placeholder UI remains
- PROGRESS.md is updated

The owner's build prompt is kept at [`docs/spec/build-prompt.pdf`](spec/build-prompt.pdf) (revised October 2026: ratings removed, M5 extended). Where the prompt still mentions ratings (a "Rate" context-menu item), D50 wins.

Platforms per milestone: **D** = Linux desktop, **M** = Android. (Windows, macOS and iOS are out of scope.)

---

## M0 — Bootstrap (D)
- Monorepo skeleton (`core/`, `ui/`, `apps/cadenza/`, `tools/`, `docs/`), workspace lints, rustfmt, eslint, prettier.
- `cargo xtask` with tasks: `fixtures`, `test`, `bench`, `ci`.
- GitHub Actions (ubuntu): build core + UI + Tauri Linux bundle and run tests; build the Android APK.
- Fixture generator v1: WAV/FLAC/ALAC/AIFF/MP3/AAC/Opus/Vorbis/WavPack/APE/DSF/DFF across depths, rates and tags.
- **Android spike**: a minimal Tauri Android app that plays FLAC from Rust in the background with a
  media session, measured against ARCHITECTURE §11 criteria on the owner's phone. Result recorded as a decision.

## M1 — Foundation (D)
- `library`: schema v1 + migrations, WAL, writer thread + reader pool.
- `tags` read path for ID3v2.3/2.4, Vorbis, MP4, APEv2, DSF ID3; field map; multi-value; legacy-encoding detection.
- `scanner`: parallel incremental scan, audio hash, watcher, move detection, artwork extraction + thumbnail cache.
- `search`: FTS5 + advanced query parser + saved searches.
- `decode`: Symphonia (+ libopus, libwavpack, DSF/DFF→PCM) with gapless trim (LAME, iTunSMPB, Opus pre-skip).
- `engine` v1: queue, play/pause/seek/next, gapless scheduler, ring buffer, shared-mode output on Linux, ReplayGain off.
- UI v1: tokens + Default Light/Dark, sidebar, album grid (virtualised, size slider), songs list
  (virtualised, sortable columns), album page (basic), search, player bar, queue panel, context menus
  (Play Next / Later / Go to Album / Artist).
- Tests: tag read for every format, gapless sine test, MD5 decode test, scan of 200k-track synthetic
  library (budget: incremental rescan in seconds), search < 50 ms.

## M2 — Hi-res engine (D)
- Output backends: ALSA `hw:` exclusive (with PipeWire device reservation), ALSA `default` shared; device enumeration +
  hot-plug; per-device settings; output picker in the player bar.
- Automatic sample-rate switching, including the format-change boundary; DoP; DSD→PCM decimator.
- DSP graph in f64: ReplayGain/R128 (track/album/auto, clipping prevention) + scanner, 10-band GEQ,
  parametric EQ, presets, AutoEq import, per-device profiles, FIR convolution (partitioned FFT),
  crossfeed, crossfade (auto-off for gapless albums), limiter, TPDF dither, rubato resampler, multichannel downmix.
- Signal path panel + quality badges with hover spec.
- Tests: capture-backend bit-perfect equality, the real ALSA path through its `file` plugin (D30; replaces `snd-aloop`), DoP marker
  validity, no-alloc audio thread assertion, seek < 100 ms benchmark.

## M3 — Design polish (D)
- Album page (full), artist page (hero, top songs by plays, release-type sections), Home shelves (basic).
- Now Playing full-screen: artwork-derived animated gradient, artwork scale on pause.
- Synced lyrics view (embedded + `.lrc`), tap-to-seek.
- Mini player (always-on-top), tray/menu-bar controller, translucent sidebar per OS.
- Light/dark/auto, spring motion, Reduce Motion/Transparency, keyboard navigation pass.
- Desktop system media controls (MPRIS).

## M4 — Tag editing (D)
- `tags` write path with the safe-write protocol; round-trip tests for every format with unknown frames;
  kill-during-write tests.
- Song Info window (Details, Artwork, Lyrics, Options, Sorting, File).
- Batch editor: form mode (Mixed), grid mode (inline, multi-cell paste, fill-down); only changed fields written.
- Actions: find/replace (regex, scoped), case conversion + exceptions, cleanup, tags↔filename patterns,
  auto-number, split/merge/copy fields, copy tags across tracks, clear/remove, fix encodings.
- Bulk artwork tools; preview diff; background writes with progress/cancel/error report.
- Undo/redo, change history, revert batch; optional backups.
- Online lookup: MusicBrainz + Cover Art Archive, Discogs (user token), AcoustID (fingerprint);
  side-by-side diff with per-field/track/all accept.
- Action presets; file organiser (rename/move with preview, illegal chars, long paths, stats kept).

## M5 — Customisation (D)
- Theme engine (JSON ↔ tokens), theme editor with live preview + WCAG AA contrast warnings,
  import/export, 6–8 built-in themes.
- Appearance controls (accent incl. from artwork, background style, blur, radius, density, font, artwork size).
- Layout engine: sidebar reorder/pin/hide, player bar position/size/controls, dockable panels
  (queue, lyrics, info, tag editor, signal path), per-section default view incl. column browser,
  list column editing + grouping, Now Playing styles, layout presets + shortcut switching.
- Custom CSS with safe-mode start; fully remappable shortcuts + global media hotkeys.
- Owner additions (October 2026): an accent colour picker; reorderable grid, tab and list options
  (sidebar, Android tabs, list columns, grid or list per section); gradient backgrounds; and a
  "liquid glass" option, researched for and adapted to Linux (WebKitGTK) and Android (WebView).

## M6 — Sync (D)
- `sync`: HLC, op log, segment writer/reader, snapshots, apply engine (rewriting local tags safely).
- Providers through OpenDAL: S3/B2, WebDAV, Dropbox, Google Drive, OneDrive; OAuth flows; keychain.
- Blob upload/download: resumable, dedupe, rate limits, bandwidth caps; pause/resume; per-device status.
- Trash (30 days), conflicts view, relative paths, playlist sequence merge, play-event merge.
- Optional client-side encryption; LAN discovery + peer transfer; handoff + remote control.
- Storage limits + pinning + on-demand streaming cache (desktop side; mobile in M7).
- Tests: N-device randomised convergence property tests, interrupted transfer resume, trash
  recovery, encrypted round trip.

## M7 — Mobile (M)
- Shell per the spike outcome (Tauri Android or Compose over UniFFI).
- Mobile UI: tab bar (customisable), floating mini player, swipe-up Now Playing, long-press menus.
- Background playback, lock screen / Control Centre / notification controls, audio focus + interruptions,
  headset/BT/car buttons.
- USB DAC: Android 14+ bit-perfect mixer attributes; resample indicator on older versions.
- Full/partial mirror with pinning and AAC/Opus transcoded copies; Wi-Fi/charging-only sync; background sync.
- **Fix first: settings don't survive closing the app on Android** (owner report, Oct 2026; see
  PROGRESS "Known issues"). The theme and look edits go back to defaults. Reproduce on the phone,
  find the cause, and add a test that closes and reopens the app.
- Stretch: Android Auto browsing, home-screen widgets, Wear OS controls.

## M8 — Extras (D+M)
- Home shelves from own library (Recently Played/Added, Heavy Rotation, Favourites, Rediscover, On This Day, random picks).
- Smart playlists (nested rules, limits, live), Autoplay + library radio (genre/year/BPM/key/history similarity).
- Stats + yearly recap; lyrics fetch (LRCLIB) + tap-to-sync editor; credits from tags + MusicBrainz;
  sleep timer; internet radio (Icecast/SHOUTcast).
- Library health (duplicates by tags/fingerprint, artwork, album-artist consistency, missing tags,
  broken/changed files); quality check (spectrogram, lossy-source/upsampled detection).
- Converter (FLAC/ALAC/MP3/AAC/Opus, tags + art preserved).
- Import/export: iTunes/Apple Music XML, M3U/M3U8/PLS/XSPF, CSV/JSON, backup/restore.
- Scrobbling (Last.fm, ListenBrainz, offline cache); casting (Chromecast, UPnP/DLNA); visualizer.

## M9 — Hardening (D+M)
- Performance pass against budgets (60 fps lists at 200k, search < 50 ms, cold start < 2 s, rescan in seconds).
- Accessibility audit (screen readers on every OS, focus, text scaling).
- Packaging: AppImage + .deb, Android APK (local keystore); auto-update via the Tauri updater against a
  location the owner chooses (e.g. their cloud storage). No store builds, no code signing certificates.
- User documentation.
