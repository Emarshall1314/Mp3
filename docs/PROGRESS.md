# Progress

## Status (2026-10-23)

**M6 (Sync) works end to end on Linux**, through a shared folder or a WebDAV/S3 server:
- What syncs: tag edits, artwork, favourites, listens and the Trash.
- Concurrent edits are kept as conflicts to review.
- Optional encryption.
- Songs can be shared between devices.
- A device can carry on listening where another stopped.

Not done yet:
- Downloading only some albums, with a storage cap. The phone needs this most, so it comes with
  M7.
- LAN transfer and remote control.
- Dropbox, Google Drive and OneDrive, which need app registrations of the owner's own (see Known
  limitations).

Two kinds of test so far:
- **Real devices:** none yet. Try it with `docs/demos/M6.md`.
- **Automated:** simulated devices on one machine, through local folders and local WebDAV and S3
  servers.

**M5 (Customisation) is done for Linux**: themes, the theme editor, liquid glass, layouts and
presets, the dockable panel (with the tag editor), the column browser, grouping, the visualizer,
remappable and system-wide shortcuts, and custom CSS. The phone's own layout options are in M7. **M4 (Tag editing) is done for Linux except online lookup**, on top of M1–M3. Online lookup
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

### M6: Sync
- **M6 sync core** (`cadenza-sync`, D55):
  - Hybrid logical clocks, which order changes across devices whose clocks disagree.
  - Ops for tag fields, favourites, the trash and plays, keyed by audio hash.
  - Log segments (CBOR, zstd), one immutable file per push.
  - A folder store that ignores other sync tools' temporary files and writes atomically.
  - Merging: last writer wins per field; edits lost to a concurrent edit are kept as conflicts;
    plays only add up. A gap in another device's log waits for the missing segment.
  - Tests: randomised offline edits on three devices with skewed clocks always converge (state,
    conflicts, and what each device applied), plus restart and gap tests.
- **M6 sync in the app** (D56):
  - The **Sync** page:
    - Where to sync: a folder, or a server (WebDAV such as Nextcloud, or S3-compatible such as
      Backblaze B2).
    - This device's name, pause, and status with transfer progress.
    - Devices, conflicts to resolve (either value can win), and Continue Listening.
  - Applying changes from other devices:
    - Tag edits are written through the normal safe tag writer and appear in the undo history.
    - They are never sent back out.
    - A change for a song this device doesn't have yet applies when the song turns up.
  - Favourites: in every song's menu, and a heart in the player bar.
  - The **Trash**:
    - Files are kept 30 days, with Restore and Delete Now.
    - Trashing and restoring follow on other devices; deleting now only affects this device.
  - **Sharing songs** (optional, per device):
    - Copies this device's songs to the sync place.
    - Downloads ones other devices shared into the first music folder.
    - Downloads resume where they stopped, and a partial download is never seen as a song.
  - **Continue Listening:** pausing publishes the queue and position. Another device can carry on
    with the songs it has.
  - **Encryption** (D57), optional:
    - Chosen when a folder or server is first used.
    - The sync place sees no titles, device names or song files.
    - A device can't join without the passphrase.
  - **Servers:**
    - OpenDAL (Apache-2.0), with retries, timeouts and an optional bandwidth cap.
    - Checking a server fails in seconds rather than retrying for minutes.
    - The password is kept in an owner-only file in the data folder.
  - **Fixtures:** every library fixture song now has its own audio, as in a real library.
  - **Tests:**
    - Two and three simulated devices, covering every item above.
    - An encrypted folder checked for readable content.
    - WebDAV and S3 stores tested against local servers (wsgidav, moto) that `cargo xtask`
      starts, including encrypted and interrupted transfers.
    - A regression test for calls from an app's async runtime.
- Demo script: `docs/demos/M6.md`, with a second-device helper (`cargo run -p cadenza-api
  --example sync_device`).

### M7: Mobile (in progress)
- **Settings that survive:** a settings file that partly fails to load keeps every value that
  reads; the unreadable file is kept as `settings.json.unreadable`, and the app says so when it
  starts. Whether this fixes the Android reset needs the phone (Known issues).
