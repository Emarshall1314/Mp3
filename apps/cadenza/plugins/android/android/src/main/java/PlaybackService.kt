package app.cadenza.android

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.drawable.Icon
import android.media.MediaMetadata
import android.media.session.MediaSession
import android.media.session.PlaybackState
import android.os.Build
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import android.os.SystemClock
import android.util.Log
import org.json.JSONObject
import kotlin.concurrent.thread

/**
 * Keeps Cadenza playing with the screen off and shows its media controls: a foreground service
 * with a media session (lock screen, notification shade, Bluetooth and headset buttons).
 *
 * The core plays the audio; this service only mirrors its state, read with
 * [MediaBridge.waitState], and sends button presses back with [MediaBridge.command]. It starts
 * when playback starts and stops when playback stops.
 */
class PlaybackService : Service() {
    companion object {
        private const val TAG = "CadenzaPlayback"
        private const val CHANNEL = "playback"
        private const val NOTIFICATION_ID = 1
        private const val ACTION_TOGGLE = "app.cadenza.TOGGLE"
        private const val ACTION_NEXT = "app.cadenza.NEXT"
        private const val ACTION_PREVIOUS = "app.cadenza.PREVIOUS"
        private const val ACTION_STOP = "app.cadenza.STOP"

        @Volatile
        private var running = false
        private var watcher: Thread? = null

        /** Starts the service whenever playback starts. Called once the app's UI has loaded. */
        @Synchronized
        fun watch(context: Context) {
            if (watcher != null) return
            watcher = thread(isDaemon = true, name = "cadenza-session-watch") {
                var version = 0L
                while (true) {
                    val json = MediaBridge.waitState(version, 60_000) ?: continue
                    val state = JSONObject(json)
                    version = state.getLong("version")
                    if (state.getString("status") == "playing" && !running) {
                        try {
                            context.startForegroundService(Intent(context, PlaybackService::class.java))
                        } catch (e: Exception) {
                            // Android 12+ refuses when playback started while Cadenza was in the
                            // background; it continues, without the screen-off guarantee.
                            Log.w(TAG, "cannot start the playback service", e)
                        }
                    }
                }
            }
        }
    }

    private val main = Handler(Looper.getMainLooper())
    private lateinit var session: MediaSession

    @Volatile
    private var alive = true
    private var artPath: String? = null
    private var art: Bitmap? = null

    override fun onCreate() {
        super.onCreate()
        running = true
        getSystemService(NotificationManager::class.java).createNotificationChannel(
            NotificationChannel(CHANNEL, "Playback", NotificationManager.IMPORTANCE_LOW).apply {
                description = "What's playing, with playback controls"
                setShowBadge(false)
            }
        )
        // Android requires startForeground within seconds of startForegroundService, even if the
        // service then stops at once.
        val first = notification(null, playing = true)
        if (Build.VERSION.SDK_INT >= 29) {
            startForeground(NOTIFICATION_ID, first, ServiceInfo.FOREGROUND_SERVICE_TYPE_MEDIA_PLAYBACK)
        } else {
            startForeground(NOTIFICATION_ID, first)
        }
        // A fresh process (Android restarted it for a stale notification button) has no core.
        val ready = try {
            MediaBridge.ready()
        } catch (_: UnsatisfiedLinkError) {
            false
        }
        if (!ready) {
            stopForeground(STOP_FOREGROUND_REMOVE)
            stopSelf()
            return
        }
        session = MediaSession(this, "Cadenza").apply {
            setCallback(object : MediaSession.Callback() {
                override fun onPlay() = MediaBridge.command(MediaBridge.PLAY, 0)
                override fun onPause() = MediaBridge.command(MediaBridge.PAUSE, 0)
                override fun onSkipToNext() = MediaBridge.command(MediaBridge.NEXT, 0)
                override fun onSkipToPrevious() = MediaBridge.command(MediaBridge.PREVIOUS, 0)
                override fun onStop() = MediaBridge.command(MediaBridge.STOP, 0)
                override fun onSeekTo(pos: Long) = MediaBridge.command(MediaBridge.SEEK, pos)
            })
            isActive = true
        }
        thread(isDaemon = true, name = "cadenza-session") {
            var version = 0L
            while (alive) {
                val json = MediaBridge.waitState(version, 1_000) ?: continue
                val state = JSONObject(json)
                version = state.getLong("version")
                main.post { if (alive) show(state) }
            }
        }
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        if (!::session.isInitialized) return START_NOT_STICKY
        when (intent?.action) {
            ACTION_TOGGLE -> MediaBridge.command(MediaBridge.TOGGLE, 0)
            ACTION_NEXT -> MediaBridge.command(MediaBridge.NEXT, 0)
            ACTION_PREVIOUS -> MediaBridge.command(MediaBridge.PREVIOUS, 0)
            ACTION_STOP -> MediaBridge.command(MediaBridge.STOP, 0)
        }
        // If Android kills the process, it doesn't restart the service: there'd be no core.
        return START_NOT_STICKY
    }

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onDestroy() {
        alive = false
        running = false
        if (::session.isInitialized) session.release()
        super.onDestroy()
    }

