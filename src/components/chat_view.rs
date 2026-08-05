use crate::storage::{CallInfo, CallKind, ChatStorage, MessageKind, VoteOption};
use crate::storage::config::Config;
use chrono::{DateTime, Local};
use std::rc::Rc;
use dioxus::html::geometry::PixelsVector2D;
use dioxus::prelude::*;

#[component]
pub fn ChatView(name: String) -> Element {
    let nav = use_navigator();
    // `name` is already percent-decoded by the dioxus router (each route
    // segment is decoded during FromStr parsing), so no manual decode here.
    let media_chat_name = name.clone();
    let menu_name = name.clone();
    let chat = use_resource(move || {
        let n = name.clone();
        async move { ChatStorage::new().ok().and_then(|s| s.load_chat(&n).ok()) }
    });
    let config = use_context::<Signal<Config>>();
    // Per-chat menu state (delete chat)
    let mut menu_open = use_signal(|| false);
    let mut confirm_delete = use_signal(|| false);
    // Scroll container reference; auto-scroll to the newest message once loaded
    let mut messages_area = use_signal(|| None::<Rc<MountedData>>);
    use_effect(move || {
        let loaded = chat.read().as_ref().map_or(false, |c| c.is_some());
        if !loaded {
            return;
        }
        let Some(area) = messages_area.cloned() else { return };
        spawn(async move {
            if let Ok(size) = area.get_scroll_size().await {
                let _ = area
                    .scroll(
                        PixelsVector2D::new(0.0, size.height),
                        ScrollBehavior::Instant,
                    )
                    .await;
            }
        });
    });
    // Compute header content outside rsx!
    let header_content: Element = match &*chat.read() {
        Some(Some(c)) => {
            rsx! {
                div { class: "flex items-center gap-2 flex-1 min-w-0",
                    div { class: "w-9 h-9 rounded-full bg-green-400 flex items-center justify-center text-white font-bold text-sm shrink-0",
                        {c.display_name().chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or("?".to_string())}
                    }
                    div { class: "min-w-0",
                        h2 { class: "font-semibold text-sm leading-tight truncate", "{c.display_name()}" }
                        p { class: "text-xs text-green-200 truncate", "WhatsApp Chat" }
                }
            }
        }
        }
        Some(None) => rsx! {
            div { class: "text-white", "Chat not found" }
        },
        None => rsx! {
            div { class: "flex items-center gap-2",
                div { class: "w-9 h-9 rounded-full bg-green-400 animate-pulse" }
                div {
                    div { class: "h-4 w-24 bg-green-500 rounded animate-pulse" }
                    div { class: "h-3 w-16 bg-green-500 rounded mt-1 animate-pulse" }
                }
            }
        },
    };

    // Compute messages content outside rsx!
    let messages_content: Element = match &*chat.read() {
        Some(Some(c)) => {
            let mut last_sender: Option<String> = None;
            let mut last_ts: i64 = 0;
            let mut items: Vec<Element> = Vec::new();
            let my_name = config.read().user_name.clone().or_else(|| c.my_name());

            // Render oldest first. Date and sender grouping compare against the
            // chronologically previous (older) message; the scroll container is
            // auto-scrolled to the bottom so the newest message is visible.
            for (i, msg) in c.messages.iter().enumerate() {
                // Show date separator if new day (compared to previous message)
                let show_date = if i > 0 {
                    day_changed(c.messages[i - 1].timestamp, msg.timestamp)
                } else {
                    true
                };

                if show_date {
                    items.push(rsx! {
                        DateSeparator { date: format_date(msg.timestamp) }
                    });
                }

                // Show sender name for group chats
                let show_sender = msg.sender.as_deref() != last_sender.as_deref()
                    || msg.timestamp - last_ts > 300;

                last_sender = msg.sender.clone();
                last_ts = msg.timestamp;
                match &msg.kind {
                    MessageKind::System(text) => {
                        items.push(rsx! {
                            SystemMessage { text: text.clone() }
                        });
                    }
                    MessageKind::EncryptionNotice => {
                        // Hide encryption notice - it's in every chat
                    }
                    MessageKind::Text { content, edited } => {
                        let is_mine = my_name.as_deref().map_or(false, |my| {
                            msg.sender.as_deref().map(crate::storage::chat::normalize_sender) == Some(my)
                        });
                        let time = format_msg_time(msg.timestamp);
                        let sender = show_sender
                            .then(|| msg.sender.clone())
                            .flatten()
                            .map(|s| crate::storage::chat::normalize_sender(&s).to_string());
                        items.push(rsx! {
                            MessageBubble {
                                is_mine,
                                sender,
                                content: content.clone(),
                                edited: *edited,
                                time,
                            }
                        });
                    }
                    MessageKind::Call(call) => {
                        let is_mine = my_name.as_deref().map_or(false, |my| {
                            msg.sender.as_deref().map(crate::storage::chat::normalize_sender) == Some(my)
                        });
                        items.push(rsx! {
                            CallCard {
                                is_mine,
                                sender: msg.sender
                                    .clone()
                                    .map(|s| crate::storage::chat::normalize_sender(&s).to_string())
                                    .unwrap_or_default(),
                                info: call.clone(),
                                time: format_msg_time(msg.timestamp),
                            }
                        });
                    }
                    MessageKind::Media { filename, caption } => {
                        let is_mine = my_name.as_deref().map_or(false, |my| {
                            msg.sender.as_deref().map(crate::storage::chat::normalize_sender) == Some(my)
                        });
                        // Show sender name for group chats (same rule as text messages)
                        let sender = show_sender
                            .then(|| msg.sender.clone())
                            .flatten()
                            .map(|s| crate::storage::chat::normalize_sender(&s).to_string());
                        let file_chat_name = media_chat_name.clone();
                        items.push(rsx! {
                            MediaMessage {
                                is_mine,
                                sender,
                                chat_name: file_chat_name.clone(),
                                filename: filename.clone(),
                                caption: caption.clone(),
                                time: format_msg_time(msg.timestamp),
                            }
                        });
                    }
                    MessageKind::Deleted { by_sender } => {
                        let is_mine = *by_sender;
                        // Show sender name for group chats (same rule as text messages)
                        let sender = show_sender
                            .then(|| msg.sender.clone())
                            .flatten()
                            .map(|s| crate::storage::chat::normalize_sender(&s).to_string());
                        items.push(rsx! {
                            DeletedMessage { is_mine, sender }
                        });
                    }
                    MessageKind::Voting { question, options } => {
                        let is_mine = my_name.as_deref().map_or(false, |my| {
                            msg.sender.as_deref().map(crate::storage::chat::normalize_sender) == Some(my)
                        });
                        items.push(rsx! {
                            VotingMessage {
                                is_mine,
                                question: question.clone(),
                                options: options.clone(),
                                time: format_msg_time(msg.timestamp),
                            }
                        });
                    }
                }
            }

            rsx! {
                {items.into_iter()}
            }
        }
        Some(None) => rsx! {
            div { class: "flex items-center justify-center h-full text-gray-400 dark:text-gray-500",
                "Chat could not be loaded"
            }
        },
        None => rsx! {
            div { class: "flex items-center justify-center h-full",
                div { class: "animate-spin rounded-full h-8 w-8 border-b-2 border-green-500" }
            }
        },
    };

    rsx! {
        div { class: "flex flex-col h-full bg-gray-100 dark:bg-gray-900",
            // Chat header
            div { class: "sticky top-0 z-10 bg-green-600 text-white px-2 py-2 flex items-center gap-2 shadow-md",
                button {
                    class: "p-2 rounded-full hover:bg-green-500 transition-colors",
                    onclick: move |_| nav.go_back(),
                    svg {
                        class: "w-6 h-6",
                        fill: "none",
                        view_box: "0 0 24 24",
                        stroke: "currentColor",
                        stroke_width: "2",
                        path {
                            d: "M15 18l-6-6 6-6",
                            stroke_linecap: "round",
                            stroke_linejoin: "round",
                        }
                    }
                }
                {header_content}
                div { class: "flex-1" }
                // Per-chat menu (⋮)
                div { class: "relative",
                    button {
                        class: "p-2 rounded-full hover:bg-green-500 transition-colors",
                        onclick: move |_| {
                            menu_open.set(!menu_open());
                            confirm_delete.set(false);
                        },
                        svg {
                            class: "w-6 h-6",
                            fill: "none",
                            view_box: "0 0 24 24",
                            stroke: "currentColor",
                            stroke_width: "2",
                            path {
                                d: "M12 5v.01M12 12v.01M12 19v.01M12 6a1 1 0 110-2 1 1 0 010 2zm0 7a1 1 0 110-2 1 1 0 010 2zm0 7a1 1 0 110-2 1 1 0 010 2z",
                                stroke_linecap: "round",
                                stroke_linejoin: "round",
                            }
                        }
                    }
                    if menu_open() {
                        div {
                            class: "bg-white dark:bg-gray-800 border border-gray-200 dark:border-gray-700 rounded-lg overflow-hidden",
                            style: "position:absolute; right:0; top:100%; margin-top:4px; z-index:20; min-width:160px; box-shadow:0 4px 12px rgba(0,0,0,0.15);",
                            button {
                                style: "display:flex; align-items:center; gap:8px; width:100%; color:#dc2626; cursor:pointer; padding:12px 16px; font-weight:500; background:none; border:none; text-align:left;",
                                onclick: move |_| {
                                    if !confirm_delete() {
                                        confirm_delete.set(true);
                                    } else {
                                        // Two-step confirmation: delete chat + its media, then go back.
                                        eprintln!("[WACV] Deleting chat: {menu_name}");
                                        let n = menu_name.clone();
                                        match ChatStorage::new() {
                                            Ok(storage) => match storage.delete_chat(&n) {
                                                Ok(()) => eprintln!("[WACV] Deleted chat: {n}"),
                                                Err(e) => eprintln!("[WACV] Delete err: {e}"),
                                            },
                                            Err(e) => eprintln!("[WACV] Storage err: {e}"),
                                        }
                                        nav.go_back();
                                    }
                                },
                                svg {
                                    class: "w-4 h-4 shrink-0",
                                    fill: "none",
                                    view_box: "0 0 24 24",
                                    stroke: "currentColor",
                                    stroke_width: "2",
                                    path {
                                        d: "M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16",
                                        stroke_linecap: "round",
                                        stroke_linejoin: "round",
                                }
                                }
                                span {
                                    style: "white-space:nowrap; font-size:14px;",
                                    {if confirm_delete() { "Confirm delete?" } else { "Delete chat" }}
                                }
                            }
                        }
                    }
                }
            }

            // Messages area
            div { class: "flex-1 overflow-y-auto px-3 py-2",
                onmounted: move |e| messages_area.set(Some(e.data())),
                {messages_content}
            }
        }
    }
}

