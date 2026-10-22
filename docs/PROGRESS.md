# Progress

## Status (2026-10-22)

**M1 (Foundation) is done for Linux.** `cargo xtask ci` passes locally and in GitHub Actions.
The Android spike is still blocked; see Known issues.

### Done
- **M0 bootstrap**:
  - Monorepo.
  - Lints.
  - Tauri 2 shell.
  - Design tokens.
  - `cargo xtask`.
  - A fixture generator with 150 synthetic files and an independent self-check.
  - CI with a Linux job and an Android APK job. Both are green.
- **`cadenza-tags`**: read path for:
  - ID3v2.3/2.4
  - Vorbis comments (FLAC/Ogg/Opus)
  - MP4
  - APEv2 (WavPack/Monkey's Audio)
  - DSF/DFF ID3
  
  It also gives:
  - A unified field map, with multi-value support, `performer:role` and `custom:NAME`.
  - Legacy-encoding detection for Latin-1 tags that are really CP1251, Shift-JIS and so on.
  - Embedded pictures and cue sheets.
  - Audio properties.
- **`cadenza-decode`**:
  - Symphonia for FLAC, ALAC, WAV, AIFF, MP3, AAC and Vorbis.
  - libopus, libwavpack (int, float and DSD) and Monkey's Audio.
  - Own DSF/DFF readers and DSD→PCM conversion.
  - Gapless trimming: LAME/Xing, the MP4 edit list plus iTunSMPB, and Opus pre-skip.
  - Sample-accurate seeking.
  
  Tests check bit-exact lossless decoding against the generator's PCM, exact lossy lengths, gapless
  alignment across track boundaries and seeks.
- **`cadenza-library`**:
  - SQLite in WAL mode, with a single writer thread and a reader pool.
  - An incremental parallel scanner with an audio-region identity hash and move detection.
  - Missing files are kept and marked, not deleted.
  - A filesystem watcher (2 s debounce, plus a 30-minute verify sweep).
  - Album grouping that handles editions and disc folders.
  - An artwork cache of originals plus JPEG thumbnails, with truncated-image checks.
  - Cue sheets.
  - FTS5 search with advanced syntax (`artist:`, `year:>2000`, `format:flac`, `bits:24`, …) and
    saved searches.
  - Play counts.
- **`cadenza-engine`**:
  - A queue with play next/later, move, remove, shuffle and repeat.
  - Same-format tracks chain gaplessly.
  - A format change drains, then reopens the output.
  - Seek; a volume ramp.
  - rubato resampling and ITU downmix for the shared output.
  - ALSA `default` shared output.
  - A capture backend for tests.
- **`cadenza-api`** and the Tauri commands:
  - One façade for the library, scanner, watcher and engine.
  - Coalesced background scans.
  - Events to the UI.
  - A `cadenza://` artwork URI scheme.
  - `--add-folder <path>`.
- **UI v1**:
  - Sidebar with search.
  - Recently Added, Albums (virtualised grid, size slider, sort), Artists, Songs (virtualised,
    sortable columns, Shuffle All), album page (discs, Other Versions), search results with saved
    searches, Music Folders (add, remove, rescan, last scan report).
  - Player bar (scrubber, volume, shuffle/repeat, quality badge).
  - Queue panel.
  - Context menus (Play / Shuffle / Play Next / Play Later / Go to Album / Go to Artist).
  - Light and dark.
- **Performance** (synthetic 200k-track library, `cargo xtask bench`):

  | Operation | Result | Budget |
  |---|---|---|
  | Incremental rescan, nothing changed | 1.25 s | seconds |
  | Search, worst case | 42.7 ms | 50 ms |
  | Deep page, Songs sorted at offset 190k | 8.7 ms | — |
  | Library open | 25 ms | — |
- Demo script: `docs/demos/M1.md`.

### Next
1. **Android spike**:
   - Needs `dl.google.com`, or a debug APK from CI.
   - Also needs an AAudio output backend, a MediaSession plugin and libwavpack cross-built for the
     NDK.
2. **M2 hi-res engine**:
   - ALSA `hw:` exclusive with PipeWire device reservation.
   - Device picker.
   - Automatic rate switching, DoP.
   - DSP graph, ReplayGain.
   - Signal path panel.

## Known limitations in the M1 app
These are shown or absent in the UI, never faked:
- **Output.** The only output is the ALSA `default` device (PipeWire/PulseAudio) in shared mode,
  resampled to the server rate. There is no device picker and no bit-perfect path yet; both come
  in M2.
- **DSD.** DSD plays as PCM at 176.4 kHz, with no DoP. It is quieter by 6 dB (D24).
- **Not yet built:**
  - Tag editing (M4).
  - Ratings can't be set; ratings stored in files are shown.
  - ReplayGain and EQ (M2).
  - MPRIS media keys.
  - Playlists.
  - Settings.
  - The Now Playing screen (M3).
  
  None of these have dead buttons.
- **32-bit integer FLAC** is reported as unsupported in the scan report (D22).
- **Search** matches word prefixes, not text in the middle of words (D23).

## Known issues / blockers
- **Android SDK/NDK cannot be downloaded in this environment.** The network policy denies
  `dl.google.com`.
  - CI builds an unsigned release APK, which Android won't install. The spike needs a debug build
    or a locally signed APK.
  - The spike also needs the owner's phone to run against the criteria in ARCHITECTURE §11.
- Fixture warnings are expected: ffmpeg decodes AAC without applying the edit list's end trim
  (D18). They are warnings, not failures, because ffmpeg isn't the system under test.

## Decisions
See ARCHITECTURE.md §13. Added during M0: D14–D19. Added during M1: D20–D26.