- **Phone layout** (any window narrower than 700 px, so it can be tried on the desktop):
  - Tabs along the bottom. Which tabs and their order are set in Settings → Layout → Phone
    (Library is always one; at most 5).
  - **Library** page: Recently Added, Artists, Albums, Songs, Saved Searches, then the rest of
    the app (Music Folders, Sync, Trash, Sound, Settings).
  - **Search** tab with its own search box.
  - A mini player above the tabs in three styles (floating, bar, minimal). Tap it or swipe it up
    for Now Playing; drag Now Playing's handle down to close it.
  - Song lists show title over artist and time, with taller rows; a tap plays.
  - Album and artist pages stack; Now Playing fits the screen, and lyrics or the queue take it
    over when open.
  - Long-press opens a song's menu on touch screens.
  - Checked in the real app at 412×860 on a virtual display. Not checked: long-press (needs a
    touch screen) and anything on a real phone.
- **Liquid glass on the phone, after iOS 26** (D58; with Panels set to Liquid Glass):
  - The tabs float in a glass capsule; a glass lens behind the current tab slides to the next
    one, stretching like a drop as it goes, and can be dragged along the bar to pick a tab.
  - Search is a round glass button beside the bar (when Search is one of the tabs).
  - The player is a glass capsule above the bar. Scrolling down shrinks the bar to the current
    tab and brings the player in beside it; scrolling up or changing page opens it again.
  - Pages run under the glass, which is clearer than the desktop's panels; the top of the page
    blurs away under the status bar.
  - Rims catch a light that turns as the phone tilts; pressing glass lights it from the finger
    and it swells a little. The back button is a glass circle.
  - Android only: each glass shape refracts through a lens fitted to its size and corners, with a
    colour fringe. Checked in Chromium (the engine of Android's WebView) on a test page, not on
    the phone. Linux keeps the rest without the bending (D51).
  - Linux only: the morph when the bar shrinks is left out, because it crashes WebKitGTK.
  - Tests: the lens field (neutral middle, direction at edges and corners, never folds).
  - Checked in the real app at phone size: capsule, lens, search button, player, shrinking on
    scroll, press glow. Not checked: tilt and dragging the lens with a finger (touch screen).
- **Polish from the owner's first look at the phone build:**
  - Lists of choices (sort, grouping, presets, fonts…) open the app's own glass menu with a tick
    on the current choice, not Android's grey system picker. Every `<select>` is gone.
  - Album grids fill the width with even margins (2 or 3 columns on a phone, from the size
    slider; the desktop's columns grow to fit too); the artist page's grid likewise. Shelves
    snap card by card.
  - One rounded look on the phone: capsule controls, rounder cards, inputs and rows; switches
    are capsules with liquid glass on the desktop too.
  - Now Playing on a phone: the title and progress bar run the width of the screen; play/pause
    in a large glass button (with its own lens on Android); lyrics and queue as glass buttons
    under the controls, opening in a rounded glass card; the cover shrinks on short screens so
    nothing is cut off.
  - The artwork-tinted background and Now Playing's colours fade over 1.8 s when the song
    changes (registered colour properties), and no longer flash grey while the new colours
    load.
  - Smoother scrolling: one refraction pass instead of three (the colour fringe is gone), a
    plain fade rather than a blur under the status bar, and artwork requested in a few fixed
    sizes so thumbnails are made once and reused rather than for every card width.
  - The glass behind the player and tabs is frostier, so the page behind doesn't fight the
    text on it.
  - Tests: artwork sizes snap to the cached sizes; calls wait for the core to open.
  - Checked in the real app at phone size. Smoothness needs the phone.
- **Motion and swipes (owner's second look):**
  - No flash of the default artwork between songs: the previous song stays until the next
    song's details arrive, and a picture stays until its replacement has loaded (then fades
    in). A failed picture is tried once more before the placeholder shows.
  - The dock's shrink on scroll is real elements moving (the capsule narrows to a circle, the
    tab row slides so the current icon sits in it, the player glides alongside), so it is smooth
    and the glass stays live; it now animates on Linux too. View transitions are gone.
  - The tab lens reaches to the new tab and then lets go of the old one, always inside the bar;
    dragging it swells it only within the bar.
  - More give: every segmented control has a sliding, stretching highlight; buttons, cards and
    rows squish slightly under a finger and spring back; the mini player stretches as it's
    pulled up; Now Playing follows the finger when dragged down.
  - **Swipe right to go back** on any page (the page follows the finger). In Now Playing,
    swiping right goes to where the music was started from (the album or page), with the
    history it had then, so swiping again goes back to the grid or list it was chosen from.
  - Pictures and links aren't natively draggable any more (a drag swallowed the rest of a swipe).
  - Search results: the albums row is no longer squashed.
  - Tests: swipe direction and completion rules; where music came from.
  - Checked in the real app with mouse drags (the same pointer events as touch).
- **Rediscover on Home** (D59; the owner chose this from the recommendation options):
  - Album of the Day: an album not played for three months, or never (those favoured); Play
    from the card.
  - Mixes, new each day: Forgotten Favourites, Deep Cuts, On This Day, and Your Mornings /
    Afternoons / Evenings / Late Nights. Each has a cover (four artworks under a colour wash,
    with its title) and a page with Play, Shuffle and its songs.
  - Still Unplayed · this week: albums added a while ago and never played, a new set each week.
  - Everything comes from this device's library and listening; nothing is sent anywhere. Parts
    with too little to show are left out, and a new library says how the shelf fills up.
  - A shelf of its own in Settings → Layout → Home; added at the top of existing layouts once.
  - Tests: weighted, seeded picks; each mix's rules; dates and the owner's time zone; a real
    scanned library over time; the shelf migration.
  - Checked in the real app at phone size (the test library only has recent listening, so it
    showed the time-of-day mix).
- **Android's back button and gesture** now step back through the app: a menu or Song Info
  closes first, then Now Playing, then the page before, then Home; from Home the app goes to
  the background (Android's `moveTaskToBack`, a new command in the Android plugin) with the
  music still playing, instead of closing. Tests: the order of what back does. Needs the
  phone; the Kotlin half builds in CI only.
- **Owner's third round:**
  - **Seeking fixed:** the position and volume bars are the app's own slider (pointer capture,
    so a drag keeps following off the bar; the position holds where it was let go until
    playback catches up, instead of snapping back). The web view's range input lost the release
    on Android touch. Held, the bar thickens (no thumb in Now Playing on a phone, as on iOS).
  - **Group by works on the phone:** grouped songs get a heading above each group (the desktop
    keeps its group column); it was forced off on phones before.
  - **Grid only** for Albums, Recently Added and Artists: the list and column views are gone
    (the core's column browser query stays, unused by the UI).
  - **Card size:** a small − / + capsule with dots instead of the big slider. A phone steps
    through 4, 3, 2 or 1 columns; the desktop through six card sizes. On a phone, pinching the
    grid changes it too; the grid settles into the new size.
  - **Dock:** the player rises before the tabs widen and narrows before it drops, so it never
    crosses Search; its up-and-down move is a transform, and while the dock rearranges its glass
    is plain blur (the refraction redrawn every frame was what made it stutter).
  - **Swipes:** the page before is kept alive under the current one (a page stack), so swiping
    back reveals it already there, sliding in from the left and brightening, with the moving
    page casting a shadow. Swiping right in Now Playing puts the album underneath at once, so
    it's there as Now Playing slides off. On a tab's top page, swiping right brings Now Playing
    in from the left under the finger, closing the loop Now Playing → album → albums → Now
    Playing.
  - **Artists A–Z:** a slim index down the right edge (libraries with more than 12 artists). It
    opens into a glass strip when touched or hovered, letters near the finger swell like the
    macOS dock, and the letter let go on narrows the grid to those artists ("The Beatles" is
    under T and B; digits under #); a chip clears it. An artist's page shows everything.
  - Tests: artist letters and the swell; layout without views. The slider has no unit test;
    it was checked by hand in the app.
  - Checked in the real app at phone and desktop size, with mouse drags. Touch-only parts
    (pinch, the index under a thumb) need the phone.
- **Playlists** (D61; my pick after round three):
  - Make, rename and delete playlists (deleting has Undo); add songs or whole albums from any
    song or album menu (Add to Playlist…, then a playlist or New Playlist…); save a Rediscover
    mix as a playlist.
  - A playlist's page: its cover (four artworks), Play, Shuffle, and its songs; tap a song to
    play from it, ✕ to remove it, drag the grip to move it (arrow keys on the grip too).
  - Playlists in the Library page, the desktop sidebar (added once to saved layouts) and as a
    phone tab option. Songs' icon is now a note, Playlists' the list.
  - A small dialog for names (the first text prompt in the app), glass with liquid glass on.
  - Not yet: syncing playlists between devices, smart playlists, importing M3U.
  - Tests: position keys (between, before, after, a thousand appends, no trailing zero); a real
    library through create, add (with duplicates), move up and down, remove one duplicate,
    rename, empty name, delete; the sidebar migration.
  - Checked in the real app at phone size: made one from an album's menu, added another album,
    reordered by dragging.
- Demo script: `docs/demos/M7.md`.

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
3. **M7: Mobile** (in progress): see the M7 section of `docs/ROADMAP.md`. The Android settings
   reset is fixed on the phone (owner's build, October 2026).
   - **Home recommendations:** Rediscover is built (option 3). Possible later: daily genre/era
     mixes, "because you played" from listening together, an opt-in online source for music the
     owner doesn't have yet.
4. **Revisit later: sync on real devices.** The owner can't test sync yet. When possible, run
   `docs/demos/M6.md` with two real devices (a Syncthing folder or Nextcloud), and fix whatever it
   finds before more sync features are built.
5. **The rest of M6, after that test**:
   - LAN transfer and remote control.
   - Syncing edits made in other tagging programs.
   - Partial mirrors (pin albums, a storage cap, streaming from the sync place) come with M7.

## Known limitations
These are shown or absent in the UI, never faked:
- **Sync**:
  - Only edits made in Cadenza sync. Changes made by other taggers are picked up by each device's
    scan but not sent to the others.
  - Bit-identical copies of a song are one song to sync.
  - Moving files (the organiser) never syncs: where files live is each device's business.
  - Changing where a device syncs starts it afresh there. Its earlier listens stay where they
    were; its favourites are shared again.
  - WebDAV uploads each song in one request, so a song is read into memory whole. That's fine for
    normal files but heavy for multi-gigabyte DSD.
  - **Dropbox, Google Drive and OneDrive:** not offered. Each needs an app registered with that
    company (a client ID, and for Google a verification review), which only the owner can do, and
    the development environment can't reach them to test. Nextcloud or any WebDAV server, and
    S3-compatible storage (Backblaze B2, Wasabi, AWS), work now.
  - Playlists sync when playlists exist (a later milestone).
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
  - Syncing playlists (playlists themselves are built, M7).
  - Android's USB bit-perfect path (API 34 mixer attributes).
  - An allocator guard on the output thread.
  - A seek-latency benchmark.

  None of these have dead buttons.
- **32-bit integer FLAC** is reported as unsupported in the scan report (D22).
- **Search** matches word prefixes, not text in the middle of words (D23).

## Known issues / blockers
- **Android: the theme and look edits reset to defaults after closing the app** (owner report,
  October 2026). **Cause found; fix waiting to be confirmed on the phone.**
  - The startup message the M7 build added showed it: "state not managed for field `app` on
    command `settings`". On Android the page starts while the app is still opening the library,
    so the first calls (the saved look, Home's shelves) failed and the default look was shown.
    The saved look was never lost: nothing saves the look until it is changed. Linux opens the
    window only after the library is open, so it never happened there. Tag edits are read later,
    which is why they survived. My earlier note that Android starts exactly as Linux was wrong.
  - **Fix:** a `ready` command, and every call from the page waits until the core is open
    (`whenOpen` in `ui/src/api.ts`; gives up after 30 s). Tested with a core that opens after the
    page starts. Not reproducible on Linux, so the phone has the final word.
  - Also done earlier in M7: a settings file that partly fails to load keeps every value that
    reads, and the unreadable file is kept as `settings.json.unreadable`.
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
