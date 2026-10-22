# Android test script

The phone test for the Android spike (ARCHITECTURE §11). Every item must pass for Tauri to stay the
Android approach. Takes about 45 minutes, mostly waiting.

## Install
1. On GitHub, open the latest green run of **CI** (Actions tab) and download the
   `cadenza-android` artifact. Unzip it to get `cadenza-arm64.apk`.
2. Copy it to the phone and open it. Allow installing from that app if Android asks.
   - If a build signed with the throwaway key is installed, uninstall it first. From now on,
     builds signed with your key install over each other and keep the library.
3. Copy some music to the phone, ideally:
   - an album you know is gapless (a live album or a DJ mix);
   - some FLAC, including 24-bit/96 kHz;
   - a song with an `.lrc` file beside it.

## 1. First run
1. Open Cadenza and tap **Add Music Folder…**. Android 13+ asks about notifications first: allow.
2. Android opens **All files access** for Cadenza. Turn it on, then go back.
3. The system folder picker opens. Choose the music folder and tap **Use this folder**, then
   **Allow**. Expected: the library fills in, with artwork.
   - Choosing a cloud folder (Drive and so on) says it isn't on the phone's storage.

## 2. Background playback (criteria 1 and 5)
1. Play the gapless album from the start, then lock the phone.
2. Leave it for 30 minutes (plug it in if the battery is low). Expected:
   - Playback never stops.
   - No gap between the album's tracks.

## 3. Lock screen and notification (criterion 2)
1. While locked, the lock screen shows the song, artist, artwork and a progress bar.
   - Pause, play, next and previous all work.
   - Dragging the progress bar seeks.
2. Unlock and pull down the notification shade: the same controls, and Stop.
   - Stop ends playback and the notification goes away.
   - Tapping the notification opens Cadenza.

## 4. Headphones and Bluetooth (criterion 3)
1. Wired headset: one press pauses or plays. Two presses skip, if your headset supports it.
2. Bluetooth headphones or a car: play/pause, next and previous work.
3. Unplug wired headphones, or turn Bluetooth headphones off, while playing: playback pauses
   instead of moving to the speaker.

## 5. Interruptions (criterion 4)
1. While playing, get someone to call you. Playback pauses while it rings and during the call.
2. Hang up: playback resumes by itself.
3. Play something in another app, such as a YouTube video. Cadenza pauses and stays paused.

## 6. Size and smoothness (criteria 7 and 8)
1. With your whole library added, scroll quickly through Songs and Albums. Scrolling should be
   smooth, with no stutter or blank stretches longer than a moment.
2. In Android's Settings → Apps → Cadenza → Memory (or Developer options → Running services),
   memory should stay roughly level while you scroll, not keep climbing.

## 7. USB DAC (criterion 6), Android 14+
Not built yet: Android's bit-perfect USB path (mixer attributes) comes later. With a USB DAC
plugged in, Cadenza plays through it via Android's mixer (shared, possibly resampled).

## Report back
For each section: pass, or what happened. If Cadenza closes unexpectedly, the time it happened
helps. A bug report (Developer options → Take bug report) captures the log.