/// Show a date separator line (like "Today", "Yesterday", or date)
#[component]
fn DateSeparator(date: String) -> Element {
    rsx! {
        div { class: "flex justify-center my-2",
            span { class: "text-xs text-gray-500 dark:text-gray-400 bg-white/80 dark:bg-gray-800/80 px-2 py-1 rounded shadow-sm", "{date}" }
        }
    }
}

/// System message (centered, small)
#[component]
fn SystemMessage(text: String) -> Element {
    rsx! {
        div { class: "flex justify-center my-2",
            span {
                class: "text-xs text-gray-500 dark:text-gray-400 italic bg-white/60 dark:bg-gray-800/60 px-3 py-1.5 rounded-lg max-w-xs text-center leading-relaxed [overflow-wrap:anywhere]",
                for part in split_links(&text) {
                    match part {
                        TextPart::Text(s) => rsx! { "{s}" },
                        TextPart::Link(href) => rsx! {
                            a {
                                href: "{href}",
                                class: "text-blue-600 dark:text-blue-400 underline break-all",
                                "{href}"
                            }
                        },
                    }
                }
            }
        }
    }
}

/// Regular message bubble
#[component]
fn MessageBubble(
    is_mine: bool,
    sender: Option<String>,
    content: String,
    edited: bool,
    time: String,
) -> Element {
    let bubble_class = if is_mine {
        "bg-[#d9fdd3] dark:bg-[#005c4b] rounded-[8px_0_8px_8px] self-end"
    } else {
        "bg-white dark:bg-[#202c33] rounded-[0_8px_8px_8px] self-start"
    };

    let container_class = if is_mine { "items-end" } else { "items-start" };

    rsx! {
        div { class: "flex flex-col {container_class} mb-1.5",
            // Sender name (for group chats, other people)
            if let Some(s) = sender {
                span { class: "text-xs text-gray-500 dark:text-gray-400 ml-1 mb-0.5 font-medium", "{s}" }
            }

            div { class: "max-w-[75%] shadow-sm {bubble_class} px-3 py-1.5",
                // Message content - render newlines as <br>
                div { class: "text-sm text-gray-900 dark:text-gray-100 whitespace-pre-wrap break-words",
                    for part in split_links(&content) {
                        match part {
                            TextPart::Text(s) => rsx! { "{s}" },
                            TextPart::Link(href) => rsx! {
                                a {
                                    href: "{href}",
                                    class: "text-blue-600 dark:text-blue-400 underline break-all",
                                    "{href}"
                                }
                            },
                        }
                    }
                }
                // Footer: edited badge + time
                div { class: "flex items-center justify-end gap-1 mt-0.5",
                    if edited {
                        span { class: "text-[10px] text-gray-400 italic", "edited" }
                    }
                    span { class: "text-[10px] text-gray-400", "{time}" }
                    if is_mine {
                        DoubleCheck {}
                    }
                }
            }
        }
    }
}

