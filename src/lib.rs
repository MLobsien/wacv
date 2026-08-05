use dioxus::prelude::*;
#[cfg(target_os = "android")]
use futures::StreamExt;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::OnceLock;
use std::thread;

#[cfg(target_os = "android")]
mod android;
mod components;
pub mod storage;

use crate::storage::config::Config;
use components::ChatList;
use components::ChatView;
use components::Settings;

const TAILWIND_CSS: &str = include_str!("../assets/tailwind.css");

static MEDIA_CACHE_BASE: std::sync::Mutex<Option<PathBuf>> = std::sync::Mutex::new(None);
static MEDIA_PORT: OnceLock<u16> = OnceLock::new();

/// Get the HTTP media server port
pub fn media_port() -> u16 {
    *MEDIA_PORT.get().unwrap_or(&0)
}

/// Set the media server cache base (called from android.rs after JNI init).
/// Overwrites the temporary fallback set in main().
pub fn set_media_cache_base(path: PathBuf) {
    *MEDIA_CACHE_BASE.lock().unwrap() = Some(path);
}

/// URL-encode a path segment
pub fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            b' ' => out.push_str("%20"),
            b'/' => out.push_str("%2F"),
            b':' => out.push_str("%3A"),
            _ => {
                out.push_str(&format!("%{:02X}", b));
            }
        }
    }
    out
}

/// URL-decode a path segment. Percent-encoded bytes are collected and then
/// decoded as a single UTF-8 sequence, so multi-byte characters (emoji in
/// chat names, accented letters) round-trip correctly instead of being
/// mangled one byte at a time.
pub fn url_decode(s: &str) -> String {
    let mut bytes = Vec::with_capacity(s.len());
    let mut chars = s.chars();

    while let Some(c) = chars.next() {
        if c == '%' {
            let hi = chars.next().and_then(|c| c.to_digit(16));
            let lo = chars.next().and_then(|c| c.to_digit(16));
            match (hi, lo) {
                (Some(h), Some(l)) => bytes.push((h << 4 | l) as u8),
                _ => bytes.push(b'%'),
            }
        } else {
            // Raw (unencoded) character in the path: keep its UTF-8 bytes.
            let mut buf = [0u8; 4];
            bytes.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
        }
    }

    String::from_utf8_lossy(&bytes).into_owned()
}

fn mime_for_extension(filename: &str) -> &'static str {
    let lower = filename.to_lowercase();
    if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        "image/jpeg"
    } else if lower.ends_with(".png") {
        "image/png"
    } else if lower.ends_with(".webp") {
        "image/webp"
    } else if lower.ends_with(".mp4") {
        "video/mp4"
    } else if lower.ends_with(".webm") {
        "video/webm"
    } else if lower.ends_with(".mp3") {
        "audio/mpeg"
    } else if lower.ends_with(".ogg") || lower.ends_with(".opus") {
        "audio/ogg"
    } else {
        "application/octet-stream"
    }
}

fn start_media_server() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("[WACV] Failed to bind media server");
    let port = listener.local_addr().unwrap().port();
    eprintln!("[WACV] HTTP media server on 127.0.0.1:{}", port);

    thread::spawn(move || {
        for stream in listener.incoming() {
            match stream {
                Ok(stream) => {
                    thread::spawn(move || handle_connection(stream));
                }
                Err(e) => eprintln!("[WACV] accept error: {}", e),
            }
        }
    });

    port
}

