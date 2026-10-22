package app.cadenza.android

/**
 * Calls into the Cadenza core, implemented by the app's native library (`cadenza_app_lib`,
 * `src/android.rs`). These work whether or not the activity is alive, which plugin calls don't.
 */
object MediaBridge {
    const val PLAY = 0
    const val PAUSE = 1
    const val TOGGLE = 2
    const val NEXT = 3
    const val PREVIOUS = 4
    const val STOP = 5
    const val SEEK = 6

    /** A transport command; `arg` is the position in milliseconds for [SEEK]. */
    @JvmStatic external fun command(code: Int, arg: Long)

    /**
     * Blocks until the playback state's version is newer than [after], or [timeoutMs] passes.
     * Returns the state as JSON (see `src/android.rs`), or null on timeout.
     */
    @JvmStatic external fun waitState(after: Long, timeoutMs: Long): String?

    /** Whether the core is running in this process (false, or a link error, if not). */
    @JvmStatic external fun ready(): Boolean
}