/// Two blue check marks shown on sent (is_mine) messages
#[component]
fn DoubleCheck() -> Element {
    rsx! {
        svg {
            class: "w-3.5 h-3.5 text-blue-500 dark:text-blue-400",
            view_box: "0 0 16 11",
            fill: "currentColor",
            path { d: "M11.071.653a.457.457 0 00-.304-.102.493.493 0 00-.381.178l-6.19 7.636-2.011-2.095a.463.463 0 00-.336-.153.457.457 0 00-.335.128.51.51 0 00-.14.32.484.484 0 00.14.345l2.394 2.493a.539.539 0 00.16.12.44.44 0 00.186.03.492.492 0 00.37-.184l6.55-8.084a.482.482 0 00.127-.319.5.5 0 00-.15-.345l-.102-.102z" }
            path { d: "M14.931.653a.457.457 0 00-.305-.102.493.493 0 00-.381.178l-6.19 7.636-1.058-1.102a.442.442 0 00-.121-.099.538.538 0 00-.079-.035.398.398 0 00-.286.019.47.47 0 00-.184.145.47.47 0 00-.098.185.475.475 0 00.01.216.44.44 0 00.074.153l1.441 1.502a.539.539 0 00.16.12.44.44 0 00.186.03.492.492 0 00.37-.184l6.55-8.084a.482.482 0 00.127-.319.5.5 0 00-.15-.345l-.102-.102z" }
        }
    }
}