    private fun show(state: JSONObject) {
        val status = state.getString("status")
        if (status == "stopped") {
            stopForeground(STOP_FOREGROUND_REMOVE)
            stopSelf()
            return
        }
        val playing = status == "playing"
        val path = state.optString("artPath").ifEmpty { null }
        if (path != artPath) {
            artPath = path
            art = path?.let { BitmapFactory.decodeFile(it) }
        }
        session.setMetadata(
            MediaMetadata.Builder()
                .putString(MediaMetadata.METADATA_KEY_TITLE, state.optString("title"))
                .putString(MediaMetadata.METADATA_KEY_ARTIST, state.optString("artist"))
                .putString(MediaMetadata.METADATA_KEY_ALBUM, state.optString("album"))
                .putLong(MediaMetadata.METADATA_KEY_DURATION, state.optLong("durationMs"))
                .putBitmap(MediaMetadata.METADATA_KEY_ALBUM_ART, art)
                .build()
        )
        session.setPlaybackState(
            PlaybackState.Builder()
                .setActions(
                    PlaybackState.ACTION_PLAY or PlaybackState.ACTION_PAUSE or
                        PlaybackState.ACTION_PLAY_PAUSE or PlaybackState.ACTION_SKIP_TO_NEXT or
                        PlaybackState.ACTION_SKIP_TO_PREVIOUS or PlaybackState.ACTION_SEEK_TO or
                        PlaybackState.ACTION_STOP
                )
                // The system moves the position on from here while playing.
                .setState(
                    if (playing) PlaybackState.STATE_PLAYING else PlaybackState.STATE_PAUSED,
                    state.optLong("positionMs"),
                    if (playing) 1f else 0f,
                    SystemClock.elapsedRealtime()
                )
                .build()
        )
        getSystemService(NotificationManager::class.java)
            .notify(NOTIFICATION_ID, notification(state, playing))
    }

    private fun notification(state: JSONObject?, playing: Boolean): Notification {
        val open = packageManager.getLaunchIntentForPackage(packageName)?.let {
            PendingIntent.getActivity(this, 0, it, PendingIntent.FLAG_IMMUTABLE)
        }
        val builder = Notification.Builder(this, CHANNEL)
            .setSmallIcon(R.drawable.ic_cadenza_note)
            .setContentTitle(state?.optString("title") ?: "Cadenza")
            .setContentText(state?.optString("artist") ?: "")
            .setLargeIcon(art)
            .setContentIntent(open)
            .setVisibility(Notification.VISIBILITY_PUBLIC)
            .setOnlyAlertOnce(true)
            .addAction(action(android.R.drawable.ic_media_previous, "Previous", ACTION_PREVIOUS, 1))
            .addAction(
                if (playing) {
                    action(android.R.drawable.ic_media_pause, "Pause", ACTION_TOGGLE, 2)
                } else {
                    action(android.R.drawable.ic_media_play, "Play", ACTION_TOGGLE, 2)
                }
            )
            .addAction(action(android.R.drawable.ic_media_next, "Next", ACTION_NEXT, 3))
            .addAction(action(android.R.drawable.ic_menu_close_clear_cancel, "Stop", ACTION_STOP, 4))
        if (::session.isInitialized) {
            builder.setStyle(
                Notification.MediaStyle()
                    .setMediaSession(session.sessionToken)
                    .setShowActionsInCompactView(0, 1, 2)
            )
        }
        return builder.build()
    }

    private fun action(icon: Int, title: String, action: String, code: Int): Notification.Action {
        val intent = Intent(this, PlaybackService::class.java).setAction(action)
        val pending = PendingIntent.getService(
            this, code, intent, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT
        )
        return Notification.Action.Builder(Icon.createWithResource(this, icon), title, pending).build()
    }
}
