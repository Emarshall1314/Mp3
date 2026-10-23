# Cadenza — Architecture

Status: **approved** (2026-10-21). Working name "Cadenza" is a placeholder.

**Scope (owner decision, overrides the original spec's platform list):** personal use only, never
published or distributed. Target platforms are **Linux desktop and Android**. Windows, macOS and iOS
are out of scope; the design keeps them possible (output-backend trait, UniFFI-ready API) but no work
is planned for them.

Priorities, in order, decide every trade-off below:
**never damage files → sound quality → metadata quality → design polish → feature breadth.**

---

## 1. Overview

```
┌────────────────────────── apps/cadenza (Tauri 2 shell) ──────────────────────────┐
│  ui/ (TypeScript + React, design tokens as CSS custom properties)                │
│      ▲ commands / paged queries        │ events (state, progress, sync)          │
│      │ cadenza:// URI scheme (artwork, waveform, spectrogram images)             │
├──────┴────────────────────────────────────────────────────────────────────────── │
│  Native plugins: media controls · background audio · USB DAC · tray/mini player  │
└───────────────────────────────────────┬──────────────────────────────────────────┘
                                        │ Rust API (cadenza-api, UniFFI-ready)
┌───────────────────────────────────────┴──── core/ (Rust workspace) ──────────────┐
│ api        façade: one `Cadenza` handle, async commands, event stream            │
│ library    SQLite catalogue, scanner, watcher, search, smart playlists           │
│ tags       read/write all tag formats, lossless round-trip, encodings, artwork   │
│ decode     Symphonia + DSD reader + libopus + libwavpack (+ FFmpeg fallback)     │
│ engine     queue/player state machine, gapless scheduler, DSP graph, signal path │
│ output     ALSA · AAudio (Android) · capture/null                               │
│ sync       change log, HLC, blob store, cloud providers, LAN peer, crypto        │
│ online     MusicBrainz, Cover Art Archive, Discogs, AcoustID, LRCLIB, scrobblers │
│ fixtures   test-library generator (tools/), no copyrighted audio                 │
└──────────────────────────────────────────────────────────────────────────────────┘
```

**Playback and sync live in the core, never in the UI.** The webview can be suspended,
reloaded or crash and audio keeps playing; the UI is a view over core state.

## 2. Repository layout

```
core/                 Cargo workspace
  api/                cadenza-api: the only crate apps depend on
  library/ tags/ decode/ engine/ output/ dsp/ sync/ online/ common/
ui/                   Vite + React + TS; shared by every shell
apps/cadenza/         Tauri 2 project (Linux desktop + Android targets)
  plugins/            Tauri plugins in Rust/Swift/Kotlin (media session, audio session, USB)
tools/fixtures/       generates the test library (Rust + ffmpeg/sox CLI)
docs/                 ARCHITECTURE, ROADMAP, PROGRESS, demo scripts, user docs
```

## 3. Stack (versions checked 2026-10-21)

| Area | Choice | Version | Licence | Notes |
|---|---|---|---|---|
| Language | Rust | 1.97 | MIT/Apache | edition 2024 |
| App shell | tauri | 2.12 | MIT/Apache | desktop + mobile |
| UI | React / TypeScript / Vite | 19.3 / 7.0 / 8.3 | MIT / Apache / MIT | |
| UI state | zustand | 5.0 | MIT | thin; truth lives in core |
| Virtual lists | @tanstack/react-virtual | 3.14 | MIT | 200k rows |
| Icons | lucide-react | 1.61 | ISC | one consistent stroke set |
| Fonts | Inter (bundled), `system-ui` on Apple | 4.x | OFL-1.1 | never bundle Apple fonts |
| UI tests | vitest, Playwright | 5.0, 1.64 | MIT, Apache | |
| Database | rusqlite + bundled SQLite (WAL, FTS5) | 0.40 | MIT / public domain | |
| Decode | symphonia | 0.6 | **MPL-2.0** (file-level copyleft; fine unmodified) | FLAC, ALAC, WAV, AIFF, CAF, MP3, AAC, Vorbis, (APE — verify in M1) |
| Decode | libopus via `opus` bindings | 1.5 | BSD-3 | Opus |
| Decode | libwavpack (own thin FFI), vendored in `third_party/wavpack` and built with `cc` | 5.9.0 | BSD-3 | WavPack incl. hybrid/DSD (D27) |
| Decode | DSF/DFF reader | in-house | — | simple container formats |
| Decode fallback | FFmpeg, dynamically linked | 6.1+ | **LGPL-2.1+** | Linux + Android; also used by the converter |
| Tags | lofty | 0.25 | MIT/Apache | used at the *concrete* tag-type level (see §6) |
| Resampling | rubato | 5.0 | MIT/Apache | sinc, quality preset ~ SoX VHQ. `soxr` crate rejected: LGPL, unmaintained since 2024 |
| Loudness | ebur128 | 0.1 | MIT | R128 / ReplayGain 2 scan |
| Fingerprint | rusty-chromaprint | 0.3 | MIT | port of Chromaprint (avoids LGPL C lib) |
| Ring buffer | rtrb | 0.4 | MIT/Apache | wait-free SPSC |
| Output Linux | alsa | 0.12 | MIT/Apache | `hw:`/`plughw:`/`default` (PipeWire) |
| Output Android | AAudio (own thin FFI to `libaaudio`) + Kotlin plugin | API 26+ | — (OS) | bit-perfect mixer attributes API 34+ (D28) |
| FS watch | notify | 8.2 | CC0 | |
| Hashing | blake3 | 1.8 | CC0/Apache | audio-stream hash, blob ids |
| Async | tokio | 1.54 | MIT | not on the audio thread |
| HTTP | reqwest (rustls) | 0.13 | MIT/Apache | |
| Cloud storage | opendal | 0.59 | Apache-2.0 | S3/B2, Dropbox, GDrive, OneDrive, WebDAV behind one trait |
| Sync segments | ciborium, ruzstd | 0.2, 0.9 | Apache-2.0, MIT | CBOR ops, pure-Rust zstd (D55) |
| Property tests | proptest | 1.12 | MIT/Apache | sync convergence (dev only) |
| Sync encryption | chacha20poly1305, argon2, hmac, sha2 | 0.10, 0.5, 0.12, 0.10 | MIT/Apache | optional encryption of the sync store (D57) |
| Test servers | wsgidav, moto (Python, test only) | 4.3, 5.2 | MIT, Apache-2.0 | local WebDAV and S3 for the remote-store tests; never shipped |
| mDNS | mdns-sd | 0.21 | MIT/Apache | LAN fast path |
| Crypto | chacha20poly1305, argon2 (RustCrypto) | — | MIT/Apache | optional client-side encryption |
| Images | image, fast_image_resize | 0.25, 6.1 | MIT/Apache | thumbnails, bulk resize |
| Media controls | souvlaki (Linux MPRIS) + own Kotlin plugin (Android MediaSession) | 0.8 | MIT | |

No GPL dependencies; MPL-2.0 and dynamically linked LGPL accepted by the owner. Because the app is
never distributed, licences are a hygiene concern rather than a legal one; the no-GPL rule is kept
anyway so the option to share later stays open.
cpal is deliberately not used for output, as the spec asks: it hides `hw:` device control and format negotiation.

## 4. Data model (SQLite, one DB per device)

Key identities:
- **`audio_hash`**: BLAKE3 of the container's *audio region* (tag chunks/frames excluded, computed
  per container). Tag edits don't change it. This is a track's identity everywhere: moves, sync,
  dedupe. As built in M1 it is sampled: three 32 KiB windows plus the region length (D20).
