#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if let Err(error) = readloom_lib::run() {
        eprintln!("Readloom failed to start: {error}");
        std::process::exit(1);
    }
}
