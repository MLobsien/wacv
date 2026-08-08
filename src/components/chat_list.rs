use crate::storage::{ChatStorage, config::Config};
use dioxus::prelude::*;

/// Desktop: multi-file picker via zenity (rfd's GTK dialog never opens here).
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

/// Messages streamed from the import worker thread into the UI coroutine.
/// The worker only sends; all signal writes happen inside `use_coroutine`
/// (signals are not thread-safe to touch from a raw `std::thread`).
pub(crate) enum ImportMsg {
    /// A new chat import has started.
    /// A new chat import has started.
    Started { name: String },
    /// Fractional progress (0..1) for the named chat.
    Progress { name: String, fraction: f32 },
    /// A single chat finished (or failed).
    Done { name: String, ok: bool, error: Option<String> },
    /// The whole batch finished; refresh the chat list and show a summary.
    AllDone { imported: usize, failed: usize, error: Option<String> },
}

/// UI state for one chat's loading bar.
#[derive(Clone, PartialEq)]
pub(crate) struct ImportRowUi {
    pub(crate) name: String,
    pub(crate) fraction: f32,
}

/// Desktop worker entry point: import already-picked files on a background
/// thread, streaming progress back through the UI channel.
#[cfg(not(target_os = "android"))]
fn import_zip_files(tx: futures::channel::mpsc::UnboundedSender<ImportMsg>, paths: Vec<std::path::PathBuf>) {
    eprintln!("[WACV] importing {} file(s): {:?}", paths.len(), paths);
    let mut imported = 0usize;
    let mut failed = 0usize;
    for path in paths {
        eprintln!("[WACV] File: {:?}", path);
        let fname = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("chat.zip")
            .to_string();
        let _ = tx.unbounded_send(ImportMsg::Started { name: fname.clone() });
        match std::fs::read(&path) {
            Ok(data) => {
                eprintln!("[WACV] Read {}B", data.len());
                match ChatStorage::new() {
                    Ok(storage) => match storage.import_chat_with_progress(&data, &fname, &mut |p| {
                        let _ = tx.unbounded_send(ImportMsg::Progress { name: fname.clone(), fraction: p });
                    }) {
                        Ok(name) => {
                            eprintln!("[WACV] Imported: {name}");
                            imported += 1;
                            let _ = tx.unbounded_send(ImportMsg::Done { name: fname, ok: true, error: None });
                        }
                        Err(e) => {
                            eprintln!("[WACV] Import err: {e}");
                            failed += 1;
                            let _ = tx.unbounded_send(ImportMsg::Done { name: fname, ok: false, error: Some(e.to_string()) });
                        }
                    },
                    Err(e) => {
                        eprintln!("[WACV] Storage err: {e}");
                        failed += 1;
                        let _ = tx.unbounded_send(ImportMsg::Done { name: fname, ok: false, error: Some(e.to_string()) });
                    }
                }
            }
            Err(e) => {
                eprintln!("[WACV] Read err: {e}");
                failed += 1;
                let _ = tx.unbounded_send(ImportMsg::Done { name: fname, ok: false, error: Some(e.to_string()) });
            }
        }
    }
    let _ = tx.unbounded_send(ImportMsg::AllDone { imported, failed, error: None });
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
    let refresh = use_context::<Signal<u32>>();
    let import_rows = use_context::<Signal<Vec<ImportRowUi>>>();
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
                if !import_rows().is_empty() {
                    div { class: "border-b border-gray-200 dark:border-gray-800 px-4 py-2 space-y-2 bg-white dark:bg-gray-900",
                        for row in &import_rows() {
                            div { class: "flex items-center gap-3",
                                div { class: "flex-1 min-w-0",
                                    div { class: "text-sm text-gray-700 dark:text-gray-300 truncate", "{row.name}" }
                                    div { class: "mt-1 h-1.5 bg-gray-200 dark:bg-gray-700 rounded-full overflow-hidden",
                                        div {
                                            class: "h-full bg-green-500 transition-all duration-200",
                                            style: "width: {((row.fraction * 100.0) as u32)}%",
                                        }
                                    }
                                }
                                span { class: "text-xs text-gray-500 dark:text-gray-400 shrink-0 w-10 text-right", "{((row.fraction * 100.0) as u32)}%" }
                            }
                        }
                    }
                }
                {list_content}
            }
        }
    }
}

