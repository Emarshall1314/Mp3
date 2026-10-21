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
| Decode | libwavpack (own thin FFI) | 5.x | BSD-3 | WavPack incl. hybrid/DSD |
| Decode | DSF/DFF reader | in-house | — | simple container formats |
| Decode fallback | FFmpeg, dynamically linked | 6.1+ | **LGPL-2.1+** | Linux + Android; also used by the converter |
| Tags | lofty | 0.25 | MIT/Apache | used at the *concrete* tag-type level (see §6) |
| Resampling | rubato | 5.0 | MIT/Apache | sinc, quality preset ~ SoX VHQ. `soxr` crate rejected: LGPL, unmaintained since 2024 |
| Loudness | ebur128 | 0.1 | MIT | R128 / ReplayGain 2 scan |
| Fingerprint | rusty-chromaprint | 0.3 | MIT | port of Chromaprint (avoids LGPL C lib) |
| Ring buffer | rtrb | 0.4 | MIT/Apache | wait-free SPSC |
| Output Linux | alsa | 0.12 | MIT/Apache | `hw:`/`plughw:`/`default` (PipeWire) |
| Output Android | AAudio via `ndk` + Kotlin plugin | — | MIT/Apache | bit-perfect mixer attributes API 34+ |
| FS watch | notify | 8.2 | CC0 | |
| Hashing | blake3 | 1.8 | CC0/Apache | audio-stream hash, blob ids |
| Async | tokio | 1.54 | MIT | not on the audio thread |
| HTTP | reqwest (rustls) | 0.13 | MIT/Apache | |
| Cloud storage | opendal | 0.59 | Apache-2.0 | S3/B2, Dropbox, GDrive, OneDrive, WebDAV behind one trait |
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
- **`audio_hash`**: BLAKE3 of the decoded-container *audio stream bytes* (tag chunks/frames excluded,
  computed per container). Tag edits don't change it. This is a track's identity everywhere: moves,
  sync, dedupe.
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
user_data(track_id, rating, rating_hlc, favourite, favourite_hlc, skip_on_shuffle, start_ms, end_ms, volume_db, ...)
play_events(id, track_id, device_id, at, duration_played, completed)    -- grow-only; counts are derived
playlists(id, parent_id, name, kind, rules_json, sort_key) / playlist_items(id, playlist_id, track_id, pos_key, tombstone)
edit_batches(id, at, description) / edit_changes(batch_id, track_id, field, old, new)  -- undo/redo/revert
search_fts  -- FTS5 contentless table over title/artist/album/composer/genre/... with trigram tokenizer
sync_log_local(seq, hlc, op_blob) / sync_cursors(remote_device_id, seq) / trash(...)
```

Album identity: `(normalised album artist, normalised title, release_key)` where `release_key` is the
MusicBrainz release ID if present, else `(date, label+catno or barcode, total discs)`, else the parent
folder. Same-named albums therefore never merge; editions sharing a release-group MBID or normalised
title + album artist appear under "Other versions".

## 5. Audio pipeline

```
 file ─► demux/decode ─► gapless trim ─► [DSP graph, f64] ─► resample ─► dither ─► format convert ─► SPSC ring ─► output callback ─► device
        (decoder thread)                  RG/R128, EQ, FIR,     rubato     TPDF       int/float/DoP      (rtrb)        (RT thread)
                                          crossfeed, limiter
```

- **Threads.** A *decoder thread* reads, decodes and processes ahead (default 2 s, more for network/cloud
  sources) into a preallocated lock-free ring. The *output callback* only copies from the ring, applies
  a precomputed software volume if enabled and handles underrun by writing silence and flagging it.
  It does no allocation, takes no locks and does no I/O; this is checked in debug builds with an
  allocator guard (`assert_no_alloc`-style) during tests.
- **Bit-perfect.** If no DSP is active, volume is hardware or fixed, and the device is open in exclusive
  mode at the source rate and depth, samples go straight through. Every stage reports what it did, so
  the signal path panel and its verdict (bit-perfect / lossless-processed / lossy) are *computed* from
  the real pipeline, never asserted.
- **Processing.** Once any DSP is enabled, everything runs in 64-bit float, and TPDF dither is applied
  whenever the output depth is lower than the processing depth.
- **Gapless.** The scheduler pre-opens the next decoder at T−10 s. Encoder delay and padding are
  removed (LAME/Xing, iTunSMPB, Opus pre-skip, MP4 edit lists). If the next track has the same output
  format, its samples are appended to the same ring, giving sample-accurate gapless playback. If the
  format differs, a *format-change marker* goes into the ring: output drains, the device is reconfigured
  (rate switch), then playback resumes. That boundary is the only non-gapless case, and the UI says so.
- **Seeks.** A local seek flushes the ring with an epoch counter, and the decoder seeks and refills only
  ~50 ms before output resumes. Target is under 100 ms.
- **DSD.** DSF/DFF are read natively. *DoP* (0x05/0xFA markers, 24-bit at DSD rate/16) is used where the
  device is marked DoP-capable. Otherwise a multistage FIR decimator converts DSD to PCM at 176.4 kHz
  or 352.8 kHz, with −6 dB gain compensation.
- **Multichannel.** Native channel layout passes through if the device has enough channels; otherwise
  it is downmixed to stereo with ITU-R BS.775 coefficients (flagged as processing).
- **CUE.** Single-file albums become virtual tracks (`start_sample`/`end_sample`). Gapless across them is
  automatic, because the underlying file is continuous.
- **Output backends.** One trait: `enumerate()`, `capabilities(device)` (rates, formats, exclusive,
  DoP, hardware volume), `open(format, mode)`, and device-change notifications. Per-device settings:
  exclusive mode, max rate, resampling policy (never / when needed / always → rate), DSD mode
  (DoP / PCM), volume type (hardware / software / fixed 100%).
- **Capture backend.** A `capture` output writes exactly what would reach the device. Bit-perfect tests
  use it in CI; on Linux they also run through `snd-aloop` for a real ALSA path.

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
- **Search.** FTS5 trigram over denormalised text gives as-you-type matching. The advanced syntax
  (`artist:"x" year:>2000 format:flac rating:>=4`) is parsed into an AST, then turned into SQL with
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
  - `SetArtwork`, `SetRating`, `SetFavourite`, `SetLyrics`
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
- **Motion:** CSS springs via `linear()` easing. Reduce Motion and Reduce Transparency media queries
  are honoured, and both are also manual toggles.
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

## 14. Owner answers (2026-10-21)

- Recommendations approved as written (licences, OpenDAL, rubato, no ASIO).
- Personal use only; never published.
- Platforms: Linux and Android only.
- Repository: not yet provided; the work is local until a GitHub remote is available.
