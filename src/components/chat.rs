use dioxus::prelude::*;

#[component]
pub fn Chat(name: String) -> Element {
    rsx! {
        div {
            class: "py-1 px-2",
            p { "{name}" }
        }
    }
}
