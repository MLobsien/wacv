//! Android JNI initialisation + JNI-based file picker.
//!
//! File picking bypasses WebView `<input type="file">` (which doesn't trigger
//! `onShowFileChooser` on some Android versions) and instead uses Android's
//! `Intent.ACTION_OPEN_DOCUMENT` directly via JNI.

use jni::objects::{GlobalRef, JObject, JString};
use std::path::PathBuf;
use std::sync::{mpsc, Mutex, OnceLock};
use std::time::Duration;

// ── global state ──────────────────────────────────────────────────────

static JAVA_VM: OnceLock<jni::JavaVM> = OnceLock::new();
static ACTIVITY: OnceLock<GlobalRef> = OnceLock::new();

/// Android app-private data directory (getFilesDir()).
static ANDROID_DATA_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Android app-private cache directory (getCacheDir()).
static ANDROID_CACHE_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Last URI returned by the system file picker.
static PICKER_RESULT_URI: OnceLock<Mutex<Option<String>>> = OnceLock::new();

// ── public API ────────────────────────────────────────────────────────

/// Initialise Android paths via JNI (idempotent, thread-safe).
pub fn init() {
    if ANDROID_DATA_DIR.get().is_some() {
        return;
    }
    PICKER_RESULT_URI.get_or_init(|| Mutex::new(None));

    let (tx, rx) = mpsc::channel();
    wry::prelude::dispatch(move |env, activity, _webview| {
        let _ = tx.send(do_init(env, activity));
    });
    match rx.recv() {
        Ok(Ok(())) => {}
        Ok(Err(e)) => eprintln!("[WACV] android::init failed: {e}"),
        Err(_) => eprintln!("[WACV] android::init channel error"),
    }
}

/// Android app-private data directory (e.g. /data/data/.../files).
pub fn android_data_dir() -> Option<&'static PathBuf> {
    ANDROID_DATA_DIR.get()
}

/// Android app-private cache directory (e.g. /data/data/.../cache).
pub fn android_cache_dir() -> Option<&'static PathBuf> {
    ANDROID_CACHE_DIR.get()
}

/// True if JNI init has completed successfully.
pub fn is_initialised() -> bool {
    ANDROID_DATA_DIR.get().is_some()
}

/// Launch the system file picker for application/zip and return (filename, bytes).
///
/// Spawns the intent on the UI thread via `wry::dispatch()`, then polls for
/// the content-URI returned through `MainActivity.onActivityResult()`.
pub fn pick_zip_file() -> Result<(String, Vec<u8>), String> {
    launch_file_picker_impl()?;
    let uri = poll_picker_result()?;
    let bytes = read_uri_content(&uri)?;

    // Best-effort filename: query OpenableColumns.DISPLAY_NAME (the URI's
    // last segment is often a numeric document id or an encoded path).
    let fname = display_name_for_uri(&uri);
    eprintln!(
        "[WACV] JNI picker got file: {fname} ({} bytes)",
        bytes.len()
    );
    Ok((fname, bytes))
}

/// Store a content-URI returned by the system file picker.
/// Called from Kotlin `MainActivity.onActivityResult()` via `FilePickerHelper`.
#[no_mangle]
pub extern "system" fn Java_dev_dioxus_main_FilePickerHelper_nativeStoreResult(
    mut env: jni::JNIEnv,
    _class: jni::objects::JClass,
    uri: JString,
) {
    let s: String = env
        .get_string(&uri)
        .map(|s| s.into())
        .unwrap_or_default();
    if !s.is_empty() {
        eprintln!("[WACV] nativeStoreResult: {s}");
    }
    // Store even empty strings: an empty URI signals picker cancellation so
    // poll_picker_result can return immediately instead of blocking.
    if let Some(lock) = PICKER_RESULT_URI.get() {
        *lock.lock().unwrap() = Some(s);
    }
}

// ── internal helpers ──────────────────────────────────────────────────

