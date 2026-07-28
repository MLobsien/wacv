use crate::storage::config::Config;
use dioxus::prelude::*;

/// Settings page — lets the user configure their display name.
#[component]
pub fn Settings() -> Element {
    let mut config = use_context::<Signal<Config>>();
    let mut name_input = use_signal(|| config.read().user_name.clone().unwrap_or_default());
    let mut saved = use_signal(|| false);
    let nav = use_navigator();

    rsx! {
        div { class: "flex flex-col h-full bg-gray-100",
            // Header
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
                h1 { class: "font-semibold text-sm", "Settings" }
            }

            // Content
            div { class: "flex-1 overflow-y-auto p-4",
                div { class: "bg-white rounded-lg shadow-sm p-4",
                    h2 { class: "text-sm font-semibold text-gray-700 mb-3", "Display Name" }
                    p { class: "text-xs text-gray-500 mb-3",
                        "Set your name so the app can identify your messages in group chats. "
                        "In 1:1 chats the name is detected automatically."
                    }

                    input {
                        class: "w-full px-3 py-2 border border-gray-300 rounded-lg text-sm focus:outline-none focus:ring-2 focus:ring-green-500 focus:border-green-500",
                        placeholder: "Your name",
                        value: name_input(),
                        oninput: move |e| {
                            name_input.set(e.value());
                            saved.set(false);
                        },
                    }

                    button {
                        class: "mt-3 px-4 py-2 bg-green-600 text-white rounded-lg text-sm font-medium hover:bg-green-700 transition-colors",
                        onclick: move |_| {
                            let name = name_input();
                            let trimmed = name.trim().to_string();
                            let val = if trimmed.is_empty() { None } else { Some(trimmed) };
                            config.write().user_name = val;
                            config.read().save();
                            saved.set(true);
                        },
                        "Save"
                    }

                    if saved() {
                        div { class: "mt-2 text-xs text-green-600 font-medium", "\u{2713} Saved" }
                    }
                }
            }
        }
    }
}
