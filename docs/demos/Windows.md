# Windows demo

The first Windows build. Nothing in development runs Windows, so this is its first real test.

## Install

1. On GitHub, open the latest successful **CI** run on `main`, and download the
   `cadenza-windows` artifact (a zip holding `Cadenza_0.1.0_x64-setup.exe`).
2. Run the installer. It isn't signed, so SmartScreen says "Windows protected your PC": choose
   **More info → Run anyway**.

## Try

1. Music Folders: add your music folder; the scan runs as on Linux.
2. Play an album. Sound → Output shows **System Default** (shared, through the Windows mixer)
   and each playback device by name.
3. Choose a device by name: exclusive mode. The signal path should show `WASAPI … exclusive`,
   the source's rate and its bit depth (e.g. `44100 Hz · 2 ch · 16-bit`). Other apps go silent
   while Cadenza has the device; that's expected. Choose System Default to share it again.
4. Media keys on the keyboard, and the media overlay that appears with the volume keys, should
   show the song and control it.
5. Settings → Keyboard → System-Wide Shortcuts: set one, switch to another app, press it.
6. Appearance → Liquid glass: the panels' edges should bend what's behind them (as on Android).
7. The tray icon (bottom right, possibly under the ^ arrow) shows the song and plays or pauses.

## What to report

- Anything in exclusive mode: a device that won't open, crackles, wrong speed or pitch.
- Gaps or clicks between songs, or stutter under load.
- Whether media keys and the overlay work.
