use dioxus::prelude::*;
use components::Hero;
use components::Chat;

mod components;

const MAIN_CSS: Asset = asset!("/assets/main.css");
const TAILWIND_CSS: Asset = asset!("/assets/tailwind.css");

fn main() {
    dioxus::launch(App);
}

#[component]
fn App() -> Element {
    rsx! {
        document::Stylesheet { href: MAIN_CSS },
        document::Stylesheet { href: TAILWIND_CSS },

        Hero {}
    }
}