/// MUST run on the Android UI thread (called from `wry::dispatch`).
fn do_init(env: &mut jni::JNIEnv, activity: &JObject) -> Result<(), String> {
    let jvm = env.get_java_vm().map_err(|e| format!("get_java_vm: {e}"))?;
    JAVA_VM.set(jvm).ok();

    let act = env
        .new_global_ref(activity)
        .map_err(|e| format!("global_ref activity: {e}"))?;
    ACTIVITY.set(act).ok();

    store_android_paths(env, activity)?;
    eprintln!("[WACV] Android JNI init complete");
    Ok(())
}

fn store_android_paths(env: &mut jni::JNIEnv, activity: &JObject) -> Result<(), String> {
    if ANDROID_DATA_DIR.get().is_some() {
        return Ok(());
    }

    // data dir (getFilesDir)
    let files = env
        .call_method(activity, "getFilesDir", "()Ljava/io/File;", &[])
        .map_err(|e| format!("getFilesDir: {e}"))?
        .l()
        .map_err(|e| format!("getFilesDir l: {e}"))?;
    let data_abs = env
        .call_method(&files, "getAbsolutePath", "()Ljava/lang/String;", &[])
        .map_err(|e| format!("getAbsolutePath data: {e}"))?
        .l()
        .map_err(|e| format!("getAbsolutePath data l: {e}"))?;
    let data_jstr = JString::from(data_abs);
    let data_s: String = env
        .get_string(&data_jstr)
        .map_err(|e| format!("get_string data: {e}"))?
        .into();
    let data_path = PathBuf::from(&data_s);
    ANDROID_DATA_DIR.set(data_path.clone()).ok();

    // cache dir (getCacheDir)
    let cache = env
        .call_method(activity, "getCacheDir", "()Ljava/io/File;", &[])
        .map_err(|e| format!("getCacheDir: {e}"))?
        .l()
        .map_err(|e| format!("getCacheDir l: {e}"))?;
    let cache_abs = env
        .call_method(&cache, "getAbsolutePath", "()Ljava/lang/String;", &[])
        .map_err(|e| format!("getAbsolutePath cache: {e}"))?
        .l()
        .map_err(|e| format!("getAbsolutePath cache l: {e}"))?;
    let cache_jstr = JString::from(cache_abs);
    let cache_s: String = env
        .get_string(&cache_jstr)
        .map_err(|e| format!("get_string cache: {e}"))?
        .into();
    let cache_path = PathBuf::from(&cache_s);
    ANDROID_CACHE_DIR.set(cache_path.clone()).ok();

    // Media lives in the data dir (same storage as chats), NOT the cache dir:
    // clearing the cache must not permanently delete media.
    let media_cache = data_path.join("wacv").join("media");
    crate::set_media_cache_base(media_cache);

    eprintln!(
        "[WACV] Android paths: data={:?} cache={:?}",
        ANDROID_DATA_DIR.get(),
        ANDROID_CACHE_DIR.get()
    );
    Ok(())
}

/// Call `FilePickerHelper.launch(activity)` on the UI thread.
/// Uses the Activity's class loader to find our app class (find_class
/// uses the boot classloader which can't see application classes on Android).
fn launch_file_picker_impl() -> Result<(), String> {
    eprintln!("[WACV] launch_file_picker: dispatching to UI thread");
    let (tx, rx) = mpsc::channel();
    wry::prelude::dispatch(move |env, activity, _webview| {
        let result = (|| -> Result<(), String> {
            // Get the app's ClassLoader from the Activity
            let class_loader = env
                .call_method(activity, "getClassLoader", "()Ljava/lang/ClassLoader;", &[])
                .map_err(|e| format!("getClassLoader: {e}"))?
                .l()
                .map_err(|e| format!("getClassLoader l: {e}"))?;

            // Load FilePickerHelper via the app ClassLoader
            let class_loader_class = env.find_class("java/lang/ClassLoader")
                .map_err(|e| format!("find ClassLoader: {e}"))?;
            let name = env.new_string("dev.dioxus.main.FilePickerHelper")
                .map_err(|e| format!("new_string: {e}"))?;
            let helper_jobj = env
                .call_method(&class_loader, "loadClass", "(Ljava/lang/String;)Ljava/lang/Class;", &[(&name).into()])
                .map_err(|e| format!("loadClass: {e}"))?
                .l()
                .map_err(|e| format!("loadClass l: {e}"))?;
            let helper: jni::objects::JClass = unsafe { jni::objects::JClass::from_raw(helper_jobj.into_raw()) };

            env.call_static_method(
                helper,
                "launch",
                "(Landroid/app/Activity;)V",
                &[activity.into()],
            )
            .map_err(|e| format!("launch call: {e}"))?;
            Ok(())
        })();
        let _ = tx.send(result);
    });
    rx.recv().map_err(|_| "dispatch channel closed".to_string())?
}

