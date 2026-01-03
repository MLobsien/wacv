use dioxus::{
    logger::{self, tracing},
    prelude::*,
};

#[component]
pub fn Hero() -> Element {
    rsx! {
        div {
            class: "border-b-gray-300 border-b-2 z-10 shadow-gray-500 px-2 py-1",
            AddButton {  }
        }
    }
}

#[component]
fn AddButton() -> Element {
    rsx! {
        label {
            class: "border-gray-400 border-2 rounded-md bg-gray-200 text-gray-900 hover:bg-gray-300",
            r#for: "file_browser",
            strong { "+" }
        }
        input {
            r#type: "file",
            accept: ".zip",
            id: "file_browser",
            onchange: |e| {
            }
        }
    }
}