#[component]
fn Header() -> Element {
    // status is written by the Android picker (set), read-only on desktop.
    #[allow(unused_mut)]
    let mut status = use_context::<Signal<String>>();
    let nav = use_navigator();
    #[cfg(target_os = "android")]
    #[allow(unused_mut)]
    let mut import_rows = use_context::<Signal<Vec<ImportRowUi>>>();

    // The import coroutine lives at the App root (lib.rs) so its receiver
    // survives route changes; Header just sends messages through its handle.
    let import_coro = use_coroutine_handle::<ImportMsg>();

    // ── Import button: platform-specific ─────────────────
    #[cfg(target_os = "android")]
    let import_button = rsx! {
        button {
            class: "flex items-center gap-1.5 px-3 py-1.5 bg-white dark:bg-gray-800 text-green-700 dark:text-green-400 rounded-full text-sm font-medium hover:bg-green-50 dark:hover:bg-gray-700 transition-colors shadow-sm cursor-pointer",
            onclick: move |_| {
                eprintln!("[WACV] Import clicked");
                import_rows.set(Vec::new());
                status.set("Opening JNI picker...".to_string());
                let tx = import_coro.tx();
                // All file picking + import work happens on a worker thread;
                // the UI only receives ImportMsg streamed across the channel.
                std::thread::spawn(move || {
                    match crate::android::pick_zip_files() {
                        Ok(files) => {
                            let mut imported = 0usize;
                            let mut failed = 0usize;
                            let storage = match ChatStorage::new() {
                                Ok(s) => s,
                                Err(e) => {
                                    eprintln!("[WACV] Storage err: {e}");
                                    let _ = tx.unbounded_send(ImportMsg::AllDone {
                                        imported: 0,
                                        failed: 1,
                                        error: Some(e.to_string()),
                                    });
                                    return;
                                }
                            };
                            for (fname, uri) in files {
                                eprintln!("[WACV] Importing {fname}...");
                                let _ = tx.unbounded_send(ImportMsg::Started { name: fname.clone() });
                                // Stage the archive on disk first (only ever
                                // holds a 64 KiB chunk in RAM while copying),
                                // then stream the import straight from the file.
                                let tmp = crate::android::temp_import_path(&fname);
                                let result = crate::android::copy_uri_content_to_file(&uri, &tmp, &mut |p| {
                                    // Download phase: first 10% of the bar.
                                    let _ = tx.unbounded_send(ImportMsg::Progress { name: fname.clone(), fraction: 0.10 * p });
                                })
                                .map_err(|e| anyhow::anyhow!("stage {fname}: {e}"))
                                .and_then(|_| {
                                    // Import phase: remaining 90% of the bar.
                                    storage.import_chat_file_with_progress(&tmp, &fname, &mut |f| {
                                        let _ = tx.unbounded_send(ImportMsg::Progress { name: fname.clone(), fraction: 0.10 + 0.90 * f });
                                    })
                                });
                                let _ = std::fs::remove_file(&tmp);
                                match result {
                                    Ok(chat_name) => {
                                        eprintln!("[WACV] Imported: {chat_name}");
                                        imported += 1;
                                        let _ = tx.unbounded_send(ImportMsg::Done { name: fname, ok: true, error: None });
                                    }
                                    Err(e) => {
                                        eprintln!("[WACV] Import err: {e}");
                                        failed += 1;
                                        let _ = tx.unbounded_send(ImportMsg::Done { name: fname, ok: false, error: Some(e.to_string()) });
                                    }
                                }
                            }
                            let _ = tx.unbounded_send(ImportMsg::AllDone { imported, failed, error: None });
                        }
                        Err(e) => {
                            eprintln!("[WACV] JNI picker error: {e}");
                            let _ = tx.unbounded_send(ImportMsg::AllDone { imported: 0, failed: 0, error: Some(e.to_string()) });
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
            class: "flex items-center gap-1.5 px-3 py-1.5 bg-white dark:bg-gray-800 text-green-700 dark:text-green-400 rounded-full text-sm font-medium hover:bg-green-50 dark:hover:bg-green-700 transition-colors shadow-sm cursor-pointer",
            onclick: move |_| {
                // zenity blocks until the dialog closes; run the pick plus
                // the imports on a worker thread and stream progress back
                // through the coroutine channel. Signals are only touched
                // on the dioxus thread inside the coroutine.
                let tx = import_coro.tx();
                std::thread::spawn(move || {
                    let paths = pick_zip_files_dialog();
                    if paths.is_empty() {
                        // Cancelled: tell the coroutine to clear the status.
                        let _ = tx.unbounded_send(ImportMsg::AllDone { imported: 0, failed: 0, error: None });
                        return;
                    }
                    import_zip_files(tx, paths);
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