- **`track_id`**: UUIDv7, stable for the track's lifetime; mapped 1:1 to `audio_hash` (re-mapped if a
  file is legitimately re-encoded).

```
library(id, root_uri, device_id, created)                 -- one per device
files(id, track_id, rel_path, size, mtime, inode_or_fileid, audio_hash, scan_state, last_verified)
tracks(id, audio_hash, codec, container, sample_rate, bit_depth, channels, is_float, dsd_rate,
       duration_samples, bitrate, enc_delay, enc_padding, flac_md5, cue_sheet_id, cue_index,
       start_sample, end_sample, added_at, quality_class)          -- quality_class: lossy|lossless|hires|dsd
tag_values(track_id, field, idx, value, hlc)              -- normalised, multi-value via idx
tag_raw(track_id, format, blob)                           -- verbatim copy of the native tag for diffing
albums(id, album_artist_key, title_key, release_key, release_group_mbid, ...)  -- materialised
artists(id, name, sort_name, mbid) / track_artists(track_id, artist_id, role, idx)
                                                          -- role: artist|album_artist|composer|conductor|performer|lyricist|...
artwork(hash, mime, w, h, bytes_len, source) / track_artwork(track_id, hash, picture_type, idx)
user_data(track_id, favourite, favourite_hlc, skip_on_shuffle, start_ms, end_ms, volume_db, ...)
play_events(id, track_id, device_id, at, duration_played, completed)    -- grow-only; counts are derived
playlists(id, parent_id, name, kind, rules_json, sort_key) / playlist_items(id, playlist_id, track_id, pos_key, tombstone)
edit_batches(id, at, description) / edit_changes(batch_id, track_id, field, old, new)  -- undo/redo/revert
search_fts  -- FTS5 contentless table over title/artist/album/composer/genre/..., word-prefix (D23)
sync_log_local(seq, hlc, op_blob) / sync_cursors(remote_device_id, seq) / trash(...)
```

The schema above is the target across all milestones. M1 builds the subset it needs (roots, albums,
tracks, tags, artwork, user_data, play_events, saved_searches, search_fts) in migration v1; later
milestones add tables through further migrations.

Album identity: `(normalised album artist, normalised title, release_key)` where `release_key` is the
MusicBrainz release ID if present, else the parent folder, with `CD1`/`Disc 2` folders collapsed (D21). Same-named albums therefore never merge; editions sharing a release-group MBID or normalised
title + album artist appear under "Other versions".

## 5. Audio pipeline

```
 player thread                                                             output thread
 file ─► decode ─► gapless trim ─► ReplayGain ─► crossfade mix ─► remix ─► resample ─► ring (f64) ─► DSP chain ─► volume ─► quantise ─► device
         (DSD: packed as DoP/native, or DSD→PCM)                           rubato      2 s, rtrb    preamp, EQ,   ramp      exact on the
                                                                                                    crossfeed,              integer grid,
                                                                                                    convolution,            else TPDF
                                                                                                    limiter
```

