use crate::storage::{ChatStorage, config::Config};
use dioxus::prelude::*;

/// Desktop: multi-file picker via zenity.
#[cfg(not(target_os = "android"))]
fn pick_zip_files_dialog() -> Vec<std::path::PathBuf> {
    eprintln!("[WACV] Opening zenity multi-file dialog...");
    let output = std::process::Command::new("zenity")
        .arg("--file-selection")
        .arg("--multiple")
        .arg("--title")
        .arg("Select WhatsApp chat export(s)")
        .arg("--file-filter")
        .arg("ZIP files (*.zip) | *.zip")
        .output();
    match output {
        Ok(out) if out.status.success() => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let paths: Vec<std::path::PathBuf> = stdout
                .split('|')
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .map(std::path::PathBuf::from)
                .collect();
            eprintln!("[WACV] zenity result: {:?}", paths);
            paths
        }
        _ => {
            eprintln!("[WACV] zenity cancelled or unavailable");
            Vec::new()
        }
    }
}

#[cfg(not(target_os = "android"))]
fn import_zipped_many(
    paths: Vec<std::path::PathBuf>,
    status: &mut Signal<String>,
    refresh: &mut Signal<u32>,
) {
    let mut imported = 0;
    let mut failed = 0;
    for path in paths {
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
                            imported += 1;
                        }
                        Err(e) => {
                            eprintln!("[WACV] Import err: {}", e);
                            failed += 1;
                        }
                    },
                    Err(e) => {
                        eprintln!("[WACV] Storage err: {}", e);
                        failed += 1;
                    }
                }
            }
            Err(e) => {
                eprintln!("[WACV] Read err: {}", e);
                failed += 1;
            }
        }
    }
    *refresh.write() += 1;
    if failed == 0 {
        status.set(format!("\u{2713} {imported} imported"));
    } else if imported > 0 {
        status.set(format!("\u{2713} {imported} imported, {failed} failed"));
    } else {
        status.set(format!("Error: import failed"));
    }
}