/// Call information card
#[component]
fn CallCard(is_mine: bool, sender: String, info: CallInfo, time: String) -> Element {
    let container_class = if is_mine { "items-end" } else { "items-start" };
    let text_color = if is_mine {
        "text-green-700 dark:text-green-400"
    } else {
        "text-red-500 dark:text-red-400"
    };

    let call_label = match info.kind {
        CallKind::Missed => {
            if is_mine {
                "Outgoing call"
            } else {
                "Missed call"
            }
        }
        CallKind::Outgoing => "Outgoing call",
        CallKind::Incoming => "Incoming call",
    };

    let duration_text = info
        .duration_secs
        .map(|s| {
            if s >= 60 {
                format!("{} min", s / 60)
            } else {
                format!("{} sec", s)
            }
        })
        .unwrap_or_default();

    rsx! {
        div { class: "flex flex-col {container_class} mb-1.5",
            div { class: "max-w-[75%] bg-white dark:bg-gray-800 rounded-lg shadow-sm px-4 py-2 flex items-center gap-3",
                // Phone icon
                svg {
                    class: "w-5 h-5 shrink-0 {text_color}",
                    fill: "none",
                    view_box: "0 0 24 24",
                    stroke: "currentColor",
                    stroke_width: "2",
                    path {
                        d: "M22 16.92v3a2 2 0 01-2.18 2 19.79 19.79 0 01-8.63-3.07 19.5 19.5 0 01-6-6 19.79 19.79 0 01-3.07-8.67A2 2 0 014.11 2h3a2 2 0 012 1.72 12.84 12.84 0 00.7 2.81 2 2 0 01-.45 2.11L8.09 9.91a16 16 0 006 6l1.27-1.27a2 2 0 012.11-.45 12.84 12.84 0 002.81.7A2 2 0 0122 16.92z",
                        stroke_linecap: "round",
                        stroke_linejoin: "round",
                    }
                }
                div {
                    p { class: "text-sm font-medium text-gray-900 dark:text-gray-100", "{call_label}" }
                    if !duration_text.is_empty() {
                        p { class: "text-xs text-gray-500 dark:text-gray-400", "{duration_text}" }
                    }
                }
                span { class: "text-[10px] text-gray-400 ml-auto shrink-0", "{time}" }
            }
        }
    }
}

