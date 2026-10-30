package org.ratspeak.android

import android.content.ClipData
import android.content.ClipDescription
import android.content.ClipboardManager
import android.content.Context
import android.os.Build
import android.os.PersistableBundle

/**
 * Keys and phrases on the clipboard: hidden from its previews, and cleared
 * when Rust's minute is up unless the app sees something else copied since.
 */
internal object RatspeakClipboard {
    private const val LABEL = "Ratspeak secret"
    @Volatile private var copiedAt = 0L

    /**
     * Call on the main thread. Before Android 9 the manager takes the looper
     * of the thread that first asks for it and the context keeps that one;
     * Rust calls in on threads without a looper.
     */
    fun initialize(context: Context) {
        context.getSystemService(ClipboardManager::class.java)
    }

    fun copy(context: Context, text: String): Boolean {
        try {
            val clipboard = context.getSystemService(ClipboardManager::class.java) ?: return false
            val clip = ClipData.newPlainText(LABEL, text)
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                clip.description.extras = PersistableBundle().apply {
                    putBoolean(ClipDescription.EXTRA_IS_SENSITIVE, true)
                }
            }
            clipboard.setPrimaryClip(clip)
            copiedAt = stamp(clipboard.primaryClipDescription)
            return true
        } catch (_: Exception) {
            return false
        }
    }

    fun clear(context: Context) {
        try {
            val clipboard = context.getSystemService(ClipboardManager::class.java) ?: return
            // Android 10 and later hide the clipboard from an app in the
            // background; the secret is cleared then too, as promised.
            val shown = clipboard.primaryClipDescription
            if (shown != null && (shown.label?.toString() != LABEL || stamp(shown) != copiedAt)) {
                return
            }
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
                clipboard.clearPrimaryClip()
            } else {
                clipboard.setPrimaryClip(ClipData.newPlainText("", ""))
            }
        } catch (_: Exception) {
        }
    }

    private fun stamp(description: ClipDescription?): Long =
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) description?.timestamp ?: 0L else 0L
}