- **Threads.** The *player thread* decodes ahead (2 s) into a lock-free ring. The *output thread*
  takes from the ring and runs the DSP chain, volume and conversion to the device format. It does no
  I/O and takes no blocking locks: a new DSP chain arrives through a `try_lock` slot, and the player
  thread builds it, including loading impulse responses. Buffers are reused, but there is no
  allocator guard in tests yet (planned).
- **Why DSP sits after the ring.** EQ and other changes are heard within one 10 ms buffer instead of
  after 2 s of queued audio (D33). ReplayGain and crossfade are per track, so they run before the ring,
  where track boundaries are known.
- **f64 throughout** (D29). Integer PCM of every depth up to 32 bits is exact in f64. The output's
  quantiser converts a buffer whose samples all lie on the device's integer grid exactly: the
  bit-perfect case. Anything else gets TPDF dither.
- **Bit-perfect** means the output is exclusive, at the source's rate and channel count, in a format
  that carries the source's depth, with no ReplayGain, DSP or volume touching it. The engine computes
  this per stream (`StreamInfo::bit_perfect`), and the signal path panel shows each stage and the
  reasons when it isn't. Tests check it byte for byte through ALSA (D30).
- **Gapless.** When a decoder ends, the next entry is opened at once, while ~2 s of audio is still
  queued. Encoder delay and padding are removed (LAME/Xing, iTunSMPB, Opus pre-skip, MP4 edit lists).
  If the next track fits the open stream (same rate and channels, and on an exclusive device a depth
  the format carries), its samples follow in the same ring: sample-accurate gapless playback. If it
  doesn't, the ring drains, the device is reopened (rate switch), and playback resumes. That boundary
  is the only non-gapless case.
- **Crossfade** (optional): the next track is mixed under the end of the current one with equal-power
  curves, except within an album or across a rate or channel change (D36).
- **Seeks.** A seek flushes the ring with an epoch handshake and refills from the new position. It is
  fast in practice; the < 100 ms target has no benchmark yet.
- **DSD.** DSF/DFF are read natively. On an exclusive ALSA device, native DSD formats are used when
  the driver offers them. *DoP* (0x05/0xFA markers, 24-bit at DSD rate/16) is used only where the
  owner marked the device DoP-capable (D32). Otherwise a multistage FIR decimator converts DSD to PCM
  at 176.4 kHz (D24). DSD passed as DSD bypasses ReplayGain, DSP and volume.
- **Multichannel.** Native channel layout passes through if the device has enough channels; otherwise
  it is downmixed to stereo with ITU-R BS.775 coefficients (flagged as processing).
- **CUE.** Single-file albums become virtual tracks (`start_sample`/`end_sample`). Gapless across them is
  automatic, because the underlying file is continuous.
- **Output backends.** `OutputBackend::open(request)` takes the source's rate, channels, bit depth and
  whether it is DSD, and returns an `Output` stating what the device granted (rate, channels, format,
  exclusive). `output::devices()` lists outputs: System Default (shared) plus every ALSA playback
  device as a stable `hw:CARD=<id>,DEV=<n>` (exclusive). The list is read afresh when the output menu
  opens; there are no hot-plug notifications yet. Exclusive opens take the card's
  `org.freedesktop.ReserveDevice1` name first, so PipeWire/PulseAudio release it (D31). Per-device
  settings so far: DoP on/off and the DSP profile (D34).
- **Capture backend.** A `capture` output records exactly what would reach the device, and can act as
  an exclusive integer, DoP or native-DSD device. Bit-perfect tests use it, and on Linux they also run
  through ALSA's `file` plugin for the real ALSA path (D30).

## 6. Tags and metadata safety

- Read and write through lofty's **concrete** types (`Id3v2Tag`, `VorbisComments`, `Ilst`, `ApeTag`),
  never the lossy generic `Tag`, so unknown frames and atoms survive. Where lofty has gaps (e.g. some
  ID3 frames, DSF ID3 chunk, unusual MP4 freeform atoms), add targeted handling in `tags/` and send
  fixes upstream.
- **Field map.** A single table maps our field names (§ spec "Fields") to each format's frame, comment
  or atom name, including multi-value conventions: ID3v2.4 null-separated, ID3v2.3 `/` or `;` per
  setting, repeated Vorbis keys, repeated MP4 freeform values.
- **Legacy encodings.** Latin-1 ID3 frames are run through a detector (chardetng: Shift-JIS, CP1251,
  …). The UI shows the best guess and alternatives, and the fixer rewrites them as UTF-8.
- **Safe write protocol.** (1) Write into a temp file in the same directory. (2) Re-read and verify that
  the tags equal the intended tags and the audio hash is unchanged. (3) fsync. (4) Atomically rename
  over the original (`rename(2)`, then fsync the directory). (5) Optionally keep the old file in
  backups. Any failure leaves the original untouched; read-only or locked files are reported per file.
- **Round-trip tests** run for every format with fixture files full of unknown frames (see ROADMAP M1/M4).