/// Media message (image, video, audio, sticker)
#[component]
fn MediaMessage(
    is_mine: bool,
    sender: Option<String>,
    chat_name: String,
    filename: String,
    caption: Option<String>,
    time: String,
) -> Element {
    let is_sticker = filename.contains("STICKER");
    let bubble_class = if is_sticker {
        // Stickers render without a bubble background
        if is_mine { "self-end" } else { "self-start" }
    } else if is_mine {
        "bg-[#d9fdd3] dark:bg-[#005c4b] rounded-[8px_0_8px_8px] self-end shadow-sm"
    } else {
        "bg-white dark:bg-[#202c33] rounded-[0_8px_8px_8px] self-start shadow-md"
    };
    let container_class = if is_mine { "items-end" } else { "items-start" };

    let port = crate::media_port();
    let media_uri = format!("http://127.0.0.1:{}/{}/{}", port, crate::url_encode(&chat_name), crate::url_encode(&filename));
    let is_image = filename.contains("PHOTO")
        || filename.contains("IMAGE")
        || filename.contains(".jpg")
        || filename.contains(".png")
        || filename.contains(".webp");
    let is_video = filename.contains("VIDEO") || filename.contains(".mp4") || filename.contains(".mov");
    let is_audio =
        filename.contains("AUDIO") || filename.contains(".opus") || filename.contains(".ogg");
    let audio_bg = if is_mine { "bg-green-100 dark:bg-green-900" } else { "bg-gray-50 dark:bg-gray-800" };
    // Image lightbox (open when the user clicks a photo/sticker)
    let mut lightbox = use_signal(|| false);
    // Track image load state. When an image fails, the browser renders a tiny
    // broken-image placeholder; with `w-min` on the bubble that placeholder
    // becomes the min-content width and squashes the caption to a few
    // characters. So on failure we drop `w-min` and let the text size the
    // bubble instead.
    let mut img_failed = use_signal(|| false);
    let bubble_width_class = if img_failed() { "max-w-[75%]" } else { "w-min max-w-[75%]" };

    rsx! {
        div { class: "flex flex-col {container_class} mb-1.5",
            // Sender name (for group chats, other people)
            if let Some(s) = sender {
                span { class: "text-xs text-gray-500 dark:text-gray-400 ml-1 mb-0.5 font-medium", "{s}" }
            }

            div { class: "{bubble_width_class} overflow-hidden {bubble_class}",
                if is_sticker {
                    img {
                        src: "{media_uri}",
                        class: "w-40 h-40 sm:w-64 sm:h-64 max-w-none object-contain cursor-pointer rounded-lg",
                        onclick: move |_| lightbox.set(true),
                        alt: "",
                    }
                } else if is_image {
                    if img_failed() {
                        // Image could not be loaded: show a small placeholder
                        // that does not constrain the bubble width, so the
                        // caption text sizes the bubble instead.
                        div { class: "p-2",
                            svg { class: "w-5 h-5 text-gray-400 dark:text-gray-500", fill: "none", view_box: "0 0 24 24", stroke: "currentColor", stroke_width: "2",
                                path { d: "M3 5h18v14H3zM3 15l5-5 4 4 3-3 6 6" }
                            }
                        }
                    } else {
                        div { class: "p-1",
                            img {
                                src: "{media_uri}",
                                class: "max-w-[50vw] max-h-[50vh] w-auto h-auto object-contain cursor-pointer",
                                onclick: move |_| lightbox.set(true),
                                onerror: move |_| img_failed.set(true),
                                alt: "",
                            }
                        }
                    }
                } else if is_video {
                    div { class: "p-1",
                        video {
                            src: "{media_uri}",
                            controls: true,
                            class: "max-w-[50vw] max-h-[50vh] w-auto h-auto",
                        }
                    }
                } else if is_audio {
                    div { class: "flex items-center gap-2 px-3 py-2 min-w-[200px] {audio_bg}",
                        audio {
                            src: "{media_uri}",
                            controls: true,
                            class: "h-8 accent-green-600",
                        }
                    }
                } else {
                    // Generic document
                    div { class: "flex items-center gap-2 px-3 py-2",
                        svg { class: "w-8 h-8 text-gray-400 dark:text-gray-500 shrink-0", fill: "none", view_box: "0 0 24 24", stroke: "currentColor", stroke_width: "2",
                            path { d: "M7 21h10a2 2 0 002-2V9.414a1 1 0 00-.293-.707l-5.414-5.414A1 1 0 0012.586 3H7a2 2 0 00-2 2v14a2 2 0 002 2z" }
                        }
                        div { class: "min-w-0",
                            p { class: "text-xs text-gray-600 dark:text-gray-300 truncate", "{filename}" }
                        }
                    }
                }
                // Caption text (media sent together with a message)
                if let Some(cap) = caption.as_ref() {
                    if !cap.is_empty() {
                        div { class: "px-2 py-1",
                            p { class: "text-sm text-gray-700 dark:text-gray-200 whitespace-pre-wrap [overflow-wrap:anywhere]",
                                for part in split_links(cap) {
                                    match part {
                                        TextPart::Text(s) => rsx! { "{s}" },
                                        TextPart::Link(href) => rsx! {
                                            a {
                                                href: "{href}",
                                                class: "text-blue-600 dark:text-blue-400 underline break-all",
                                                "{href}"
                                            }
                                        },
                                    }
                                }
                            }
                        }
                    }
                }
                // Time footer
                div { class: "px-2 py-1 flex justify-end",
                    span { class: "text-[10px] text-gray-400", "{time}" }
                    if is_mine {
                        DoubleCheck {}
                    }
                }
            }
        }
        if lightbox() {
            ImageModal {
                uri: media_uri.clone(),
                caption: caption.clone(),
                on_close: move |_| lightbox.set(false),
            }
        }
    }
}

