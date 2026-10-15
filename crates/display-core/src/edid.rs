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
    /// Screen size in millimetres (EDID stores centimetres).
    pub size_mm: Option<(u32, u32)>,
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

    let size_mm = (bytes[21] > 0 && bytes[22] > 0)
        .then(|| (u32::from(bytes[21]) * 10, u32::from(bytes[22]) * 10));
    let mut info = EdidInfo {
        vendor,
        product,
        size_mm,
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

/// The monitor's preferred mode: EDID puts it in the first detailed timing
/// descriptor. Refresh is rounded to whole hertz, as Windows reports modes.
pub fn preferred_mode(bytes: &[u8]) -> Option<crate::model::Mode> {
    if bytes.len() < 128 || bytes[..8] != HEADER {
        return None;
    }
    let d = &bytes[54..72];
    let clock_hz = u64::from(u16::from_le_bytes([d[0], d[1]])) * 10_000;
    if clock_hz == 0 {
        return None; // a display descriptor, not a timing
    }
    let h_active = u32::from(d[2]) | (u32::from(d[4] & 0xf0) << 4);
    let h_blank = u32::from(d[3]) | (u32::from(d[4] & 0x0f) << 8);
    let v_active = u32::from(d[5]) | (u32::from(d[7] & 0xf0) << 4);
    let v_blank = u32::from(d[6]) | (u32::from(d[7] & 0x0f) << 8);
    let total = u64::from(h_active + h_blank) * u64::from(v_active + v_blank);
    if h_active == 0 || v_active == 0 || total == 0 {
        return None;
    }
    let hz = ((clock_hz as f64) / (total as f64)).round() as u32;
    Some(crate::model::Mode {
        width: h_active,
        height: v_active,
        refresh_mhz: hz * 1000,
    })
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
        assert_eq!(info.size_mm, None);
        let mut sized = edid;
        sized[21] = 60;
        sized[22] = 34;
        assert_eq!(parse(&sized).unwrap().size_mm, Some((600, 340)));
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
    fn reads_preferred_timing() {
        // CEA 1080p60: 148.5 MHz, 1920+280 x 1080+45.
        let mut b = sample("DEL", 1, 0, None, None);
        let dtd = [0x02, 0x3a, 0x80, 0x18, 0x71, 0x38, 0x2d, 0x40];
        b[54..62].copy_from_slice(&dtd);
        let mode = preferred_mode(&b).unwrap();
        assert_eq!(
            (mode.width, mode.height, mode.refresh_mhz),
            (1920, 1080, 60_000)
        );
        assert_eq!(preferred_mode(&sample("DEL", 1, 0, Some("X"), None)), None);
    }

    #[test]
    fn rejects_garbage() {
        assert_eq!(parse(&[0u8; 128]), None);
        assert_eq!(parse(&HEADER), None);
    }
}
