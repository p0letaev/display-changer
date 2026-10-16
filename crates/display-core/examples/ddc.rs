//! DDC/CI probe: what a monitor lets us control over the cable.
//!
//! cargo run -p display-core --example ddc                      # monitors, input source, capabilities
//! cargo run -p display-core --example ddc -- scan DP-1         # every readable VCP code
//! cargo run -p display-core --example ddc -- set DP-1 0x60 0x0f   # write a value (changes the monitor!)
//!
//! To find an undocumented code (e.g. picture-by-picture), scan with the mode
//! off and on and diff the two outputs.
//!
//! Linux needs the i2c-dev module and access to /dev/i2c-*:
//! `sudo modprobe i2c-dev`, then run as root or add yourself to the i2c group.
//! Windows names monitors `\\.\DISPLAY1` and so on; a unique part is enough.

use std::process::ExitCode;

use display_core::ddc::{self, DdcMonitor, INPUT_SOURCE};

fn parse_number(s: &str) -> Option<u16> {
    match s.strip_prefix("0x") {
        Some(hex) => u16::from_str_radix(hex, 16).ok(),
        None => s.parse().ok(),
    }
}

fn list(monitors: Vec<(String, std::io::Result<DdcMonitor>)>) {
    for (name, monitor) in monitors {
        println!("{name}");
        let mut m = match monitor {
            Ok(m) => m,
            Err(e) => {
                println!("  unreachable: {e}");
                continue;
            }
        };
        match m.get(INPUT_SOURCE) {
            Ok(Some(v)) => println!(
                "  input:  0x{:02x} {} (raw 0x{:04x}, max 0x{:04x})",
                v.current & 0xff,
                ddc::input_name(v.current).unwrap_or("vendor-specific"),
                v.current,
                v.maximum,
            ),
            Ok(None) => println!("  input:  not supported"),
            Err(e) => println!("  input:  error: {e}"),
        }
        match m.capabilities() {
            Ok(caps) => println!("  caps:   {caps}"),
            Err(e) => println!("  caps:   error: {e}"),
        }
    }
}

fn scan(m: &mut DdcMonitor) {
    eprintln!("scanning {} (0x00..0xff, takes about a minute)", m.name);
    for code in 0..=0xffu8 {
        match m.get(code) {
            Ok(Some(v)) => println!(
                "0x{code:02x} = 0x{:04x} max 0x{:04x} kind {}",
                v.current, v.maximum, v.kind
            ),
            Ok(None) => {}
            Err(e) => eprintln!("0x{code:02x}: {e}"),
        }
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let monitors = ddc::enumerate();
    if monitors.is_empty() {
        eprintln!("no connected monitors found");
        return ExitCode::FAILURE;
    }
    if matches!(args.first().map(String::as_str), None | Some("list")) {
        list(monitors);
        return ExitCode::SUCCESS;
    }

    let Some(wanted) = args.get(1) else {
        eprintln!("usage: list | scan <monitor> | set <monitor> <code> <value>");
        return ExitCode::FAILURE;
    };
    // Exact name first, so DP-1 does not also pick DP-10.
    let exact = monitors.iter().any(|(name, _)| name == wanted);
    let mut found: Vec<_> = monitors
        .into_iter()
        .filter(|(name, _)| {
            if exact {
                name == wanted
            } else {
                name.contains(wanted.as_str())
            }
        })
        .collect();
    if found.len() != 1 {
        eprintln!(
            "{wanted:?} matches {} monitors; run without arguments to list them",
            found.len()
        );
        return ExitCode::FAILURE;
    }
    let mut m = match found.remove(0).1 {
        Ok(m) => m,
        Err(e) => {
            eprintln!("{wanted}: {e}");
            return ExitCode::FAILURE;
        }
    };

    match args[0].as_str() {
        "scan" => scan(&mut m),
        "set" => {
            let (Some(code), Some(value)) = (
                args.get(2)
                    .and_then(|s| parse_number(s))
                    .and_then(|c| u8::try_from(c).ok()),
                args.get(3).and_then(|s| parse_number(s)),
            ) else {
                eprintln!("usage: set <monitor> <code 0x00..0xff> <value>");
                return ExitCode::FAILURE;
            };
            if let Err(e) = m.set(code, value) {
                eprintln!("error: {e}");
                return ExitCode::FAILURE;
            }
            println!("{}: 0x{code:02x} <- 0x{value:04x}", m.name);
        }
        other => {
            eprintln!("unknown command {other:?}; use list, scan or set");
            return ExitCode::FAILURE;
        }
    }
    ExitCode::SUCCESS
}
