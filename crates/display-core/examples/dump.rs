//! Prints the detected backend and current display state as JSON. Read-only.
//!
//! cargo run -p display-core --example dump

use display_core::backends;

/// Prints backend log messages (detection reasons) to stderr.
struct StderrLog;

impl log::Log for StderrLog {
    fn enabled(&self, _: &log::Metadata) -> bool {
        true
    }
    fn log(&self, record: &log::Record) {
        eprintln!("[{}] {}", record.level(), record.args());
    }
    fn flush(&self) {}
}

fn main() {
    let _ = log::set_logger(&StderrLog).map(|()| log::set_max_level(log::LevelFilter::Debug));
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
