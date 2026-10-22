# Progress

## Status (2026-10-22)

**M5 (Customisation) is done for Linux**: themes, the theme editor, liquid glass, layouts and
presets, the dockable panel (with the tag editor), the column browser, grouping, the visualizer,
remappable and system-wide shortcuts, and custom CSS. The phone's own layout options wait for M7. **M4 (Tag editing) is done for Linux except online lookup**, on top of M1–M3. Online lookup
(MusicBrainz, Cover Art Archive, Discogs, AcoustID) waits for network access to those services
from the development environment, so it can be built against the real services rather than
guessed responses. `cargo xtask ci` passes. M3 was checked in the real app on a virtual display; the tray icon and
media keys need a real desktop session to see, so check them on the owner's machine with
`docs/demos/M3.md`. None of the M2 output work has run on real audio hardware yet (see
`docs/demos/M2.md`). The Android spike is built and waiting for the owner's phone: see
`docs/demos/Android.md`.

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

### M2: Hi-res engine
- **f64 pipeline** (D29). Outputs quantise samples that lie on the integer grid exactly (bit-perfect),
  and TPDF-dither everything else.
- **Outputs**:
  - System Default (shared, float, through PipeWire/PulseAudio).
  - Every ALSA playback device as exclusive `hw:CARD=<id>,DEV=<n>`. The source rate is followed, ALSA
    resampling is off, and the integer format carries the source's depth (16/24/32-bit).
  - Exclusive opens reserve the card over D-Bus so the sound server lets go (D31).
  - Outputs can be switched while playing; playback continues at the same position.
  - The output menu is in the player bar; the choice is saved per machine (D34).
- **DSD out**:
  - Native DSD (DSD_U8, U16/U32 LE/BE) when the driver offers it.
  - DoP v1.1 when enabled for the device (D32).
  - Otherwise DSD→PCM conversion, remembered per output so DSD albums stay gapless.
- **DSP chain** on the output thread, heard at once (D33):
  - Preamp.
  - Graphic (10-band) or parametric EQ: RBJ biquads, presets, AutoEq `ParametricEQ.txt` import, and a
    response curve computed by the core.
  - Crossfeed: three strengths, bs2b-style.
  - Convolution: partitioned FFT, impulse responses up to 10 s, resampled to the output rate.
  - Limiter: −0.3 dBFS ceiling, 1.5 ms lookahead.
  - One DSP profile per output device.
- **ReplayGain**:
  - Read from tags in every container; Opus R128 tags are converted.
  - Tracks without tags are measured with EBU R128 in the background, album by album (D35).
  - Track, Album and Auto modes, with a preamp, a level for untagged tracks, and clipping prevention.
- **Crossfade**: 0–12 s with equal-power curves. Never within an album, nor across a rate or channel
  change (D36).
- **Signal path panel**: click the quality badge. It shows each stage from file to device, a
  Bit-perfect badge, or the reasons it isn't.
- **Tests**:
  - Bit-perfect at 16/24/32-bit through the capture backend and through ALSA's `file` plugin.
  - DoP markers and payload; native DSD word order; volume never touching DSD.
  - Gapless DSD on a non-DSD output; output switching mid-track.
  - EQ, crossfeed and limiter responses; convolution against direct convolution and with a real IR
    file.
  - ReplayGain modes, Auto's album detection and clipping prevention; R128 measurement end to end.
  - Crossfade sample-exact; WavPack's lossless flag.
- **Fixes found along the way**:
  - WavPack files were reported as lossy (wrong `MODE_LOSSLESS` constant).
  - The resampler's final flush only counted the last of several chained tracks.
- Demo script: `docs/demos/M2.md`.

### M3: Design polish
- **Home** (the start page): Recently Played, Recently Added and Most Played shelves. Shelves with
  nothing in them are hidden.
- **Artist page**: artwork header, Play/Shuffle, Top Songs by play count, releases grouped as
  Albums, EPs, Singles, Live, Compilations and Soundtracks (from the release-type tag, D41), and
  Appears On. Album pages gain a "More by" shelf.
- **Now Playing**: full screen, opened from the player bar artwork.
  - Large artwork over a slowly drifting gradient made from the artwork's colours (D39).
  - The artwork springs smaller while paused.
  - A lyrics or queue panel beside it.
- **Lyrics** from a `.lrc` beside the file or embedded tags (D37). Synced lyrics follow playback
  with the current line centred, and clicking a line seeks to it.
