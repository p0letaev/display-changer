//! Applies a layout JSON (as printed by the `dump` example under "layout")
//! read from stdin. No confirmation and no revert: for backend debugging.
//!
//! cargo run -p display-core --example dump > state.json
//! jq .layout state.json | cargo run -p display-core --example apply

use std::io::Read;

use display_core::{Layout, backends};

fn main() {
    let mut input = String::new();
    std::io::stdin()
        .read_to_string(&mut input)
        .expect("read stdin");
    let layout: Layout = serde_json::from_str(&input).expect("layout JSON");
    let backend = backends::detect();
    eprintln!("backend: {}", backend.name());
    if let Err(e) = backend.apply(&layout) {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
    let state = backend.current_state().expect("state after apply");
    println!(
        "{}",
        serde_json::to_string_pretty(&state.layout).expect("serializable")
    );
}
