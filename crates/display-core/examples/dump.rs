//! Prints the detected backend and current display state as JSON. Read-only.
//!
//! cargo run -p display-core --example dump

use display_core::backends;

fn main() {
    let backend = backends::detect();
    eprintln!("backend: {}", backend.name());
    match backend.current_state() {
        Ok(state) => println!(
            "{}",
            serde_json::to_string_pretty(&state).expect("serializable")
        ),
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
}