/// Full-screen lightbox for a clicked image/sticker
#[component]
fn ImageModal(uri: String, caption: Option<String>, on_close: EventHandler<()>) -> Element {

    let c1 = on_close.clone();

    rsx! {
        div {
            id: "image-modal",
            class: "fixed inset-0 z-50 bg-black/90 flex items-center justify-center",
            tabindex: "0",
            autofocus: true,
            onclick: move |_| on_close.call(()),
            onkeydown: move |e| {
                if e.key() == Key::Escape {
                    on_close.call(());
                }
            },
            // Close button (top-right)
            button {
                class: "absolute top-4 right-4 text-white/80 hover:text-white p-2 rounded-full hover:bg-white/10 transition-colors",
                onclick: move |e| {
                    e.stop_propagation();
                    c1.call(());
                },
                svg {
                    class: "w-6 h-6",
                    fill: "none",
                    view_box: "0 0 24 24",
                    stroke: "currentColor",
                    stroke_width: "2",
                    path { d: "M6 18L18 6M6 6l12 12", stroke_linecap: "round", stroke_linejoin: "round" }
                }
            }
            // Full-size image
            img {
                src: "{uri}",
                class: "max-w-[95vw] max-h-[85vh] object-contain rounded-lg shadow-2xl",
                onclick: move |e| e.stop_propagation(),
            }
            // Caption below the image
            if let Some(c) = caption {
                if !c.is_empty() {
                    p {
                        class: "absolute bottom-6 left-1/2 -translate-x-1/2 text-white/90 text-sm whitespace-pre-wrap [overflow-wrap:anywhere] max-w-[85vw] text-center",
                        "{c}"
                    }
                }
            }
        }
    }
}