fn handle_connection(mut stream: TcpStream) {
    let cache_base = MEDIA_CACHE_BASE
        .lock()
        .unwrap()
        .clone()
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    let mut reader = BufReader::new(&stream);

    // Read request line
    let mut request_line = String::new();
    if reader.read_line(&mut request_line).is_err() || request_line.is_empty() {
        return;
    }
    let request_line = request_line.trim();

    // Parse "GET /path HTTP/1.1"
    let parts: Vec<&str> = request_line.splitn(3, ' ').collect();
    if parts.len() < 2 || parts[0] != "GET" {
        let _ = stream.write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\n\r\n");
        return;
    }
    let path = parts[1];

    // Drain remaining headers
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).is_err() {
            return;
        }
        if line.trim().is_empty() {
            break;
        }
    }

    // Split the raw path on '/' first, then decode each segment. Splitting
    // before decoding keeps a percent-encoded slash (%2F) inside a chat name
    // from being decoded into a real separator.
    let path_stripped = path.trim_start_matches('/');
    let segments: Vec<&str> = path_stripped.splitn(2, '/').collect();
    if segments.len() < 2 {
        let _ = stream.write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\n\r\n");
        return;
    }

    let chat_name = url_decode(segments[0]);
    let filename = url_decode(segments[1]);
    let file_path = cache_base.join(chat_name).join(&filename);

    match std::fs::read(&file_path) {
        Ok(data) => {
            let mime = mime_for_extension(&filename);
            eprintln!("[WACV] 200: {} ({} bytes, {})", filename, data.len(), mime);
            respond_full(&mut stream, mime, &data);
        }
        Err(e) => {
            eprintln!("[WACV] 404: {} -> {}", path_stripped, e);
            let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n");
        }
    }
}

