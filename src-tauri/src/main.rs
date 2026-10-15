// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if let Some(code) = display_changer_lib::run_cli() {
        std::process::exit(code);
    }
    display_changer_lib::run();
}
