package dev.dioxus.main;

// need to re-export buildconfig down from the parent
import com.example.Wacv.BuildConfig;
import android.app.Activity;
import android.content.Intent;
typealias BuildConfig = BuildConfig;

class MainActivity : WryActivity() {
    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        super.onActivityResult(requestCode, resultCode, data)
        FilePickerHelper.onActivityResult(requestCode, resultCode, data)
    }
}
