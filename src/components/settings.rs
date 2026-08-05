use crate::storage::config::{ChatSort, Config};
use dioxus::prelude::*;

/// Settings page — lets the user configure their display name.
#[component]
pub fn Settings() -> Element {
    let mut config = use_context::<Signal<Config>>();
    let mut name_input = use_signal(|| config.read().user_name.clone().unwrap_or_default());
    let mut saved = use_signal(|| false);
    let nav = use_navigator();
    let toggle_class =
        if config.read().dark_mode { "bg-green-600" } else { "bg-gray-300 dark:bg-gray-600" };
    let knob_class =
        if config.read().dark_mode { "left-[22px]" } else { "left-0.5" };
    let active_btn = "bg-green-600 text-white";
    let inactive_btn = "bg-gray-100 dark:bg-gray-700 text-gray-700 dark:text-gray-300";
    let sort = config.read().chat_sort;
    let by_time_btn = if sort == ChatSort::ByTime { active_btn } else { inactive_btn };
    let alpha_btn = if sort == ChatSort::Alphabetical { active_btn } else { inactive_btn };
    let base_btn = "flex-1 px-3 py-2 rounded-lg text-sm font-medium transition-colors";

    rsx! {
        div { class: "flex flex-col h-full bg-gray-100 dark:bg-gray-900",
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
                div { class: "bg-white dark:bg-gray-800 rounded-lg shadow-sm p-4",
                    h2 { class: "text-sm font-semibold text-gray-700 dark:text-gray-200 mb-3", "Display Name" }
                    p { class: "text-xs text-gray-500 dark:text-gray-400 mb-3",
                        "Set your name so the app can identify your messages in group chats. "
                        "In 1:1 chats the name is detected automatically."
                    }

                    input {
                        class: "w-full px-3 py-2 border border-gray-300 dark:border-gray-600 dark:bg-gray-900 dark:text-gray-100 rounded-lg text-sm focus:outline-none focus:ring-2 focus:ring-green-500 focus:border-green-500",
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
                div { class: "bg-white dark:bg-gray-800 rounded-lg shadow-sm p-4 mt-4",
                    h2 { class: "text-sm font-semibold text-gray-700 dark:text-gray-200 mb-3", "Appearance" }
                    div { class: "flex items-center justify-between",
                        div {
                            p { class: "text-sm text-gray-700 dark:text-gray-200 font-medium", "Dark Mode" }
                            p { class: "text-xs text-gray-500 dark:text-gray-400 mt-0.5", "Use a dark color scheme" }
                        }
                        button {
                            class: "relative w-11 h-6 rounded-full transition-colors {toggle_class}",
                            onclick: move |_| {
                                let on = config.read().dark_mode;
                                config.write().dark_mode = !on;
                                config.read().save();
                            },
                            div { class: "absolute top-0.5 h-5 w-5 rounded-full bg-white shadow transition-all {knob_class}" }
                        }
                    }
                }
                div { class: "bg-white dark:bg-gray-800 rounded-lg shadow-sm p-4 mt-4",
                    h2 { class: "text-sm font-semibold text-gray-700 dark:text-gray-200 mb-3", "Chat List" }
                    p { class: "text-xs text-gray-500 dark:text-gray-400 mb-3",
                        "How chats are ordered in the chat list."
                    }
                    div { class: "flex gap-2",
                        button {
                            class: "{base_btn} {by_time_btn}",
                            onclick: move |_| {
                                config.write().chat_sort = ChatSort::ByTime;
                                config.read().save();
                            },
                            "By time"
                        }
                        button {
                            class: "{base_btn} {alpha_btn}",
                            onclick: move |_| {
                                config.write().chat_sort = ChatSort::Alphabetical;
                                config.read().save();
                            },
                            "Alphabetical"
                        }
                    }
                }

            }
        }
    }
}
