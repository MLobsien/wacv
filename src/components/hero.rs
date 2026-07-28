use dioxus::prelude::*;

/// Top header component for the main view
#[component]
pub fn Hero() -> Element {
    rsx! {
        div { class: "bg-green-600 text-white px-4 py-3 flex items-center justify-between shadow-md",
            h1 { class: "text-xl font-bold", "WACV" }
        }
    }
}
