package app.cadenza.android

import android.Manifest
import android.app.Activity
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.os.Environment
import android.provider.DocumentsContract
import android.provider.Settings
import android.webkit.WebView
import androidx.activity.result.ActivityResult
import app.tauri.annotation.ActivityCallback
import app.tauri.annotation.Command
import app.tauri.annotation.Permission
import app.tauri.annotation.PermissionCallback
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin

/** Storage access and the folder picker, called by the shell (`plugins/android/src`). */
@TauriPlugin(
    permissions = [Permission(strings = [Manifest.permission.READ_EXTERNAL_STORAGE], alias = "storage")]
)
class CadenzaPlugin(private val activity: Activity) : Plugin(activity) {

    override fun load(webView: WebView) {
        super.load(webView)
        // The playback notification needs this from Android 13; without it playback still
        // continues, only the notification is hidden.
        if (Build.VERSION.SDK_INT >= 33 &&
            activity.checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) !=
            PackageManager.PERMISSION_GRANTED
        ) {
            activity.requestPermissions(arrayOf(Manifest.permission.POST_NOTIFICATIONS), 7301)
        }
        PlaybackService.watch(activity.applicationContext)
    }

    private fun hasAccess(): Boolean =
        if (Build.VERSION.SDK_INT >= 30) {
            Environment.isExternalStorageManager()
        } else {
            activity.checkSelfPermission(Manifest.permission.READ_EXTERNAL_STORAGE) ==
                PackageManager.PERMISSION_GRANTED
        }

    private fun resolveAccess(invoke: Invoke) =
        invoke.resolve(JSObject().put("granted", hasAccess()))

    @Command
    fun storageAccess(invoke: Invoke) = resolveAccess(invoke)

    /** Android 11+: the "All files access" settings page; before that, the permission prompt. */
    @Command
    fun requestStorageAccess(invoke: Invoke) {
        if (hasAccess()) return resolveAccess(invoke)
        if (Build.VERSION.SDK_INT >= 30) {
            val intent = Intent(
                Settings.ACTION_MANAGE_APP_ALL_FILES_ACCESS_PERMISSION,
                Uri.parse("package:${activity.packageName}")
            )
            startActivityForResult(invoke, intent, "accessResult")
        } else {
            requestPermissionForAlias("storage", invoke, "permissionResult")
        }
    }

    @ActivityCallback
    fun accessResult(invoke: Invoke, @Suppress("UNUSED_PARAMETER") result: ActivityResult) =
        resolveAccess(invoke)

    @PermissionCallback
    fun permissionResult(invoke: Invoke) = resolveAccess(invoke)

    /** Back from the first screen: to the background, not closed, so the music carries on. */
    @Command
    fun toBackground(invoke: Invoke) {
        activity.moveTaskToBack(true)
        invoke.resolve()
    }

    @Command
    fun pickFolder(invoke: Invoke) {
        startActivityForResult(invoke, Intent(Intent.ACTION_OPEN_DOCUMENT_TREE), "folderResult")
    }

    @ActivityCallback
    fun folderResult(invoke: Invoke, result: ActivityResult) {
        val uri = result.data?.data
        if (result.resultCode != Activity.RESULT_OK || uri == null) {
            return invoke.resolve(JSObject())
        }
        val path = treePath(uri)
        if (path == null) {
            invoke.reject("That folder isn't on this phone's storage. Choose a folder on the phone or its SD card.")
        } else {
            invoke.resolve(JSObject().put("path", path))
        }
    }

    /**
     * The file path of a folder chosen in the system picker: "primary:Music/Jazz" is
     * /storage/emulated/0/Music/Jazz, "1A2B-3C4D:Music" is /storage/1A2B-3C4D/Music. Folders from
     * other providers (cloud drives) have no path.
     */
    private fun treePath(uri: Uri): String? {
        if (uri.authority != "com.android.externalstorage.documents") return null
        val id = DocumentsContract.getTreeDocumentId(uri)
        val volume = id.substringBefore(':')
        val relative = id.substringAfter(':', "").trimEnd('/')
        @Suppress("DEPRECATION")
        val base = if (volume.equals("primary", ignoreCase = true)) {
            Environment.getExternalStorageDirectory().path
        } else {
            "/storage/$volume"
        }
        return if (relative.isEmpty()) base else "$base/$relative"
    }
}
