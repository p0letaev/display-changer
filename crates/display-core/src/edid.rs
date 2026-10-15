//! Minimal EDID parser: just what identifies a monitor and names it.

/// Fields from the EDID base block.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EdidInfo {
    /// Three-letter PNP manufacturer code, e.g. `DEL`.
    pub vendor: String,
    pub product: u16,
    /// Serial from the descriptor string, else the numeric serial if non-zero.
    pub serial: Option<String>,
    /// Monitor name descriptor, e.g. `DELL U2723QE`.
    pub name: Option<String>,
}

const HEADER: [u8; 8] = [0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00];
const TAG_SERIAL: u8 = 0xff;
const TAG_NAME: u8 = 0xfc;

/// Parses the 128-byte base block. Returns `None` for anything that is not EDID.
pub fn parse(bytes: &[u8]) -> Option<EdidInfo> {
    if bytes.len() < 128 || bytes[..8] != HEADER {
        return None;
    }

    // Manufacturer ID: three 5-bit letters, big-endian, 'A' = 1.
    let id = u16::from_be_bytes([bytes[8], bytes[9]]);
    let vendor: String = [10, 5, 0]
        .iter()
        .map(|shift| {
            let v = ((id >> shift) & 0x1f) as u8;
            if (1..=26).contains(&v) {
                char::from(b'A' + v - 1)
            } else {
                '?'
            }
        })
        .collect();
    let product = u16::from_le_bytes([bytes[10], bytes[11]]);
    let numeric_serial = u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]);

    let mut info = EdidInfo {
        vendor,
        product,
        ..Default::default()
    };

    // Four 18-byte descriptors; display descriptors start with 00 00 00 <tag>.
    for block in bytes[54..126].as_chunks::<18>().0 {
        if block[..3] != [0, 0, 0] {
            continue;
        }
        let text = descriptor_text(&block[5..18]);
        match block[3] {
            TAG_SERIAL if !text.is_empty() => info.serial = Some(text),
            TAG_NAME if !text.is_empty() => info.name = Some(text),
            _ => {}
        }
    }
    if info.serial.is_none() && numeric_serial != 0 {
        info.serial = Some(numeric_serial.to_string());
    }
    Some(info)
}

/// Descriptor strings end at 0x0a and are padded with spaces.
fn descriptor_text(raw: &[u8]) -> String {
    let end = raw.iter().position(|&b| b == 0x0a).unwrap_or(raw.len());
    String::from_utf8_lossy(&raw[..end]).trim().to_string()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Builds a base block with the given identity and descriptors.
    pub fn sample(
        vendor: &str,
        product: u16,
        serial: u32,
        name: Option<&str>,
        serial_text: Option<&str>,
    ) -> Vec<u8> {
        let mut b = vec![0u8; 128];
        b[..8].copy_from_slice(&HEADER);
        let v = vendor.as_bytes();
        let id = (u16::from(v[0] - b'A' + 1) << 10)
            | (u16::from(v[1] - b'A' + 1) << 5)
            | u16::from(v[2] - b'A' + 1);
        b[8..10].copy_from_slice(&id.to_be_bytes());
        b[10..12].copy_from_slice(&product.to_le_bytes());
        b[12..16].copy_from_slice(&serial.to_le_bytes());
        let mut slot = 54;
        for (tag, text) in [(TAG_NAME, name), (TAG_SERIAL, serial_text)] {
            if let Some(text) = text {
                b[slot + 3] = tag;
                let mut field = [b' '; 13];
                field[..text.len()].copy_from_slice(text.as_bytes());
                if text.len() < 13 {
                    field[text.len()] = 0x0a;
                }
                b[slot + 5..slot + 18].copy_from_slice(&field);
                slot += 18;
            }
        }
        b
    }

    #[test]
    fn parses_identity_and_descriptors() {
        let edid = sample("DEL", 0x4291, 12345, Some("DELL U2723QE"), Some("7XQ2Y93"));
        let info = parse(&edid).unwrap();
        assert_eq!(info.vendor, "DEL");
        assert_eq!(info.product, 0x4291);
        assert_eq!(info.name.as_deref(), Some("DELL U2723QE"));
        assert_eq!(info.serial.as_deref(), Some("7XQ2Y93"));
    }

    #[test]
    fn falls_back_to_numeric_serial() {
        let info = parse(&sample("GSM", 1, 777, None, None)).unwrap();
        assert_eq!(info.serial.as_deref(), Some("777"));
        assert_eq!(info.name, None);
        assert_eq!(
            parse(&sample("GSM", 1, 0, None, None)).unwrap().serial,
            None
        );
    }

    #[test]
    fn rejects_garbage() {
        assert_eq!(parse(&[0u8; 128]), None);
        assert_eq!(parse(&HEADER), None);
    }
}