- **Desktop integration**:
  - MPRIS: media keys and the desktop's media widget show and control playback (D38).
  - A tray icon with the current track and transport.
  - An always-on-top mini player window.
- **Appearance** (Settings): Auto/Light/Dark theme, Reduce Motion and Reduce Transparency (D40).
  The translucent sidebar, player bar, queue and menus turn solid with Reduce Transparency.
- **Motion**: spring curves for view, panel and menu entrances and button presses. Reduce Motion
  makes them instant and stops the Now Playing background.
- **Keyboard**:
  - Shortcuts for playback, seeking, volume, search, navigation, lyrics, queue, mini player and
    Settings, listed in Settings.
  - Arrow keys in the Songs list, album grids and menus; one Tab stop per list (D42).
  - Shift+F10 or the Menu key opens the focused item's actions.
  - Now Playing keeps focus inside until closed. Menus and popovers return focus when closed.
- **Fixes found along the way**:
  - Changes made while the app was closed were only noticed by the 30-minute sweep; the library
    now rescans on start.
  - The artist list stretched to half the window.
  - With a forced dark theme, drop-down menus drew light on light.
  - The mini player button showed on Android, where there is no mini player.
- **Tests**: LRC parsing, lyric sources, palettes, artist pages and Home queries (library),
  appearance saved across restarts (api), keyboard shortcuts (UI).
- Demo script: `docs/demos/M3.md`.

### Android spike
- **Built by CI, not yet run on a phone.** This environment can't download the Android SDK, so the
  Kotlin half is compiled only by CI; the Rust half is also type-checked here.
- **File access and the music folder** (D43): adding a folder asks for All files access, then
  opens the system folder picker. Files picked for Sound (impulse responses, AutoEq) are mapped
  from Android's `content://` links to paths on the phone.
- **Playback service** (D44, D45):
  - Playback continues with the screen off.
  - Lock screen, notification, Bluetooth and headset controls.
  - Calls pause playback and it resumes afterwards; another app playing pauses it.
  - Unplugging headphones pauses.
- **Signed builds**: CI signs with the owner's key (`ANDROID_KEYSTORE` secrets set 2026-10-22), so
  new builds install over old ones.
- The mini player and tray are desktop only, and their buttons don't show on Android.
- Phone test: `docs/demos/Android.md`.

### M4: Tag editing
- **Safe writing** (`cadenza-tags`, D46): every tag format Cadenza reads can be written: ID3v2.3/2.4
  (MP3, WAV, AIFF, and the ID3 tag inside DSF/DFF), Vorbis comments (FLAC, Ogg, Opus), MP4 atoms
  and APEv2.
  - Only the edited fields change; frames, comments and atoms Cadenza doesn't know survive.
  - The edited copy must read back as intended, with every other field and the audio (a full
    BLAKE3 of the audio region) unchanged, before it is fsynced and renamed over the original.
  - Read-only files, unwritable fields and bad values are refused up front.
- **Edits through the library** (D47): background batches with progress, Stop, per-file errors, an
  incremental scan afterwards (same track ids), and history with undo that leaves alone anything
  changed since. Optional backups of each file before it changes.
- **Song Info** (Get Info…, Ctrl+I for the playing song): Details, Credits, Sorting, Lyrics,
  Artwork, All Tags (every field, custom ones included), and File.
  - Several songs at once: differing fields show Mixed and stay untouched unless changed.
  - Table: per-song cells with keyboard movement, paste from a spreadsheet, Fill Down, Number
    Tracks.
  - Actions (D48): find and replace (text or regular expressions), case (title case with
    exceptions), clean-up, tags from file names, copy, split and join, clear, fix legacy
    encodings.
  - Changes: every pending change, old to new, with Revert, before anything is written.
- **Artwork**: add, retype, describe and remove pictures; one cover for many songs; save the cover
  to the folder. Undo restores old artwork from the artwork cache.
- **File organiser** (D49): rename and move files from a tag pattern, previewed first; names safe
  for Windows and Android SD cards; play counts follow the files; undoable.
- **Fixes found along the way**:
  - An MP3 with a date frame that isn't an ID3 timestamp ("Spring 1999") couldn't be read at all.
  - lofty drops ID3v2.4-only frames from v2.3 tags; those fields go to TXXX frames there.
  - lofty's Vorbis comment removal reordered other fields' values.
