//! Command-line interface, handled before Tauri starts. On Wayland this is
//! how profiles get hotkeys: bind `display-changer apply <name>` in the
//! compositor.

use display_core::{DisplayBackend, Profile, backends, profile::resolve};

use crate::profiles;

const HELP: &str = "\
Display Changer — monitor layouts and profiles

Usage:
  display-changer                 open the window
  display-changer --minimized     start in the tray
  display-changer list            list profiles (* = active)
  display-changer apply <name>    apply a profile by name or id, without confirmation
  display-changer state           print the current layout as JSON
  display-changer help            show this help";

/// Returns an exit code when the arguments were a CLI command.
pub fn run() -> Option<i32> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let command = args.first()?.as_str();
    if !matches!(
        command,
        "list" | "apply" | "state" | "help" | "--help" | "-h" | "--version" | "-V"
    ) {
        return None;
    }
    attach_console();
    Some(match command {
        "list" => list(),
        "apply" => match args.get(1) {
            Some(name) => apply(args[1..].join(" ").trim()).unwrap_or_else(|| {
                eprintln!("no profile named {name:?}; see `display-changer list`");
                2
            }),
            None => {
                eprintln!("usage: display-changer apply <name>");
                2
            }
        },
        "state" => state(),
        "--version" | "-V" => {
            println!("display-changer {}", env!("CARGO_PKG_VERSION"));
            0
        }
        _ => {
            println!("{HELP}");
            0
        }
    })
}

fn load(backend: &dyn DisplayBackend) -> Result<Vec<Profile>, i32> {
    profiles::open_store(backend.name())
        .map(|s| s.all().to_vec())
        .map_err(|e| {
            eprintln!("cannot read profiles: {e}");
            1
        })
}

fn list() -> i32 {
    let backend = backends::detect();
    let profiles = match load(backend.as_ref()) {
        Ok(p) => p,
        Err(code) => return code,
    };
    let current = backend.current_state().ok();
    let caps = backend.capabilities();
    for p in &profiles {
        let active = current.as_ref().is_some_and(|c| {
            display_core::profile::same_layout(&resolve(&p.layout, c, &caps).layout, &c.layout)
        });
        let hotkey = p
            .hotkey
            .as_deref()
            .map(|h| format!("  [{h}]"))
            .unwrap_or_default();
        println!("{} {}{hotkey}", if active { "*" } else { " " }, p.name);
    }
    if profiles.is_empty() {
        eprintln!("no profiles yet (backend: {})", backend.name());
    }
    0
}

fn apply(query: &str) -> Option<i32> {
    let backend = backends::detect();
    let profiles = match load(backend.as_ref()) {
        Ok(p) => p,
        Err(code) => return Some(code),
    };
    let profile = profiles
        .iter()
        .find(|p| p.id == query)
        .or_else(|| profiles.iter().find(|p| p.name.eq_ignore_ascii_case(query)))?;
    let state = match backend.current_state() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{e}");
            return Some(1);
        }
    };
    let resolved = resolve(&profile.layout, &state, &backend.capabilities());
    for w in &resolved.warnings {
        eprintln!("warning: {w:?}");
    }
    Some(match backend.apply(&resolved.layout) {
        Ok(()) => {
            println!("applied {:?}", profile.name);
            0
        }
        Err(e) => {
            eprintln!("{e}");
            1
        }
    })
}

fn state() -> i32 {
    let backend = backends::detect();
    match backend.current_state() {
        Ok(s) => {
            println!(
                "{}",
                serde_json::to_string_pretty(&s.layout).expect("serializable")
            );
            0
        }
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

/// Release builds on Windows have no console; reuse the parent's so output
/// shows up in the terminal that ran the command.
#[cfg(windows)]
fn attach_console() {
    use windows::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};
    // SAFETY: plain Win32 call; failure just means no console to attach to.
    unsafe {
        let _ = AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

#[cfg(not(windows))]
fn attach_console() {}
