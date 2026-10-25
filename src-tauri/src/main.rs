// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // `neontop --helper …`: modo ayudante lanzado por pkexec (sin interfaz)
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--helper") {
        std::process::exit(neontop_lib::helper::run(&args[1..]));
    }
    neontop_lib::run()
}