/// Deleted message placeholder
#[component]
fn DeletedMessage(is_mine: bool, sender: Option<String>) -> Element {
    let container_class = if is_mine { "items-end" } else { "items-start" };
    let text = if is_mine {
        "You deleted this message"
    } else {
        "This message was deleted"
    };

    rsx! {
        div { class: "flex flex-col {container_class} mb-1.5",
            // Sender name (for group chats, other people)
            if let Some(s) = sender {
                span { class: "text-xs text-gray-500 dark:text-gray-400 ml-1 mb-0.5 font-medium", "{s}" }
            }
            div { class: "max-w-[75%] bg-gray-100 dark:bg-gray-800 rounded-lg px-3 py-2 italic text-gray-400 dark:text-gray-500 text-sm border border-gray-200 dark:border-gray-700",
                svg {
                    class: "w-3.5 h-3.5 inline mr-1 -mt-0.5",
                    fill: "none",
                    view_box: "0 0 24 24",
                    stroke: "currentColor",
                    stroke_width: "2",
                    path {
                        d: "M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16",
                        stroke_linecap: "round", stroke_linejoin: "round",
                    }
                }
                "{text}"
            }
        }
    }
}

// --- Helper functions ---

fn format_msg_time(ts: i64) -> String {
    DateTime::from_timestamp(ts, 0)
        .map(|dt| dt.with_timezone(&Local).format("%H:%M").to_string())
        .unwrap_or_default()
}

/// A segment of message text: either plain text or a clickable URL.
enum TextPart {
    Text(String),
    Link(String),
}

/// Split message text into plain-text and link parts.
///
/// Any `http://`/`https://` URL (at a token start, or after an opening
/// bracket/quote) becomes a `Link`; trailing punctuation is kept as text.
fn split_links(text: &str) -> Vec<TextPart> {
    let mut parts = Vec::new();
    for piece in text.split_inclusive(char::is_whitespace) {
        let word_end = piece.trim_end_matches(char::is_whitespace).len();
        let word = &piece[..word_end];
        let whitespace = &piece[word_end..];
        if let Some(idx) = find_url_start(word) {
            let prefix = &word[..idx];
            let prefix_ok = prefix
                .chars()
                .all(|c| matches!(c, '(' | '[' | '{' | '\u{ab}' | '\u{201c}' | '\u{2018}'));
            if prefix_ok {
                if !prefix.is_empty() {
                    parts.push(TextPart::Text(prefix.to_string()));
                }
                let url = &word[idx..];
                let trimmed = url.trim_end_matches(|c: char| {
                    matches!(c, '.' | ',' | ';' | ':' | '!' | '?' | ')' | ']' | '}' | '>' | '\u{201d}' | '\u{2019}' | '\u{2026}')
                });
                let (href, tail) = (&url[..trimmed.len()], &url[trimmed.len()..]);
                parts.push(TextPart::Link(href.to_string()));
                if !tail.is_empty() {
                    parts.push(TextPart::Text(tail.to_string()));
                }
                if !whitespace.is_empty() {
                    parts.push(TextPart::Text(whitespace.to_string()));
                }
                continue;
            }
        }
        parts.push(TextPart::Text(piece.to_string()));
    }
    parts
}

