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
mod storage;

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

/// URL-decode a path segment
pub fn url_decode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();

    while let Some(c) = chars.next() {
        if c == '%' {
            let hi = chars.next().and_then(|c| c.to_digit(16));
            let lo = chars.next().and_then(|c| c.to_digit(16));
            match (hi, lo) {
                (Some(h), Some(l)) => out.push((h << 4 | l) as u8 as char),
                _ => out.push('%'),
            }
        } else {
            out.push(c);
        }
    }
    out
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

    // Decode URL path
    let decoded_path = url_decode(path);
    let path_stripped = decoded_path.trim_start_matches('/');
    let segments: Vec<&str> = path_stripped.splitn(2, '/').collect();
    if segments.len() < 2 {
        let _ = stream.write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\n\r\n");
        return;
    }

    let chat_name = segments[0];
    let filename = segments[1];
    let file_path = cache_base.join(chat_name).join(filename);

    match std::fs::read(&file_path) {
        Ok(data) => {
            let mime = mime_for_extension(filename);
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