async fn get_chat_list() -> Result<Vec<(String, i64)>, String> {
    let storage = ChatStorage::new().map_err(|e| format!("{:?}", e))?;
    storage
        .list_chats_with_last_timestamp()
        .map_err(|e| format!("{:?}", e))
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
    let mut search = use_signal(|| String::new());

    // Filter and sort the chat list. Sorting depends on the config setting,
    // filtering on the search box; both re-run reactively.
    let visible_chats = use_memo(move || {
        let list = chat_list.read();
        let Some(Ok(chats)) = list.as_ref() else {
            return Vec::new();
        };
        let mut chats = chats.clone();
        let sort = config.read().chat_sort;
        match sort {
            crate::storage::config::ChatSort::ByTime => {
                // Newest last-message first.
                chats.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
            }
            crate::storage::config::ChatSort::Alphabetical => {
                chats.sort_by(|a, b| a.0.cmp(&b.0));
            }
        }
        let query = search().trim().to_lowercase();
        if !query.is_empty() {
            chats.retain(|(name, _)| name.to_lowercase().contains(&query));
        }
        chats
    });

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
                div { class: "flex flex-col items-center justify-center h-full text-gray-400 dark:text-gray-500 p-8 text-center",
                    svg {
                        class: "w-16 h-16 mb-4 text-gray-300 dark:text-gray-600",
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
        Some(Ok(_)) => {
            let visible = visible_chats();
            if visible.is_empty() {
                rsx! {
                    div { class: "flex items-center justify-center h-full text-gray-400 dark:text-gray-500 p-8 text-center",
                        p { class: "text-sm", "No chats match your search" }
                    }
                }
            } else {
                rsx! {
                    for (chat_name, _) in visible.iter() {
                        ChatEntry { key: "{chat_name}", name: chat_name.clone() }
                    }
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
        div { class: "flex flex-col h-full bg-white dark:bg-gray-900",
            Header {}
            if show_name_prompt {
                div { class: "bg-yellow-50 dark:bg-yellow-900/40 border-b border-yellow-200 dark:border-yellow-800 px-4 py-2 text-xs text-yellow-800 dark:text-yellow-200 flex items-center gap-1.5",
                    span { "\u{26A0}\u{FE0F} Set your name in" }
                    button {
                        class: "underline font-medium hover:text-yellow-900 dark:hover:text-yellow-100",
                        onclick: move |_| { nav.push("/settings"); },
                        "Settings"
                    }
                    span { "to identify your messages" }
                }
            }
            // Search box
            if has_chats {
                div { class: "px-4 py-2 border-b border-gray-200 dark:border-gray-800",
                    div { class: "relative",
                        svg {
                            class: "absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-gray-400 dark:text-gray-500",
                            fill: "none",
                            view_box: "0 0 24 24",
                            stroke: "currentColor",
                            stroke_width: "2",
                            path {
                                d: "M21 21l-4.35-4.35M17 11a6 6 0 11-12 0 6 6 0 0112 0z",
                                stroke_linecap: "round",
                                stroke_linejoin: "round",
                            }
                        }
                        input {
                            class: "w-full pl-9 pr-8 py-2 bg-gray-100 dark:bg-gray-800 rounded-lg text-sm focus:outline-none focus:ring-2 focus:ring-green-500 focus:border-green-500 dark:text-gray-100",
                            placeholder: "Search chats",
                            value: search(),
                            oninput: move |e| { search.set(e.value()); },
                        }
                        if !search().is_empty() {
                            button {
                                class: "absolute right-2 top-1/2 -translate-y-1/2 p-1 text-gray-400 hover:text-gray-600 dark:hover:text-gray-300",
                                onclick: move |_| search.set(String::new()),
                                svg {
                                    class: "w-3.5 h-3.5",
                                    fill: "none",
                                    view_box: "0 0 24 24",
                                    stroke: "currentColor",
                                    stroke_width: "2",
                                    path {
                                        d: "M6 18L18 6M6 6l12 12",
                                        stroke_linecap: "round",
                                    }
                                }
                            }
                        }
                    }
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
            class: "flex items-center gap-1.5 px-3 py-1.5 bg-white dark:bg-gray-800 text-green-700 dark:text-green-400 rounded-full text-sm font-medium hover:bg-green-50 dark:hover:bg-gray-700 transition-colors shadow-sm cursor-pointer",
            onclick: move |_| {
                eprintln!("[WACV] Import clicked");
                status.set("Opening JNI picker...".to_string());
                let mut s = status.clone();
                let mut r = refresh.clone();
                spawn(async move {
                    match crate::android::pick_zip_files() {
                        Ok(files) => {
                            let storage = match ChatStorage::new() {
                                Ok(s) => s,
                                Err(e) => {
                                    eprintln!("[WACV] Storage err: {e}");
                                    s.set(format!("Storage: {e}"));
                                    return;
                                }
                            };
                            let mut imported = 0;
                            let mut failed = 0;
                            for (fname, data) in files {
                                s.set(format!("Importing {fname}..."));
                                match storage.import_chat(&data, &fname) {
                                    Ok(chat_name) => {
                                        eprintln!("[WACV] Imported: {chat_name}");
                                        imported += 1;
                                    }
                                    Err(e) => {
                                        eprintln!("[WACV] Import err: {e}");
                                        failed += 1;
                                    }
                                }
                            }
                            *r.write() += 1;
                            if failed == 0 {
                                s.set(format!("\u{2713} {imported} imported"));
                            } else if imported > 0 {
                                s.set(format!("\u{2713} {imported} imported, {failed} failed"));
                            } else {
                                s.set(format!("Error: import failed"));
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
            class: "flex items-center gap-1.5 px-3 py-1.5 bg-white dark:bg-gray-800 text-green-700 dark:text-green-400 rounded-full text-sm font-medium hover:bg-green-50 dark:hover:bg-gray-700 transition-colors shadow-sm",
            onclick: move |_| {
                eprintln!("[WACV] Import clicked");
                status.set("Opening dialog...".to_string());
                let mut s = status.clone();
                let mut r = refresh.clone();
                spawn(async move {
                    // zenity blocks the calling thread; run it off the Dioxus
                    // runtime so the UI stays responsive.
                    let (tx, rx) = futures::channel::oneshot::channel();
                    std::thread::spawn(move || {
                        let paths = pick_zip_files_dialog();
                        let _ = tx.send(paths);
                    });
                    let paths = rx.await.unwrap_or_default();
                    eprintln!("[WACV] pick_zip_files returned: {:?}", paths);
                    if !paths.is_empty() {
                        import_zipped_many(paths, &mut s, &mut r);
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
            class: "flex items-center px-4 py-3 hover:bg-gray-100 dark:hover:bg-gray-800 cursor-pointer border-b border-gray-100 dark:border-gray-800 transition-colors",
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
                    h3 { class: "font-medium text-gray-900 dark:text-gray-100 truncate", "{display_name}" }
                    span { class: "text-xs text-gray-400 dark:text-gray-500 shrink-0 ml-2", "{last_time}" }
                }
                p { class: "text-sm text-gray-500 dark:text-gray-400 truncate mt-0.5", "{last_preview}" }
            }
        }
    }
}

fn format_timestamp(ts: i64) -> String {
    // Stored timestamps are local wall-clock interpreted as UTC; compare
    // against the current local wall-clock interpreted the same way.
    let now = chrono::Local::now().naive_local().and_utc().timestamp();
    let dt = chrono::DateTime::from_timestamp(ts, 0);

    match dt {
        Some(dt) => {
            let diff_days = (now - ts) / 86400;
            if diff_days == 0 {
                dt.format("%H:%M").to_string()
            } else if diff_days < 7 {
                dt.format("%a").to_string()
            } else {
                dt.format("%d.%m.%y").to_string()
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