- **Tests**:
  - All 13 tag formats round-trip an edit and its undo.
  - Every field at once in every format.
  - Unknown data survives; pictures replace and clear.
  - Refused edits leave files byte-identical.
  - A process killed at each stage of a write leaves the old or the new file.
  - API: edits, conflicts on undo, artwork, the organiser with play counts.
  - UI: every action.
- Demo script: `docs/demos/M4.md`.

### M5: Customisation
The owner's revised build prompt is in `docs/spec/build-prompt.pdf`: no ratings anywhere (D50),
and M5 adds an accent colour picker, reorderable grid, tab and list options, gradients and liquid
glass.
- **Themes** (D52): eight built-in themes (Default Light and Dark, OLED Black, High Contrast,
  Paper, Midnight, Forest, Sunset), all meeting WCAG AA. A theme for light and one for dark.
  - Theme editor: every token, the whole app previewing as it changes, contrast warnings, and
    colours that aren't colours refused.
  - The owner's themes are JSON files the core checks (no `url()`, nothing that could escape its
    declaration). Import and export; on Android exports go to the Download folder.
- **Look**: accent colour (theme, ten presets, any colour, or from the playing artwork, kept
  readable); background solid, gradient or tinted by the artwork; panels frosted, liquid or solid,
  with blur and transparency; corner rounding; density (song rows follow it); font (theme, system,
  serif, rounded, monospace or any installed font); text size; artwork size (album pages and Now
  Playing).
- **Liquid glass** (D51): rims, sheen and stronger saturation on Linux; on Android the panels also
  bend what's behind them at their edges. WebKitGTK can't draw SVG filters in `backdrop-filter`
  (WebKit bug 245510), which is why Linux has no bending. The bending was checked in Chromium,
  the engine of Android's WebView, not yet on the phone.
- **Layout** (D53): sidebar order, hidden items, width (drag its edge) and icons-only; player bar
  at the top or bottom, three sizes, chosen controls in a chosen order; grid or list for Albums,
  Recently Added and Artists; song list columns shown, hidden, reordered and resized (drag
  headings and edges); Home shelves chosen and ordered; Now Playing styles (artwork, lyrics
  first, minimal).
- **Dockable panel**: Queue, Lyrics, Info (credits and file) and Signal Path in one panel docked
  right, left or at the bottom, resizable, or floating, moved by its header. Song Info can be
  docked there too (its Tags tab), so tags can be edited beside the song list.
- **Column browser**: a third Albums view: genres, album artists, albums, and the album's songs.
- **Grouping**: the song list grouped by album, artist or genre. Sorting by album now keeps
  same-titled albums by different artists apart (migration 4).
- **Visualizer** (D54): a Now Playing style with a 48-band spectrum of what the device is
  playing, read from a lock-free tap on the output thread only while it's on screen. Off while
  Reduce Motion is on, and blank for DSD sent as DSD.
- **Layout presets**: Listening, Tagging and Minimal, plus saved ones, from the toolbar's layout
  button or Ctrl+Shift+L.
- **Shortcuts**: every shortcut can be remapped; a combination used elsewhere moves to the new
  action. New optional actions: shuffle, repeat, Now Playing, info and signal path panels.
- **System-wide shortcuts** (Linux, X11): playback keys that work while another app has focus,
  through `tauri-plugin-global-shortcut` (MIT/Apache). None are set at first. Wayland doesn't let
  apps take keys, which Settings says, pointing to `playerctl` in the desktop's keyboard settings.
- **Fixed**: theme Export couldn't open its save dialog (the permission was missing).
- **Custom CSS** (desktop): applied after the theme, so it can override any token or style.
  `cadenza --safe-mode` starts without it, and Ctrl+Alt+Shift+S turns it off at once. Images
  and fonts can't come from the web (the page's content security policy).
- **Removed**: the leftover rating display, sort and search (D50).
- **Tests**: theme engine (colours, contrast of every built-in theme, look normalisation, accent
  from artwork), theme file validation and storage (core), layout normalisation and presets,
  remapped shortcuts.
- Demo script: `docs/demos/M5.md`.

### Next
1. **Owner tests**:
   - `docs/demos/Android.md` on the phone (decides whether Tauri stays the Android approach).
   - `docs/demos/M2.md` against a USB DAC.
   - `docs/demos/M3.md` on the desktop (tray, media keys).
   - `docs/demos/M4.md` on a copy of some music.
   - `docs/demos/M5.md` on the desktop, and liquid glass on the phone.