/// Poll `PICKER_RESULT_URI` (set from Kotlin via `nativeStoreResult`) with
/// a 30-second timeout.
fn poll_picker_result() -> Result<String, String> {
    eprintln!("[WACV] poll_picker_result: waiting for URI...");
    let lock = PICKER_RESULT_URI
        .get()
        .ok_or("PICKER_RESULT_URI not init")?;

    for i in 0..300 {
        // 30 s timeout
        let mut guard = lock.lock().unwrap();
        if let Some(uri) = guard.take() {
            if uri.is_empty() {
                eprintln!("[WACV] poll_picker_result: picker cancelled");
                return Err("File picker cancelled".to_string());
            }
            eprintln!("[WACV] poll_picker_result: got URI after {i} polls");
            return Ok(uri);
        }
        drop(guard);
        std::thread::sleep(Duration::from_millis(100));
    }
    Err("Timed out waiting for file picker result".to_string())
}

/// Read all bytes from a `content://` URI through Android's ContentResolver.
fn read_uri_content(uri_str: &str) -> Result<Vec<u8>, String> {
    eprintln!("[WACV] read_uri_content: {uri_str}");
    let jvm = JAVA_VM.get().ok_or("JAVA_VM not init")?;
    let mut guard = jvm
        .attach_current_thread_as_daemon()
        .map_err(|e| format!("attach: {e}"))?;

    guard
        .with_local_frame::<_, Vec<u8>, jni::errors::Error>(32, |env| {
            let activity = ACTIVITY.get().expect("ACTIVITY not init");


            // Parse URI string -> android.net.Uri
            let uri_class = env.find_class("android/net/Uri").unwrap();
            let uri_jstr = env.new_string(uri_str).unwrap();
            let uri_obj = env
                .call_static_method(
                    &uri_class,
                    "parse",
                    "(Ljava/lang/String;)Landroid/net/Uri;",
                    &[(&uri_jstr).into()],
                )
                .unwrap()
                .l()
                .unwrap();

            // ContentResolver
            let resolver = env
                .call_method(
                    activity.as_obj(),
                    "getContentResolver",
                    "()Landroid/content/ContentResolver;",
                    &[],
                )
                .unwrap()
                .l()
                .unwrap();

            // Open InputStream
            let stream = env
                .call_method(
                    &resolver,
                    "openInputStream",
                    "(Landroid/net/Uri;)Ljava/io/InputStream;",
                    &[(&uri_obj).into()],
                )
                .unwrap()
                .l()
                .unwrap();

            // Read all bytes via ByteArrayOutputStream
            let baos_class = env.find_class("java/io/ByteArrayOutputStream").unwrap();
            let baos = env
                .new_object(&baos_class, "()V", &[])
                .unwrap();
            let buf = env.new_byte_array(4096).unwrap();

            loop {
                let nread = env
                    .call_method(&stream, "read", "([B)I", &[(&buf).into()])
                    .unwrap()
                    .i()
                    .unwrap();
                if nread < 0 {
                    break;
                }
                env.call_method(
                    &baos,
                    "write",
                    "([BII)V",
                    &[(&buf).into(), 0.into(), nread.into()],
                )
                .unwrap();
            }

            let result = env
                .call_method(&baos, "toByteArray", "()[B", &[])
                .unwrap()
                .l()
                .unwrap();
            let jba: jni::objects::JByteArray = result.into();
            let rust_bytes = env.convert_byte_array(&jba).unwrap();

            // Close
            let _ = env.call_method(&stream, "close", "()V", &[]);

            eprintln!("[WACV] read_uri_content: {} bytes", rust_bytes.len());
            Ok(rust_bytes)
        })
        .map_err(|e| format!("read_uri_content failed: {e}"))
}

