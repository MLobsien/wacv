use std::env;

fn main() {
    let args: Vec<String> = env::args().collect();

    // `wacv` with no args launches the GUI. `wacv import <path>` and
    // `wacv --help` run the import CLI instead.
    if args.len() > 1 {
        dioxusmain::cli_main();
        return;
    }

    dioxusmain::main();
}
