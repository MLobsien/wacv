#![cfg_attr(target_os = "android", no_main)]

use std::env;

fn main() {
    let args: Vec<String> = env::args().collect();

    // `wacv` with no args launches the GUI. `wacv import <path>` and
    // `wacv --help` run the import CLI instead (desktop only; the Android
    // binary has no CLI).
    #[cfg(not(target_os = "android"))]
    if args.len() > 1 {
        dioxusmain::cli_main();
        return;
    }

    dioxusmain::main();
}
