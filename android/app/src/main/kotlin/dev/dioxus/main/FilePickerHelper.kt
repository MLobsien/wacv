package dev.dioxus.main

import android.app.Activity
import android.content.Intent

object FilePickerHelper {
    private const val REQUEST_CODE = 9001

    init {
        System.loadLibrary("dioxusmain")
    }

    @JvmStatic
    fun launch(activity: Activity) {
        val intent = Intent(Intent.ACTION_OPEN_DOCUMENT).apply {
            addCategory(Intent.CATEGORY_OPENABLE)
            type = "application/zip"
        }
        activity.startActivityForResult(intent, REQUEST_CODE)
    }

    @JvmStatic
    fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        if (requestCode == REQUEST_CODE) {
            val uri = if (resultCode == Activity.RESULT_OK) data?.data?.toString() else null
            // Always notify Rust: empty string = cancelled/dismissed, so the
            // Rust side stops waiting instead of blocking until timeout.
            nativeStoreResult(uri ?: "")
        }
    }

    private external fun nativeStoreResult(uri: String)
}
