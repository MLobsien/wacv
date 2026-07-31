use crate::storage::{ChatStorage, config::Config};
use dioxus::prelude::*;
#[cfg(not(target_os = "android"))]
use rfd::AsyncFileDialog;

/// Desktop: native file dialog via rfd.
#[cfg(not(target_os = "android"))]
async fn pick_file_dialog() -> Option<std::path::PathBuf> {
    eprintln!("[WACV] Opening GTK file dialog via rfd...");
    let handle = AsyncFileDialog::new()
        .add_filter("ZIP files", &["zip"])
        .pick_file()
        .await;
    let path = handle.as_ref().map(|h| h.path().to_path_buf());
    eprintln!("[WACV] File dialog result: {:?}", path);
    path
}

#[cfg(not(target_os = "android"))]
fn import_zipped(path: std::path::PathBuf, status: &mut Signal<String>, refresh: &mut Signal<u32>) {
    eprintln!("[WACV] File: {:?}", path);
    status.set(format!("Loading {}", path.display()));
    match std::fs::read(&path) {
        Ok(data) => {
            eprintln!("[WACV] Read {}B", data.len());
            let fname = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("chat.zip")
                .to_string();
            match ChatStorage::new() {
                Ok(storage) => match storage.import_chat(&data, &fname) {
                    Ok(name) => {
                        eprintln!("[WACV] Imported: {}", name);
                        status.set(format!("\u{2713} {} imported", name));
                        *refresh.write() += 1;
                    }
                    Err(e) => {
                        eprintln!("[WACV] Import err: {}", e);
                        status.set(format!("Error: {}", e));
                    }
                },
                Err(e) => {
                    eprintln!("[WACV] Storage err: {}", e);
                    status.set(format!("Storage: {}", e));
                }
            }
        }
        Err(e) => {
            eprintln!("[WACV] Read err: {}", e);
            status.set(format!("Read err: {}", e));
        }
    }
}

async fn get_chat_list() -> Result<Vec<String>, String> {
    let storage = ChatStorage::new().map_err(|e| format!("{:?}", e))?;
    storage.list_chats().map_err(|e| format!("{:?}", e))
}

#[component]
pub fn ChatList() -> Element {
    let nav = use_navigator();
    let refresh = use_signal(|| 0u32);
    use_context_provider(|| refresh);
    let chat_list = use_resource(move || {
        let _ = refresh();
        get_chat_list()
    });
    let config = use_context::<Signal<Config>>();
    let has_chats = chat_list
        .read()
        .as_ref()
        .and_then(|r| r.as_ref().ok())
        .map(|c| !c.is_empty())
        .unwrap_or(false);
    let show_name_prompt = has_chats && config.read().user_name.is_none();

    let list_content: Element = match &*chat_list.read() {
        Some(Ok(chats)) if chats.is_empty() => {
            rsx! {
                div { class: "flex flex-col items-center justify-center h-full text-gray-400 p-8 text-center",
                    svg {
                        class: "w-16 h-16 mb-4 text-gray-300",
                        fill: "none",
                        view_box: "0 0 24 24",
                        stroke: "currentColor",
                        stroke_width: "1.5",
                        path { d: "M8 12h.01M12 12h.01M16 12h.01M21 12c0 4.418-4.03 8-9 8a9.863 9.863 0 01-4.255-.949L3 20l1.395-3.72C3.512 15.042 3 13.574 3 12c0-4.418 4.03-8 9-8s9 3.582 9 8z" }
                    }
                    p { class: "text-lg font-medium", "No chats" }
                    p { class: "text-sm mt-1", "Import a WhatsApp chat to get started" }
                }
            }
        }
        Some(Ok(chats)) => {
            rsx! {
                for chat_name in chats.iter() {
                    ChatEntry { key: "{chat_name}", name: chat_name.clone() }
                }
            }
        }
        Some(Err(e)) => rsx! {
            div { class: "p-4 text-red-500 text-center", "Error: {e}" }
        },
        None => rsx! {
            div { class: "flex items-center justify-center h-full",
                div { class: "animate-spin rounded-full h-8 w-8 border-b-2 border-green-500" }
            }
        },
    };

    rsx! {
        div { class: "flex flex-col h-full bg-white",
            Header {}
            if show_name_prompt {
                div { class: "bg-yellow-50 border-b border-yellow-200 px-4 py-2 text-xs text-yellow-800 flex items-center gap-1.5",
                    span { "\u{26A0}\u{FE0F} Set your name in" }
                    button {
                        class: "underline font-medium hover:text-yellow-900",
                        onclick: move |_| { nav.push("/settings"); },
                        "Settings"
                    }
                    span { "to identify your messages" }
                }
            }
            div { class: "flex-1 overflow-y-auto",
                {list_content}
            }
        }
    }
}