## 7. Library, scanner, search

- **Scanner.** A parallel walk (rayon) skips files whose `(size, mtime, file id)` are unchanged. New or
  changed files are hashed and parsed, then written in batches of 500 inside a transaction.
  `notify` watches for changes, with a periodic verification sweep. The first scan streams results,
  so the UI is usable within seconds.
- **Move detection.** A disappearing path and an appearing path with the same `audio_hash` are treated
  as a move, so `track_id`, stats and playlists are kept.
- **Search.** FTS5 word-prefix matching over denormalised text gives as-you-type results (D23). The advanced syntax
  (`artist:"x" year:>2000 format:flac`) is parsed into an AST, then turned into SQL with
  indexed columns. Saved searches store the query string. Smart playlists compile the same way.
- **IPC to UI.** Lists are paged by `(sort_key, id)` windows; the UI never holds 200k rows of objects,
  only a virtual window plus id arrays.

## 8. Sync design (cloud as hub, no server)

Cloud layout (inside an app-scoped folder):
```
/Cadenza/library.json                     library id, format version, encryption params (salt, KDF)
/Cadenza/devices/<device-id>.json         name, platform, last seen, root alias
/Cadenza/log/<device-id>/<seq>.seg        immutable, append-by-new-file segments (zstd CBOR ops)
/Cadenza/snapshots/<device-id>-<hlc>.db   periodic compacted catalogue snapshot for fast catch-up
/Cadenza/blobs/<aa>/<audio_hash>.<ext>    audio, uploaded once, deduped by audio hash
/Cadenza/art/<aa>/<art_hash>              artwork
```
- **Ops**, each with a hybrid logical clock (HLC) and device ID:
  - `TrackAdded{audio_hash, rel_path, blob_ref, tags}`, `TrackMoved{track, rel_path}`, `TrackTrashed` / `Restored` / `Purged`
  - `SetField{track, field, values}`: last writer wins per *field*, ordered by HLC
  - `SetArtwork`, `SetFavourite`, `SetLyrics`
  - `PlayEvent{track, at}`: grow-only, so counts always add up
  - `Playlist*`: tree and sequence operations with fractional position keys and tombstones, so edits merge
  - `Settings*`: themes and layouts, with per-device overrides
- **Apply.** Every device rewrites its *own* copy's tags from ops using the safe write protocol. A tag
  edit therefore costs bytes, not the file. A device that receives `TrackAdded` downloads the blob,
  then applies all later ops.
- **Change detection.** Uses provider change feeds where they exist (Dropbox `list_folder/continue`,
  Google Drive `changes`, OneDrive `delta`) and polls otherwise (S3/B2, WebDAV).
- **Conflicts.** Concurrent edits within the same HLC causality window that touch the same field with
  different values are resolved by LWW *and* logged to Sync Conflicts for review. Move-vs-trash,
  same-path-different-audio and edit-on-trashed also go there. Nothing is silently discarded: the
  losing value is kept in the conflict record.
- **Deletes.** A deleted file goes to an in-app trash folder on every device and the cloud blob is
  marked trashed. Both are purged after 30 days.
- **Transfers.** Chunked, resumable uploads/downloads (provider sessions/multipart), rate-limit aware
  with exponential backoff, with bandwidth caps, Wi-Fi-only and charging-only options. A partial
  download is a `.part` file and is never visible as a track.
- **Partial mirrors.** Devices with a storage limit pin albums and playlists. Others stream from the
  blob store on demand, cached in an LRU. Pinned items can optionally be stored as AAC/Opus
  transcodes, which still get tag ops because the transcode is keyed to the source `audio_hash`.
- **LAN fast path.** Peers advertise over mDNS and authenticate with a key derived from the library
  secret, then fetch blobs over HTTPS (rustls) from each other first.
- **Encryption (optional).** XChaCha20-Poly1305 per blob and log segment, with a key from Argon2id
  over a passphrase, and blob names HMAC-ed. The cloud then sees only opaque data.