2. **Online lookup** (MusicBrainz, Cover Art Archive, Discogs, AcoustID) once the development
   environment may reach musicbrainz.org, coverartarchive.org (and archive.org), api.discogs.com
   and api.acoustid.org.
3. **M6: Sync**.
4. **M7: the phone layout**, including reordering its tabs and the mini-player style (they will
   use the same layout settings as the sidebar).

## Known limitations
These are shown or absent in the UI, never faked:
- **Untested on hardware.** Exclusive output, device reservation, DoP and native DSD have only run
  against ALSA's `null` and `file` devices (see Status).
- **Untested on a real desktop.** MPRIS was checked over D-Bus (`busctl`), not with media keys. The
  tray icon needs a StatusNotifier host (KDE, or GNOME with the AppIndicator extension); desktops
  without one show no icon, and the mini player and player bar still work.
- **Mini player** opens at the window manager's chosen spot each time; its position isn't saved.
- **Lyrics**: plain lyrics can be edited in Song Info; synced lyrics belong in a .lrc file beside
  the song (they aren't written into tags), and there is no online lookup.
- **Tag writing**:
  - ID3v2.3 has no place for a full original date (only its year, TORY) or for several values in
    one field (they are joined with "/", as other taggers write them).
  - A date frame another tagger wrote in a form Cadenza can't read is dropped when that file is
    edited.
  - Songs from a cue sheet (one file, many songs) are edited in the .cue file, not by Cadenza.
  - Undoing an artwork change needs the old images in Cadenza's artwork cache (they are put
    there when Song Info opens or a picture is changed).
- **Online lookup** (MusicBrainz, Discogs, AcoustID) isn't built yet (see Next).
- **Blur** behind glass panels needs WebKit's compositing; it wasn't visible on the test
  display, so check it on the desktop (`docs/demos/M5.md`). Panels stay readable without it.
- **Custom CSS** isn't offered on Android (the build prompt makes it a desktop feature).
- **Android**:
  - USB DACs play through Android's mixer; the bit-perfect USB path (API 34) isn't built.
  - If playback starts while Cadenza is in the background, Android 12+ may refuse the playback
    service; playing continues, without the screen-off guarantee.
  - Folders on cloud drives can't be added (they have no file path).
- **Device list.** It is read when the output menu opens; plugging in a DAC while the menu is open
  doesn't update it. A device that disappears mid-playback stops playback with an error.
- **Volume** is software only. Below 100% it ends bit-perfect playback, which the signal path says.
  Hardware mixer volume is not used yet.
- **Native DSD** is used whenever ALSA offers it. Its word layout (oldest byte most significant)
  follows MPD and is unverified on a real DAC.
- **Crossfade** skips tracks whose rate or channel count differ from the current one (D36).
- **EQ presets** are built in only; saving your own isn't there yet.
- **DSD** converted to PCM is 6 dB quieter than the DSD reference level (D24).
- **Not yet built:**
  - Online tag lookup.
  - Saved action presets (the case exceptions and the organiser pattern are remembered).
  - Favourites (the library has room for them; nothing sets them).
  - Playlists.
  - Android's USB bit-perfect path (API 34 mixer attributes).
  - An allocator guard on the output thread.
  - A seek-latency benchmark.

  None of these have dead buttons.
- **32-bit integer FLAC** is reported as unsupported in the scan report (D22).
- **Search** matches word prefixes, not text in the middle of words (D23).

## Known issues / blockers
- **Android SDK/NDK cannot be downloaded in this environment.** The network policy denies
  `dl.google.com`, so Android builds happen in CI only.
  - CI signs the APK with the `ANDROID_KEYSTORE` / `ANDROID_KEYSTORE_PASSWORD` repository
    secrets (now set). Without them it would use a throwaway key.
  - The spike needs the owner's phone to run against the criteria in ARCHITECTURE §11.
- Fixture warnings are expected: ffmpeg decodes AAC without applying the edit list's end trim
  (D18). They are warnings, not failures, because ffmpeg isn't the system under test.

## Decisions
See ARCHITECTURE.md §13. Added during M0: D14–D19. Added during M1: D20–D28. Added during M2:
D29–D36. Added during M3: D37–D42. Added for Android: D43–D45. Added during M4: D46–D49. Added during M5: D50–D54.
