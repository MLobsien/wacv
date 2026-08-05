package dev.dioxus.main

import android.app.Activity
import android.content.Intent

object FilePickerHelper {
    private const val REQUEST_CODE = 9001
    private const val SEPARATOR = "\u0001"

    init {
        System.loadLibrary("dioxusmain")
    }

    @JvmStatic
    fun launch(activity: Activity) {
        val intent = Intent(Intent.ACTION_OPEN_DOCUMENT).apply {
            addCategory(Intent.CATEGORY_OPENABLE)
            type = "application/zip"
            putExtra(Intent.EXTRA_ALLOW_MULTIPLE, true)
        }
        activity.startActivityForResult(intent, REQUEST_CODE)
    }

    @JvmStatic
    fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        if (requestCode == REQUEST_CODE) {
            if (resultCode == Activity.RESULT_OK && data != null) {
                // Multiple files arrive in clipData, a single selection in data.data.
                val uris = mutableListOf<String>()
                data.clipData?.let { clip ->
                    for (i in 0 until clip.itemCount) {
                        clip.getItemAt(i).uri?.toString()?.let { uris.add(it) }
                    }
                }
                if (uris.isEmpty()) data.data?.toString()?.let { uris.add(it) }
                nativeStoreResult(uris.joinToString(SEPARATOR))
            } else {
                // Always notify Rust: empty string = cancelled/dismissed, so the
                // Rust side stops waiting instead of blocking until timeout.
                nativeStoreResult("")
            }
        }
    }

    private external fun nativeStoreResult(uri: String)
}
