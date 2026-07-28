use dioxus::prelude::*;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::OnceLock;
use std::thread;

mod components;
mod storage;

use components::ChatList;
use components::ChatView;
use crate::storage::config::Config;
use components::Settings;

const MAIN_CSS: &str = include_str!("../assets/main.css");
const TAILWIND_CSS: &str = include_str!("../assets/tailwind.css");

static MEDIA_PORT: OnceLock<u16> = OnceLock::new();

/// Get the HTTP media server port
pub fn media_port() -> u16 {
    *MEDIA_PORT.get().unwrap_or(&0)
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
            let hex: String = chars.by_ref().take(2).collect();
            if hex.len() == 2 {
                if let Ok(byte) = u8::from_str_radix(&hex, 16) {
                    out.push(byte as char);
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn mime_for_extension(path: &str) -> &'static str {
    let lower = path.to_lowercase();
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

fn start_media_server(cache_base: PathBuf) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0")
        .expect("[WACV] Failed to bind media server");
    let port = listener.local_addr().unwrap().port();
    eprintln!("[WACV] HTTP media server on 127.0.0.1:{}", port);

    thread::spawn(move || {
        for stream in listener.incoming() {
            match stream {
                Ok(stream) => {
                    let cache = cache_base.clone();
                    thread::spawn(move || handle_connection(stream, cache));
                }
                Err(e) => eprintln!("[WACV] accept error: {}", e),
            }
        }
    });

    port
}

fn handle_connection(mut stream: TcpStream, cache_base: PathBuf) {
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
        if reader.read_line(&mut line).is_err() { return; }
        if line.trim().is_empty() { break; }
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


fn main() {
    let cache_base = dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("wacv")
        .join("media");

    eprintln!("[WACV] Media cache base: {}", cache_base.display());

    let port = start_media_server(cache_base);
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
        .with_custom_index(html);

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
    use_context_provider(|| config);

    rsx! {
        style { "{TAILWIND_CSS}" }
        style { "{MAIN_CSS}" }
        div { class: "h-screen w-screen flex flex-col bg-gray-100 overflow-hidden",
            Router::<Route> {}
        }
    }
}