#[component]
fn Header() -> Element {
    let refresh = use_context::<Signal<u32>>();
    let mut status = use_signal(|| String::new());
    let nav = use_navigator();

    // ── Import button: platform-specific ─────────────────
    #[cfg(target_os = "android")]
    let import_button = rsx! {
        button {
            class: "flex items-center gap-1.5 px-3 py-1.5 bg-white text-green-700 rounded-full text-sm font-medium hover:bg-green-50 transition-colors shadow-sm cursor-pointer",
            onclick: move |_| {
                eprintln!("[WACV] Import clicked");
                status.set("Opening JNI picker...".to_string());
                let mut s = status.clone();
                let mut r = refresh.clone();
                spawn(async move {
                    match crate::android::pick_zip_file() {
                        Ok((fname, data)) => {
                            s.set(format!("Loading {fname}..."));
                            match ChatStorage::new() {
                                Ok(storage) => match storage.import_chat(&data, &fname) {
                                    Ok(chat_name) => {
                                        eprintln!("[WACV] Imported: {chat_name}");
                                        s.set(format!("\u{2713} {chat_name} imported"));
                                        *r.write() += 1;
                                    }
                                    Err(e) => {
                                        eprintln!("[WACV] Import err: {e}");
                                        s.set(format!("Error: {e}"));
                                    }
                                },
                                Err(e) => {
                                    eprintln!("[WACV] Storage err: {e}");
                                    s.set(format!("Storage: {e}"));
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!("[WACV] JNI picker error: {e}");
                            s.set(format!("Error: {e}"));
                        }
                    }
                });
            },
            svg {
                class: "w-4 h-4",
                fill: "none",
                view_box: "0 0 24 24",
                stroke: "currentColor",
                stroke_width: "2",
                path {
                    d: "M21 15v4a2 2 0 01-2 2H5a2 2 0 01-2-2v-4M17 8l-5-5-5 5M12 3v12",
                    stroke_linecap: "round",
                    stroke_linejoin: "round"
                }
            }
            " Import"
        }
    };

    #[cfg(not(target_os = "android"))]
    let import_button = rsx! {
        button {
            class: "flex items-center gap-1.5 px-3 py-1.5 bg-white text-green-700 rounded-full text-sm font-medium hover:bg-green-50 transition-colors shadow-sm",
            onclick: move |_| {
                eprintln!("[WACV] Import clicked");
                status.set("Opening dialog...".to_string());
                let mut s = status.clone();
                let mut r = refresh.clone();
                spawn(async move {
                    let file = pick_file_dialog().await;
                    eprintln!("[WACV] pick_file returned: {:?}", file);
                    if let Some(path) = file {
                        import_zipped(path, &mut s, &mut r);
                    }
                });
            },
            svg {
                class: "w-4 h-4",
                fill: "none",
                view_box: "0 0 24 24",
                stroke: "currentColor",
                stroke_width: "2",
                path {
                    d: "M21 15v4a2 2 0 01-2 2H5a2 2 0 01-2-2v-4M17 8l-5-5-5 5M12 3v12",
                    stroke_linecap: "round",
                    stroke_linejoin: "round"
                }
            }
            "Import"
        }
    };

    rsx! {
        div { class: "sticky top-0 z-10 bg-green-600 text-white px-4 py-3 flex items-center justify-between shadow-md",
            h1 { class: "text-xl font-bold", "WACV" }
            div { class: "flex items-center gap-2",
                {import_button}
                // Settings gear
                button {
                    class: "p-2 rounded-full hover:bg-green-500 transition-colors",
                    onclick: move |_| { nav.push("/settings"); },
                    svg {
                        class: "w-5 h-5",
                        fill: "none",
                        view_box: "0 0 24 24",
                        stroke: "currentColor",
                        stroke_width: "2",
                        path {
                            d: "M10.325 4.317c.426-1.756 2.924-1.756 3.35 0a1.724 1.724 0 002.573 1.066c1.543-.94 3.31.826 2.37 2.37a1.724 1.724 0 001.066 2.573c1.756.426 1.756 2.924 0 3.35a1.724 1.724 0 00-1.066 2.573c.94 1.543-.826 3.31-2.37 2.37a1.724 1.724 0 00-2.573 1.066c-.426 1.756-2.924 1.756-3.35 0a1.724 1.724 0 00-2.573-1.066c-1.543.94-3.31-.826-2.37-2.37a1.724 1.724 0 00-1.066-2.573c-1.756-.426-1.756-2.924 0-3.35a1.724 1.724 0 001.066-2.573c-.94-1.543.826-3.31 2.37-2.37.996.608 2.296.07 2.572-1.065z",
                            stroke_linecap: "round",
                            stroke_linejoin: "round",
                        }
                        path {
                            d: "M15 12a3 3 0 11-6 0 3 3 0 016 0z",
                            stroke_linecap: "round",
                            stroke_linejoin: "round",
                        }
                    }
                }
            }
        }
        if !status().is_empty() {
            div { class: "text-center text-sm text-white bg-green-500 py-1", "{status}" }
        }
    }
}

#[component]
fn ChatEntry(name: String) -> Element {
    let nav = use_navigator();
    let name_clone = name.clone();
    let chat = use_resource(move || {
        let n = name_clone.clone();
        async move {
            ChatStorage::new()
                .ok()
                .and_then(|s| s.load_chat(&n).ok())
        }
    });

    let last_preview = use_memo(move || {
        chat.read()
            .as_ref()
            .and_then(|c| c.as_ref())
            .and_then(|c| c.last_message_preview().map(|s| s.to_string()))
            .unwrap_or_default()
    });

    let last_time = use_memo(move || {
        chat.read()
            .as_ref()
            .and_then(|c| c.as_ref())
            .and_then(|c| c.last_timestamp())
            .map(format_timestamp)
            .unwrap_or_default()
    });

    let display_name = chat
        .read()
        .as_ref()
        .and_then(|c| c.as_ref())
        .map(|c| c.display_name().to_string())
        .unwrap_or_else(|| name.clone());

    rsx! {
        div {
            class: "flex items-center px-4 py-3 hover:bg-gray-100 cursor-pointer border-b border-gray-100 transition-colors",
            onclick: move |_| {
                nav.push(format!("/chat/{}", url_encode(&name)));
            },

            // Avatar circle
            div { class: "w-12 h-12 rounded-full bg-green-500 flex items-center justify-center text-white font-bold text-lg shrink-0",
                {display_name.chars().next().unwrap_or('?').to_uppercase().to_string()}
            }

            // Name & preview
            div { class: "ml-3 flex-1 min-w-0",
                div { class: "flex items-center justify-between",
                    h3 { class: "font-medium text-gray-900 truncate", "{display_name}" }
                    span { class: "text-xs text-gray-400 shrink-0 ml-2", "{last_time}" }
                }
                p { class: "text-sm text-gray-500 truncate mt-0.5", "{last_preview}" }
            }
        }
    }
}

fn format_timestamp(ts: i64) -> String {
    let now = chrono::Utc::now().timestamp();
    let dt = chrono::DateTime::from_timestamp(ts, 0);

    match dt {
        Some(dt) => {
            let local = dt.with_timezone(&chrono::Local);
            let diff_days = (now - ts) / 86400;
            if diff_days == 0 {
                local.format("%H:%M").to_string()
            } else if diff_days < 7 {
                local.format("%a").to_string()
            } else {
                local.format("%d.%m.%y").to_string()
            }
        }
        None => String::new(),
    }
}

fn url_encode(s: &str) -> String {
    // URL-encode chat names for router navigation
    s.bytes()
        .flat_map(|b| {
            if b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.' || b == b'~' {
                vec![b as char]
            } else {
                format!("%{:02X}", b).chars().collect()
            }
        })
        .collect()
}