/// Find the byte index of the first `http://` or `https://` (case-insensitive).
fn find_url_start(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    for i in 0..bytes.len() {
        let is_http = i + 7 <= bytes.len()
            && bytes[i] | 32 == b'h'
            && bytes[i + 1] | 32 == b't'
            && bytes[i + 2] | 32 == b't'
            && bytes[i + 3] | 32 == b'p'
            && bytes[i + 4] == b':'
            && bytes[i + 5] == b'/'
            && bytes[i + 6] == b'/';
        let is_https = i + 8 <= bytes.len()
            && bytes[i] | 32 == b'h'
            && bytes[i + 1] | 32 == b't'
            && bytes[i + 2] | 32 == b't'
            && bytes[i + 3] | 32 == b'p'
            && bytes[i + 4] | 32 == b's'
            && bytes[i + 5] == b':'
            && bytes[i + 6] == b'/'
            && bytes[i + 7] == b'/';
        if is_http || is_https {
            return Some(i);
        }
    }
    None
}

fn format_date(ts: i64) -> String {
    let now = Local::now();
    let dt = DateTime::from_timestamp(ts, 0).map(|dt| dt.with_timezone(&Local));

    match dt {
        Some(dt) => {
            if dt.date_naive() == now.date_naive() {
                "Today".to_string()
            } else if dt.date_naive() == (now - chrono::Duration::days(1)).date_naive() {
                "Yesterday".to_string()
            } else {
                dt.format("%d.%m.%Y").to_string()
            }
        }
        None => String::new(),
    }
}

fn day_changed(prev: i64, current: i64) -> bool {
    let prev_dt = DateTime::from_timestamp(prev, 0).map(|dt| dt.with_timezone(&Local).date_naive());
    let curr_dt =
        DateTime::from_timestamp(current, 0).map(|dt| dt.with_timezone(&Local).date_naive());

    match (prev_dt, curr_dt) {
        (Some(p), Some(c)) => p != c,
        _ => false,
    }
}


/// Voting/poll message display
#[component]
fn VotingMessage(is_mine: bool, question: String, options: Vec<VoteOption>, time: String) -> Element {
    let container_class = if is_mine { "items-end" } else { "items-start" };
    let bubble_class = if is_mine {
        "bg-[#d9fdd3] dark:bg-[#005c4b] rounded-[8px_0_8px_8px] self-end"
    } else {
        "bg-white dark:bg-[#202c33] rounded-[0_8px_8px_8px] self-start shadow-md"
    };

    let total: u32 = options.iter().map(|o| o.votes).sum();
    rsx! {
        div { class: "flex flex-col {container_class} mb-1.5",
            div { class: "max-w-[75%] overflow-hidden {bubble_class}",
                div { class: "px-3 py-2",
                    p { class: "text-sm font-semibold text-gray-900 dark:text-gray-100 mb-2", "{question}" }
                    {options.iter().map(|opt| {
                        let pct = if total > 0 { (opt.votes as f64 / total as f64) * 100.0 } else { 0.0 };
                        let vote_label = if opt.votes == 1 { "vote" } else { "votes" };
                        rsx! {
                            div { class: "flex flex-col py-0.5",
                                // Row 1: icon + option text (full width) + vote count
                                div { class: "flex items-center gap-2",
                                    svg { class: "w-4 h-4 shrink-0 text-gray-400 dark:text-gray-500", view_box: "0 0 16 16",
                                        circle { cx: "8", cy: "8", r: "6", fill: "none", stroke: "currentColor", stroke_width: "2" }
                                    }
                                    span { class: "text-sm text-gray-700 dark:text-gray-300 min-w-0 flex-1 break-words", "{opt.text}" }
                                    span { class: "text-xs text-gray-400 dark:text-gray-500 font-medium shrink-0 text-right", "{opt.votes} {vote_label}" }
                                }
                                // Row 2: full-width percentage bar
                                div {
                                    class: "h-3 rounded-full overflow-hidden bg-gray-400 dark:bg-gray-600 w-full mt-0.5",
                                    div { class: "h-full rounded-full bg-blue-500", style: "width: {pct}%" }
                                }
                            }
                    }
                }
                )}
                }
                div { class: "px-3 py-1 flex justify-end border-t border-gray-100 dark:border-gray-700",
                    span { class: "text-[10px] text-gray-400", "{time}" }
                }
            }
        }
    }
}
