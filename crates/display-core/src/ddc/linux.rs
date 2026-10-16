//! DDC/CI over i2c-dev: the monitor answers at address 0x37 on the bus that
//! sysfs links to its DRM connector.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::path::Path;
use std::thread::sleep;
use std::time::Duration;

use super::{DdcMonitor, VcpValue};

const I2C_SLAVE: libc::c_ulong = 0x0703;
const DDC_ADDRESS: libc::c_ulong = 0x37;
/// Address bytes the checksums start from (MCCS 2.2, section 4).
const DISPLAY_WRITE: u8 = 0x6e;
const HOST: u8 = 0x51;
const HOST_VIRTUAL: u8 = 0x50;

const GET_VCP: u8 = 0x01;
const GET_VCP_REPLY: u8 = 0x02;
const SET_VCP: u8 = 0x03;
const CAPABILITIES: u8 = 0xf3;
const CAPABILITIES_REPLY: u8 = 0xe3;

/// Monitors need time to prepare a reply and between commands.
const REPLY_DELAY: Duration = Duration::from_millis(50);
const ATTEMPTS: usize = 3;

pub struct Handle {
    file: File,
}

pub fn enumerate() -> Vec<(String, io::Result<DdcMonitor>)> {
    let Ok(entries) = fs::read_dir("/sys/class/drm") else {
        return Vec::new();
    };
    let mut connectors: Vec<_> = entries
        .flatten()
        .filter(|e| {
            fs::read_to_string(e.path().join("status")).is_ok_and(|s| s.trim() == "connected")
        })
        .collect();
    connectors.sort_by_key(|e| e.file_name());
    connectors
        .into_iter()
        .map(|e| {
            let file_name = e.file_name().to_string_lossy().into_owned();
            // card0-DP-1 -> DP-1
            let name = file_name
                .split_once('-')
                .map_or(file_name.clone(), |(_, c)| c.to_string());
            let monitor = open_connector(&e.path()).map(|inner| DdcMonitor {
                name: name.clone(),
                inner,
            });
            (name, monitor)
        })
        .collect()
}

/// Tries the connector's buses: DisplayPort AUX first (an `i2c-N`
/// subdirectory), then the `ddc` link, and keeps the first that answers.
fn open_connector(dir: &Path) -> io::Result<Handle> {
    let mut buses: Vec<String> = fs::read_dir(dir)?
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("i2c-"))
        .collect();
    if let Some(name) = fs::read_link(dir.join("ddc"))
        .ok()
        .and_then(|l| Some(l.file_name()?.to_string_lossy().into_owned()))
    {
        buses.push(name);
    }
    let mut last = io::Error::new(io::ErrorKind::NotFound, "no i2c bus for this connector");
    for bus in buses {
        let path = Path::new("/dev").join(&bus);
        if !path.exists() {
            last = io::Error::new(
                io::ErrorKind::NotFound,
                format!(
                    "{} missing, load i2c-dev (sudo modprobe i2c-dev)",
                    path.display()
                ),
            );
            continue;
        }
        match Handle::open(&path).and_then(|mut h| h.get(super::INPUT_SOURCE).map(|_| h)) {
            Ok(h) => return Ok(h),
            Err(e) => last = io::Error::new(e.kind(), format!("{}: {e}", path.display())),
        }
    }
    Err(last)
}

impl Handle {
    fn open(path: &Path) -> io::Result<Self> {
        let file = OpenOptions::new().read(true).write(true).open(path)?;
        // SAFETY: plain ioctl on an owned fd with an integer argument.
        if unsafe { libc::ioctl(file.as_raw_fd(), I2C_SLAVE, DDC_ADDRESS) } < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self { file })
    }

    pub fn get(&mut self, code: u8) -> io::Result<Option<VcpValue>> {
        retry(|| {
            let reply = self.request(&[GET_VCP, code], 8, 8)?;
            // [0x02, result, code, type, max hi, max lo, current hi, current lo]
            if reply[0] != GET_VCP_REPLY || reply[2] != code {
                return Err(invalid("unexpected reply to get"));
            }
            if reply[1] != 0 {
                return Ok(None);
            }
            Ok(Some(VcpValue {
                maximum: u16::from_be_bytes([reply[4], reply[5]]),
                current: u16::from_be_bytes([reply[6], reply[7]]),
                kind: reply[3],
            }))
        })
    }

    pub fn set(&mut self, code: u8, value: u16) -> io::Result<()> {
        let [hi, lo] = value.to_be_bytes();
        self.write(&[SET_VCP, code, hi, lo])?;
        sleep(REPLY_DELAY);
        Ok(())
    }

    pub fn capabilities(&mut self) -> io::Result<String> {
        let mut caps = Vec::new();
        loop {
            let [hi, lo] = u16::try_from(caps.len())
                .map_err(|_| invalid("capabilities too long"))?
                .to_be_bytes();
            let chunk = retry(|| {
                let reply = self.request(&[CAPABILITIES, hi, lo], 3, 35)?;
                if reply[0] != CAPABILITIES_REPLY || reply[1..3] != [hi, lo] {
                    return Err(invalid("unexpected reply to capabilities"));
                }
                Ok(reply[3..].to_vec())
            })?;
            if chunk.is_empty() || caps.len() > 4096 {
                break;
            }
            caps.extend(chunk);
        }
        let end = caps.iter().position(|&b| b == 0).unwrap_or(caps.len());
        Ok(String::from_utf8_lossy(&caps[..end]).into_owned())
    }

    fn write(&mut self, payload: &[u8]) -> io::Result<()> {
        let mut packet = vec![HOST, 0x80 | payload.len() as u8];
        packet.extend_from_slice(payload);
        packet.push(checksum(DISPLAY_WRITE, &packet));
        self.file.write_all(&packet)
    }

    /// Sends `payload` and returns the reply payload, `min..=max` bytes long.
    fn request(&mut self, payload: &[u8], min: usize, max: usize) -> io::Result<Vec<u8>> {
        self.write(payload)?;
        sleep(REPLY_DELAY);
        // [source, 0x80 | length, payload..., checksum]
        let mut buf = vec![0u8; max + 3];
        self.file.read_exact(&mut buf)?;
        let n = (buf[1] & 0x7f) as usize;
        if buf[1] & 0x80 == 0 || !(min..=max).contains(&n) {
            // Includes the "null message" a busy monitor sends.
            return Err(invalid("reply has unexpected length"));
        }
        if checksum(HOST_VIRTUAL, &buf[..n + 2]) != buf[n + 2] {
            return Err(invalid("reply checksum mismatch"));
        }
        Ok(buf[2..n + 2].to_vec())
    }
}

fn checksum(start: u8, bytes: &[u8]) -> u8 {
    bytes.iter().fold(start, |acc, b| acc ^ b)
}

fn invalid(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg)
}

fn retry<T>(mut f: impl FnMut() -> io::Result<T>) -> io::Result<T> {
    let mut result = f();
    for _ in 1..ATTEMPTS {
        if result.is_ok() {
            break;
        }
        sleep(REPLY_DELAY * 2);
        result = f();
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_vcp_request_checksum() {
        // From the MCCS examples: get brightness (0x10).
        let packet = [HOST, 0x82, GET_VCP, 0x10];
        assert_eq!(checksum(DISPLAY_WRITE, &packet), 0xac);
    }
}