/// Best-effort original filename for a `content://` URI.
///
/// Queries `OpenableColumns.DISPLAY_NAME` through the ContentResolver; the
/// URI's last path segment is often a numeric document id (e.g. "570") or a
/// URL-encoded path, neither of which is a usable chat filename.
fn display_name_for_uri(uri: &str) -> String {
    if let Some(jvm) = JAVA_VM.get() {
        if let Ok(mut guard) = jvm.attach_current_thread_as_daemon() {
            if let Ok(Some(name)) = guard.with_local_frame::<_, Option<String>, jni::errors::Error>(64, |env| {
                let Some(activity) = ACTIVITY.get() else { return Ok(None) };

                // Parse URI string -> android.net.Uri
                let uri_class = env.find_class("android/net/Uri")?;
                let uri_jstr = env.new_string(uri)?;
                let uri_obj = env
                    .call_static_method(
                        &uri_class,
                        "parse",
                        "(Ljava/lang/String;)Landroid/net/Uri;",
                        &[(&uri_jstr).into()],
                    )?
                    .l()?;

                // ContentResolver
                let resolver = env
                    .call_method(
                        activity.as_obj(),
                        "getContentResolver",
                        "()Landroid/content/ContentResolver;",
                        &[],
                    )?
                    .l()?;

                // query(uri, ["display_name"], null, null, null)
                let string_class = env.find_class("java/lang/String")?;
                let projection = env.new_object_array(1, &string_class, JObject::null())?;
                let col = env.new_string("display_name")?;
                env.set_object_array_element(&projection, 0, &col)?;
                let null = JObject::null();
                let cursor = env
                    .call_method(
                        &resolver,
                        "query",
                        "(Landroid/net/Uri;[Ljava/lang/String;Ljava/lang/String;[Ljava/lang/String;Ljava/lang/String;)Landroid/database/Cursor;",
                        &[(&uri_obj).into(), (&projection).into(), (&null).into(), (&null).into(), (&null).into()],
                    )?
                    .l()?;
                if cursor.is_null() {
                    return Ok(None);
                }

                let moved = env.call_method(&cursor, "moveToFirst", "()Z", &[])?.z()?;
                let mut result = None;
                if moved {
                    let idx = env
                        .call_method(&cursor, "getColumnIndex", "(Ljava/lang/String;)I", &[(&col).into()])?
                        .i()?;
                    if idx >= 0 {
                        let s = env
                            .call_method(&cursor, "getString", "(I)Ljava/lang/String;", &[idx.into()])?
                            .l()?;
                        if !s.is_null() {
                            result = env.get_string(&JString::from(s)).ok().map(|s| s.into());
                        }
                    }
                }
                let _ = env.call_method(&cursor, "close", "()V", &[]);
                Ok(result)
            }) {
                if !name.is_empty() {
                    return name;
                }
            } else {
                eprintln!("[WACV] display_name query failed, falling back to URI segment");
            }
        }
    }
    // Fallback: last URI segment, URL-decoded, stripping the provider
    // prefix (e.g. "primary:") that some documents providers prepend.
    let last = uri.rsplit('/').next().unwrap_or("chat.zip");
    let decoded = crate::url_decode(last);
    decoded
        .strip_prefix("primary:")
        .unwrap_or(&decoded)
        .to_string()
}