fn respond_full(stream: &mut TcpStream, content_type: &str, body: &[u8]) {
    let headers = format!("HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {}\r\nX-Content-Type-Options: nosniff\r\nAccess-Control-Allow-Origin: *\r\nCache-Control: no-cache\r\n\r\n", content_type, body.len());
    let _ = stream.write_all(headers.as_bytes());
    let _ = stream.write_all(body);
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn main() {
    eprintln!("[WACV] Android main()");
    // On Android, dirs::*() returns None.  Use temp_dir as fallback until
    // android::init() sets the real paths via JNI.
    let cache_base = std::env::temp_dir().join("wacv").join("media");
    set_media_cache_base(cache_base);

    let port = start_media_server();
    MEDIA_PORT.set(port).unwrap();

    dioxus::LaunchBuilder::new().launch(App);
}

#[cfg(not(target_os = "android"))]
pub fn main() {
    let cache_base = dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("wacv")
        .join("media");
    set_media_cache_base(cache_base.clone());

    eprintln!("[WACV] Media dir: {}", cache_base.display());

    eprintln!("[WACV] Media cache base: {}", cache_base.display());

    let port = start_media_server();
    MEDIA_PORT.set(port).unwrap();

    let html = r#"<!DOCTYPE html>
<html>
<head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <meta http-equiv="Content-Security-Policy" content="default-src * 'unsafe-inline' 'unsafe-eval' data: ws:; img-src * data:; media-src *;">
</head>
<body>
    <div id="main"></div>
</body>
</html>"#.to_string();

    let config = dioxus::desktop::Config::new()
        .with_custom_index(html)
        .with_menu(None)
        .with_window(dioxus::desktop::WindowBuilder::new().with_decorations(false));

    dioxus::LaunchBuilder::new().with_cfg(config).launch(App);
}

#[derive(Routable, Clone, PartialEq)]
enum Route {
    #[route("/")]
    ChatList {},
    #[route("/chat/:name")]
    ChatView { name: String },
    #[route("/settings")]
    Settings {},
}

fn App() -> Element {
    let config = use_signal(|| Config::load());
    eprintln!("[WACV] App() rendered, loading config...");
    use_context_provider(|| config);

    // On Android, eagerly init JNI so config/data paths are available before
    // the user interacts with the app. Rendering is gated on completion so
    // ChatStorage/Config never observe the pre-init fallback paths.
    #[cfg(target_os = "android")]
    let android_ready = use_signal(|| false);

    #[cfg(target_os = "android")]
    {
        // Coroutine body runs on the dioxus main thread, so it may touch
        // signals. The background init thread only sends a message through
        // the Send-safe UnboundedSender.
        let mut ready = android_ready.clone();
        let mut cfg = config.clone();
        let coroutine = use_coroutine(move |mut rx: UnboundedReceiver<()>| async move {
            while let Some(()) = rx.next().await {
                // Android paths are now set: render the app and load the real config.
                ready.set(true);
                *cfg.write() = Config::load();
            }
        });

        use_effect(move || {
            if !android_ready() {
                let tx = coroutine.tx();
                std::thread::spawn(move || {
                    // Small delay ensures the webview is running before dispatch().
                    std::thread::sleep(std::time::Duration::from_millis(100));
                    crate::android::init();
                    let _ = tx.unbounded_send(());
                });
            }
        });
    }

    #[cfg(target_os = "android")]
    let content = if android_ready() {
        rsx! { Router::<Route> {} }
    } else {
        rsx! {
            div { class: "flex items-center justify-center h-full",
                div { class: "animate-spin rounded-full h-8 w-8 border-b-2 border-green-500" }
            }
        }
    };

    #[cfg(not(target_os = "android"))]
    let content = rsx! { Router::<Route> {} };

    let dark_class = if config.read().dark_mode { "dark" } else { "" };
    rsx! {
        style { "{TAILWIND_CSS}" }
        div { class: "h-screen w-screen flex flex-col {dark_class} bg-gray-100 dark:bg-gray-900 overflow-hidden",
            {content}
        }
    }
}


#[cfg(not(target_os = "android"))]
pub fn cli_main() {
    use crate::storage::ChatStorage;
    use std::{fs, path::Path};

    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 || args[1] == "--help" || args[1] == "-h" {
        println!(r#"wacv import <path>    Import WhatsApp chat export(s). <path> is either a single .zip file or a
                      directory: in the latter case all .zip files directly inside it (not subdirectories) are
                      imported.

wacv --help            Show this help

Imported chats appear in the WACV app like those imported via the GUI.
"#);
        return;
    }

    if args[1] != "import" {
        eprintln!("[WACV] unknown command '{}'", args[1]);
        std::process::exit(2);
    }

    if args.len() < 3 {
        eprintln!("[WACV] usage: wacv import <path>");
        std::process::exit(2);
    }

    let path = Path::new(&args[2]);
    let storage = match ChatStorage::new() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[WACV] failed to open storage: {:#}", e);
            std::process::exit(1);
        }
    };

    let mut zips = Vec::new();
    if path.is_dir() {
        match fs::read_dir(path) {
            Ok(entries) => {
                for entry in entries.flatten() {
                    let p = entry.path();
                    if p.is_file()
                        && p.extension()
                            .map(|e| e.eq_ignore_ascii_case("zip"))
                            .unwrap_or(false)
                    {
                        zips.push(p);
                    }
                }
            }
            Err(e) => {
                eprintln!("[WACV] failed to read directory {}: {}", path.display(), e);
                std::process::exit(1);
            }
        }
        zips.sort();
    } else if path.is_file() {
        zips.push(path.to_path_buf());
    } else {
        eprintln!("[WACV] path doesn't exist: {}", path.display());
        std::process::exit(1);
    }

    if zips.is_empty() {
        eprintln!("[WACV] no .zip files found{}", if path.is_dir() { " in directory" } else { "" });
        std::process::exit(1);
    }

    let mut failures = 0;
    for zip in &zips {
        let filename = zip
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| zip.display().to_string());
        let bytes = match fs::read(zip) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("[WACV] failed to read {}: {}", filename, e);
                failures += 1;
                continue;
            }
        };
        match storage.import_chat(&bytes, &filename) {
            Ok(chat_name) => println!("[WACV] imported {} -> {}", filename, chat_name),
            Err(e) => {
                eprintln!("[WACV] failed to import {}: {:#}", filename, e);
                failures += 1;
            }
        }
    }

    if failures > 0 {
        eprintln!("[WACV] {failures} import(s) failed");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_roundtrip_preserves_multibyte_utf8() {
        // Emoji in a chat name must survive encode -> decode unchanged.
        let name = "🎉 Trip to Lisbon";
        assert_eq!(url_decode(&url_encode(name)), name);
    }

    #[test]
    fn url_decode_reassembles_utf8_sequences() {
        // U+1F600 (😀) is F0 9F 98 80 as UTF-8; decoding the four percent
        // escapes must rebuild the single code point, not four mojibake chars.
        assert_eq!(url_decode("%F0%9F%98%80"), "😀");
    }

    #[test]
    fn url_encode_escapes_slashes_and_spaces() {
        assert_eq!(url_encode("a/b c"), "a%2Fb%20c");
        assert_eq!(url_decode("a%2Fb%20c"), "a/b c");
    }

    #[test]
    fn url_decode_keeps_invalid_escapes() {
        assert_eq!(url_decode("100%"), "100%");
    }
}