- **OAuth scopes are minimal:** Google Drive `drive.file` (avoids Google's restricted-scope audit),
  a Dropbox App Folder, OneDrive `Files.ReadWrite.AppFolder`. Tokens live in the OS keychain (`keyring`).
- **Handoff/remote.** Queue and position are published as an ephemeral op. LAN remote control uses
  the same authenticated peer channel.

## 9. UI architecture and design system

- Every colour, radius, spacing value, font, blur, shadow and duration is a CSS custom property in
  `ui/src/theme/tokens.css`. Themes are JSON → tokens. Layout presets are JSON describing sidebar,
  player bar and dock positions, consumed by a layout engine (resizable split panes + floating panels).
- **Platform shells:** desktop uses a sidebar layout; mobile (narrow and touch) uses a tab bar, a
  floating mini player and a swipe-up Now Playing.
- **Motion:** CSS springs via `linear()` easing (`--ease-spring`) for view, panel and menu entrances
  and button presses. Reduce Motion and Reduce Transparency are app settings that add to the
  desktop's own preferences (D40): either one sets an attribute on `<html>`, and the stylesheet keys
  off the attributes only.
- **Keyboard:** shortcuts come from one table that both handles keys and lists them in Settings.
  Long lists and grids are one Tab stop each, with arrow keys inside (D42).
- **Artwork** is served by a custom `cadenza://art/<hash>?s=<size>` protocol from the thumbnail
  cache (sizes 64/128/256/512/1024, WebP). The artwork gradient for Now Playing uses palette extraction
  in the core.

## 10. Per-platform capabilities and limits

| Capability | Linux | Android |
|---|---|---|
| Bit-perfect | ALSA `hw:` exclusive. **The device must be released by PipeWire/PulseAudio**: the output picker detects a busy device and explains how to release it (PipeWire device profile "Off", or `pw-reserve`-style D-Bus device reservation, which we request automatically). | API 34+ (Android 14) bit-perfect mixer attributes for USB DACs. Older versions or internal speakers are resampled by the OS, shown in the signal path. |
| Sample-rate switching | Reopen `hw:` at the track's rate | Via mixer attributes on API 34+ USB; otherwise the OS mixer rate |
| DSD | DoP via `hw:`; native DSD (`DSD_U32_BE`) where the driver exposes it | DoP over USB on API 34+ bit-perfect path; otherwise DSD→PCM |
| Shared mode | ALSA `default` (PipeWire), our resampler targets the server rate | AAudio shared stream |
| Media controls | MPRIS (D-Bus) | MediaSession + notification + lock screen, via a Kotlin foreground `MediaSessionService` |
| Translucent sidebar | Real window transparency needs a compositor; WebKitGTK `backdrop-filter` works in-page. Falls back to solid with Reduce Transparency or no compositor. | In-page blur |
| Mini player always-on-top / tray | Yes (tray needs a StatusNotifier host, e.g. KDE or GNOME with AppIndicator extension) | — (floating mini player in-app) |
| Custom CSS | Yes | No (spec: desktop only) |
| Global media hotkeys | MPRIS covers media keys under most desktops; X11 global shortcuts; Wayland via the GlobalShortcuts portal where available | Headset/Bluetooth via MediaSession |
| Background sync | Always | WorkManager for periodic sync, foreground service while transferring; Wi-Fi-only and charging-only options |
| Casting | Chromecast, UPnP/DLNA | Chromecast, UPnP/DLNA |

## 11. Mobile approach (Android; decision pending a spike)

**Default: Tauri 2 Android + native plugins.** Audio, decoding and sync run in Rust and don't depend on
the webview. A Kotlin `MediaSessionService` foreground service owns the media session (lock screen,
notification, headset and Bluetooth buttons, audio focus). The Rust engine outputs via AAudio, and USB
DACs use `AudioManager.setPreferredMixerAttributes` (API 34+) for bit-perfect output.

**Fallback:** a Jetpack Compose UI over the same core via UniFFI. `cadenza-api` is designed
UniFFI-ready from day one (plain data types, callback-interface events), so switching replaces only
the UI.

**Spike criteria** (all must pass on the owner's phone for Tauri to be kept):
- 30 min of background playback with the screen locked
- notification and lock-screen controls work
- Bluetooth and headset buttons work
- calls interrupt playback and it resumes afterwards
- gapless playback is maintained in the background
- USB DAC at native rate on Android 14+
- memory stays bounded with a 200k-track catalogue
- scrolling stays at 60 fps

The spike APK is built by CI (or locally once `dl.google.com` is reachable) and sideloaded.

**As built** (`apps/cadenza/plugins/android`): a local Tauri plugin carries the Kotlin half, so the
generated Android project stays generated.
- `CadenzaPlugin`: file access (D43) and the music folder picker, called from Rust commands.
- `PlaybackService`: a foreground service with a framework `MediaSession` (no AndroidX Media
  dependency) and a MediaStyle notification. It holds audio focus while playing and pauses when
  headphones are unplugged.
- `MediaBridge`: JNI into the app library for state and commands (D44).
- The test script is `docs/demos/Android.md`.

## 12. Testing strategy

- **Fixtures** (`tools/fixtures`, run by `cargo xtask fixtures`) are generated with ffmpeg/sox plus our
  own writers. They cover:
  - every format, bit depth (16/24/32i/32f), rate (44.1–384k) and DSD64–256
  - multichannel audio and CUE sheets
  - every tag type, with unknown frames, Unicode, multi-value fields and legacy encodings
  - 20 MB artwork and corrupt or truncated files
  - a synthetic library of 200k tracks for performance (generated as tiny files plus DB-only rows)
- **Required tests:**
  - tag round-trips per format, including preservation of unknown fields
  - bit-perfect: decoded PCM matches the FLAC STREAMINFO MD5, and capture output equals the source
  - gapless: a continuous sine split across tracks has no discontinuity (checked on sample deltas)
  - interrupted writes: the write is killed at every step and the file must still be intact
  - sync convergence: property tests with randomised offline edits on N simulated devices against an
    in-memory and a MinIO-style local S3 store, plus interrupted transfers and trash recovery
  - performance budgets as benchmarks with thresholds in CI
- **CI:** GitHub Actions on Ubuntu builds and tests core, UI and the Linux bundle, and builds the
  Android APK. Android device tests need the owner's phone.

## 13. Decisions log

| # | Decision | Why |
|---|---|---|
| D1 | rubato over libsoxr | MIT, maintained; quality preset matches our requirements |
| D2 | No cpal; per-OS backends | exclusive/hog/hw control, per spec |
| D3 | libopus + libwavpack + own DSD reader alongside Symphonia; FFmpeg only as fallback and for the converter | native libs are BSD and cross-compile cleanly for Android |
| D4 | rusty-chromaprint over libchromaprint | MIT vs LGPL; no C build |
| D5 | Track identity = BLAKE3 of the audio stream | survives tag edits, moves, sync |
| D6 | lofty concrete tag types only | lossless round-trip |
| D7 | OpenDAL for cloud providers | one maintained crate covers all five providers |
| D8 | Minimal OAuth scopes (app folder / `drive.file`) | avoids Google restricted-scope audit; least privilege |
| D9 | One Tauri project covers desktop + mobile | shared UI; native fallback kept cheap via UniFFI-ready API |
| D10 | ~~No native ASIO~~ | Moot: Windows out of scope (D11) |
| D11 | Platforms: Linux + Android only | Owner decision 2026-10-21 |
| D12 | Personal use; no code signing, no store builds | Owner decision. Linux ships as AppImage + .deb; Android as a sideloaded APK signed with a local keystore |
| D13 | Owner supplies cloud and API credentials (Dropbox/Google/Microsoft app registrations, AcoustID key, Discogs token) in Settings | Personal build; nothing embedded |
| D14 | TypeScript pinned to 6.0 | typescript-eslint 8.x supports TS < 6.1; revisit when it supports 7.x |
| D15 | Fixture generator written in Python with **mutagen (GPL-2.0+)** as the independent tag oracle | Test-time tool only, never linked or shipped; an oracle that shares no code with our tag crate catches mapping bugs our own writer would repeat |
| D16 | Monkey's Audio 3.99 Unix port (Monkey's Audio licence) built from source by xtask, pinned commit | Only available APE encoder; test-time only |
| D17 | FLAC embedded art is limited to one 16 MiB metadata block | Format limit, found by the fixtures. The M4 artwork tools must resize or refuse instead of failing mid-write |
| D18 | AAC gapless: honour the MP4 edit list *and* iTunSMPB | ffmpeg-made files carry only an edit list, iTunes-made ones only iTunSMPB. ffmpeg itself ignores the edit list's end trim, so Cadenza applies trimming itself (M1 gapless test) |
| D19 | ID3v2.3 multi-value text is read as one "/"-joined string, not split | Splitting would break "AC/DC"; the owner can opt in per field later |
| D20 | Identity hash samples the audio region: BLAKE3 over three 32 KiB windows (start, middle, end) plus the region length; for Ogg, page payloads only | Hashing whole files made the first scan of a large library disk-bound for hours. Sampling is still unchanged by tag edits and distinguishes real-world files; a full hash can be added for sync dedupe in M7 if needed |
| D21 | Album key = album artist + title + (MusicBrainz release id, else folder, with disc subfolders collapsed) | Date/label/barcode keys from the original design split albums whose tracks disagree slightly; the folder is what owners actually use to separate editions. Same-named albums by one artist in different folders stay separate and link as Other Versions |
| D22 | 32-bit integer FLAC is rejected as "unsupported" | Symphonia's FLAC decoder overflows on the side channel at 32 bits. Such files are rare (libFLAC 1.4+); shown in the scan report, not silently dropped |
| D23 | Search uses FTS5 word-prefix (`unicode61 remove_diacritics 2`, prefix 1–3) instead of trigram | Trigram missed the 50 ms budget at 200k tracks. Trade-off: no mid-word matches ("ight" doesn't find "Night"); prefix matching covers as-you-type. Ranking is skipped above 5,000 matches |
| D24 | DSD→PCM applies no +6 dB make-up gain; SACD 0 dB reference lands at −6 dBFS | Hot DSD masters exceed the reference level and would clip; ReplayGain (M2) can make up the level |
| D25 | Player state carries queue length and version, not the queue; the UI fetches id windows | A 200k-track "play all" made every state event megabytes |
| D26 | Empty sort keys sort last (stored as U+10FFFF) | "Unknown Artist" and untitled albums belong at the end of lists, and an index-friendly key avoids a computed ORDER BY |
| D27 | WavPack is vendored (5.9.0 library sources) and compiled by `cadenza-decode`'s build script on every target | The NDK has no libwavpack; one build path keeps Linux and Android on the same version and drops a system package. libopus already builds from bundled source the same way |
| D28 | Android minimum is API 26 (Android 8.0); AAudio is bound directly, not through the `ndk` crate | AAudio needs API 26 and its library is only in the NDK sysroot from 26. The API used is about 20 functions, so a hand-written binding avoids a dependency |
| D29 | The engine pipeline is f64; outputs quantise, exactly when samples are on the integer grid, else with TPDF dither | One path for bit-perfect and processed playback; 32-bit integer sources stay exact (f32 would not) |
| D30 | Bit-perfect output is tested through ALSA's `file` plugin, not `snd-aloop` | CI runners have no `snd-aloop` module; the `file` plugin goes through the same alsa-lib code and records the exact bytes the device would get (PCM, DoP and native DSD) |
| D31 | Exclusive ALSA output reserves the card over D-Bus (`org.freedesktop.ReserveDevice1`) with the `dbus` crate (MIT/Apache) | The protocol PipeWire, PulseAudio and JACK honour; `dbus` is already linked through Tauri's `tao` |
| D32 | Native DSD is used automatically; DoP only per device when the owner enables it | Drivers advertise native DSD only for DACs that take it, but nothing reveals DoP support, and a DAC without it plays DoP as full-scale noise. Native DSD words (U8, U16/U32 LE/BE) hold the oldest byte in the most significant position, as MPD writes them |
| D33 | The DSP chain runs on the output thread, after the 2 s ring; ReplayGain and crossfade run on the player thread | Settings changes are heard within ~10 ms; per-track gain and crossfades need track boundaries, which only the player thread knows |
| D34 | Output device, DoP, DSP profiles (one per output device), ReplayGain and crossfade live in `settings.json` in the data folder | They describe this machine's audio hardware, not the library, so they are not synced; writes are atomic (temp file + rename) |
| D35 | Loudness is measured with EBU R128 (`ebur128`, MIT) and stored in the library with the audio hash it was measured from | Tags always win; a measurement stays valid only while the audio is unchanged. Files are never written |
| D36 | Crossfade never applies within an album and needs the same rate and channel count on both sides | Albums stay gapless; mixing before the shared resampler keeps one resampler per stream. Other changes happen without a fade, and the Sound page says so |
| D37 | Lyrics: a sidecar `.lrc` beside the file wins, then embedded synced lyrics (ID3 SYLT converted to LRC), then embedded plain lyrics, parsed as LRC when they carry timestamps | A sidecar is how owners add or fix lyrics without touching files. LRC parsing handles repeated stamps, `[offset:]` and word stamps |
| D38 | MPRIS through `souvlaki` 0.8 (MIT, `dbus` backend); position is sent on state changes and seeks, not every tick | Media keys and desktop widgets on every Linux desktop; MPRIS clients interpolate position themselves |
| D39 | Artwork palettes are computed in the core from the 128 px thumbnail: up to four distinct colours, weighted towards saturated ones | Cheap (a few ms) and cached with the thumbnails; grey-heavy covers still give a usable gradient, and the UI falls back to calm defaults |
| D40 | Appearance (Auto/Light/Dark, Reduce Motion, Reduce Transparency) lives in `settings.json`; the reduce switches add to the desktop's preferences and never turn them off | Per machine, like the other display-adjacent settings. The main window and mini player apply changes from a `cadenza://appearance` event |
| D41 | Artist sections come from the `releasetype` tag (MusicBrainz primary + secondary types; live, soundtrack and compilation win). Untagged releases count as Albums, or Compilations when flagged. Top Songs need at least one play | Tagged libraries sort themselves; untagged ones still read sensibly. An empty Top Songs is hidden rather than filled with unplayed songs |
| D42 | Lists (Songs) and grids (Albums) are a single Tab stop with arrow keys inside; album cards are two stops (open, play), their title and artist links reachable by the context menu | Tabbing past a 200k-song list must not take 200k presses |
| D43 | Android reads music folders as files with All files access (`MANAGE_EXTERNAL_STORAGE`; `READ_EXTERNAL_STORAGE` before Android 11) | Media-only access hides `.lrc`, `.cue` and folder artwork, and the Storage Access Framework would need a second, URI-based file layer in the core. The Play Store restricts this permission, but Cadenza is sideloaded (D12) |
| D44 | The Android playback service talks to the core over JNI (`MediaBridge`: `waitState`, `command`), not Tauri plugin calls | Plugin calls run on the activity, which Android destroys while the service keeps playing. `waitState` blocks on a condition variable, so the service doesn't poll |
| D45 | The playback service keeps its foreground notification while paused, until playback stops (the Stop button) | Android 12+ forbids restarting a foreground service from the background, which would break resuming from the lock screen or headphones after a pause |
| D46 | Tag writes edit the format's own tag object in place and are verified on a copy (edited fields as intended, all others and the full audio region unchanged) before an atomic rename; refused rather than written when anything differs | Only the edited fields can change, and a write that would lose data (a lofty quirk, an unusual file) is caught before the original is touched. Found in testing: lofty drops v2.4-only frames from v2.3 tags and reorders Vorbis comments on removal; both are worked around and the verification would refuse any recurrence |
| D47 | Edits run as background batches recorded in the library (old and new values per field, tracks by uid); undo writes old values back only where the file still holds the new ones | One mechanism serves the editor, artwork and the organiser. Undo never overwrites a change made since, in Cadenza or elsewhere |
| D48 | Editing actions (find/replace, case, clean-up, tags from file names...) are pure functions in the UI over pending values; their results are reviewed as changes before saving | Nothing an action does reaches a file unseen, actions chain, and they are unit-tested without the core |
| D49 | The organiser renames within the song's music folder, names sanitised for FAT/exFAT and Windows, previewed first, and undoable; the library follows by its move detection | Libraries live on SD cards and shared drives as well as Linux filesystems. Keeping files inside their music folder means the scanner always sees both sides of a move |
| D50 | No ratings. Cadenza neither sets, shows, sorts nor searches by ratings; rating fields already in files are kept untouched like any other field | Owner decision (October 2026); the build prompt was revised to remove them. Favourites stay |
| D51 | Liquid glass: frosted panels with stronger saturation, bright rims and a sheen everywhere; on Android (Chromium WebView) they also refract what is behind them at their edges, with an SVG displacement filter in `backdrop-filter` | WebKitGTK drops any `backdrop-filter` chain that references an SVG filter (WebKit bug 245510, open as of 2026), so Linux can't refract without losing the blur. The displacement map is drawn on a canvas, in panel-relative units, so one filter fits every panel. Reduce Transparency turns it off |
| D52 | Themes are JSON files in `<data>/themes` (validated by the core: known shapes only, no `url()` or anything that could leave a declaration); built-in themes live in the UI. The look (theme per scheme, accent, background, panels, density, type, artwork size) is one JSON object in the appearance settings, normalised by the UI, and applied as inline custom properties on `<html>` | Themes are shareable files; a theme can never load anything from the network or break out of its property. One appearance event keeps the main window and mini player in step |
| D53 | The layout (sidebar, player bar, default views, song columns, Home shelves, Now Playing style, dock) and named presets live in the look, as JSON normalised by the UI; drags update it live and save it 300 ms after they stop | One place for every visual choice, so presets are just copies of it and the main window and mini player stay in step. The theme goes in a `:root` style sheet rather than inline styles, so custom CSS after it can override any token |
| D54 | The visualizer reads a tap on the output thread: while it is on screen, the thread stores a mono mix of each buffer it sends (after DSP and volume) into an 8192-sample ring of atomics; the UI asks for a 48-band spectrum 30 times a second, computed on the calling thread with `realfft` | The output thread never blocks, allocates or waits for the UI, and does nothing extra while the visualizer is closed. Showing what the device gets keeps the bars in step with what is heard. DSD sent as DSD has no samples to show |
| D55 | The sync core (`cadenza-sync`) is independent of the library and of any provider. Ops are keyed by audio hash (plus cue index), never paths or database ids. Each op carries the HLC of the value its device had (`seen`); a version no later op names as `seen` was lost to a concurrent edit and, if its value differs from the winner, is a conflict, so conflicts depend only on which ops have been read. Segments are CBOR (ciborium) compressed with ruzstd, a pure-Rust zstd, so the Android build needs no C zstd. The first store is a plain folder (kept in step by Syncthing, Nextcloud or a USB drive); cloud providers come behind the same three-call `Store` trait | Lets sync be built and tested end to end without cloud accounts, which the development environment can't reach; a folder is also a useful sync method in itself. Randomised 3-device tests with skewed clocks check that every device converges, including on conflicts |
| D56 | Sync in the app is driven by the merged state, not by ops as they arrive: each round applies every value whose winning version this device hasn't applied yet (tracked per song and field). So a change for a song this device doesn't have applies when the song turns up (downloaded, restored, rescanned), and a change that couldn't be applied (the tag writer busy) waits for the next round. Changes from other devices go through the normal tag writer (safe write, undo history) but are not sent back out. Listens carry an id that is the same on every device, so each is counted once. Moving files (the organiser) never syncs: where files live is each device's business. Songs whose audio is bit-identical are one song to sync | Robust to ordering, restarts and missing songs without special cases; reuses the tested write path |
| D57 | Encryption of the sync store is optional and chosen when a store is first used (a store already holding unencrypted data can't be converted in place). XChaCha20-Poly1305 with a key from Argon2id (64 MiB, 3 passes) over a passphrase; small objects sealed whole, songs in 1 MiB STREAM chunks; every object bound to its path; blob and artwork names replaced by keyed hashes. `library.json` (unencrypted) holds the salt, cost and a check value. The derived key is kept in the data folder, readable only by the owner (not the OS keychain: Linux desktops without a Secret Service and Android would need two other mechanisms for a personal app) | Pure Rust (RustCrypto), so the Android build needs nothing extra; the folder or server never sees titles, devices or song files |
| D58 | Liquid glass on the phone follows iOS 26: the tabs float in a glass capsule with a glass lens behind the current tab (it slides and stretches between tabs, and can be dragged along the bar), Search is a round button beside it, the player is a capsule above, and scrolling down shrinks the bar to the current tab with the player alongside. Pages run under the dock. Rims carry a light that turns with the phone's tilt (`deviceorientation`); pressing glass lights it from the finger and swells it. On Chromium each glass shape gets its own SVG lens filter fitted to its size and corner radius (a rounded-rectangle distance field; the rim squeezes what's behind it towards the edge, three times at the very edge; red, green and blue bend by 100/94/88 % for a colour fringe), referenced from `backdrop-filter` through `--lens` | A backdrop filter only sees what is behind the element, so the rim can't show what lies beyond the edge as real glass does; gathering from inside the bezel, without folding, gives the same bending lines. One filter per size (not one in object units) keeps the rim the same width on a capsule and a circle. View transitions morph the dock only on Android: WebKitGTK 2.52 crashes the whole app (segfault in libwebkit2gtk) when one runs over this layout. Everything is off with Reduce Transparency; motion (lens stretch, tilt, swelling) is off with Reduce Motion |
