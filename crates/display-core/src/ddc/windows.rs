//! DDC/CI through dxva2 (Monitor Configuration API).

use std::io;

use windows::Win32::Devices::Display::{
    CapabilitiesRequestAndCapabilitiesReply, DestroyPhysicalMonitors, GetCapabilitiesStringLength,
    GetNumberOfPhysicalMonitorsFromHMONITOR, GetPhysicalMonitorsFromHMONITOR,
    GetVCPFeatureAndVCPFeatureReply, MC_VCP_CODE_TYPE, PHYSICAL_MONITOR, SetVCPFeature,
};
use windows::Win32::Foundation::{LPARAM, RECT};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO, MONITORINFOEXW,
};
use windows::core::BOOL;

use super::{DdcMonitor, VcpValue};

pub struct Handle {
    monitor: PHYSICAL_MONITOR,
}

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: the handle came from GetPhysicalMonitorsFromHMONITOR.
        let _ = unsafe { DestroyPhysicalMonitors(&[self.monitor]) };
    }
}

pub fn enumerate() -> Vec<(String, io::Result<DdcMonitor>)> {
    unsafe extern "system" fn collect(h: HMONITOR, _: HDC, _: *mut RECT, out: LPARAM) -> BOOL {
        // SAFETY: `out` is the Vec passed below, alive for the whole call.
        unsafe { (*(out.0 as *mut Vec<HMONITOR>)).push(h) };
        true.into()
    }
    let mut hmonitors: Vec<HMONITOR> = Vec::new();
    // SAFETY: the callback only pushes into `hmonitors`.
    let _ = unsafe {
        EnumDisplayMonitors(
            None,
            None,
            Some(collect),
            LPARAM(&mut hmonitors as *mut _ as isize),
        )
    };

    let mut result = Vec::new();
    for h in hmonitors {
        let device = gdi_name(h).unwrap_or_else(|| "?".into());
        let mut count = 0;
        // SAFETY: valid HMONITOR from the enumeration.
        if let Err(e) = unsafe { GetNumberOfPhysicalMonitorsFromHMONITOR(h, &mut count) } {
            result.push((device, Err(io::Error::other(e))));
            continue;
        }
        let mut physical = vec![PHYSICAL_MONITOR::default(); count as usize];
        // SAFETY: the slice has the size Windows asked for.
        if let Err(e) = unsafe { GetPhysicalMonitorsFromHMONITOR(h, &mut physical) } {
            result.push((device, Err(io::Error::other(e))));
            continue;
        }
        for monitor in physical {
            // Packed struct: copy the field out before borrowing it.
            let raw = monitor.szPhysicalMonitorDescription;
            let end = raw.iter().position(|&c| c == 0).unwrap_or(raw.len());
            let description = String::from_utf16_lossy(&raw[..end]);
            let name = format!("{device} ({description})");
            result.push((
                name.clone(),
                Ok(DdcMonitor {
                    name,
                    inner: Handle { monitor },
                }),
            ));
        }
    }
    result
}

fn gdi_name(h: HMONITOR) -> Option<String> {
    let mut info = MONITORINFOEXW {
        monitorInfo: MONITORINFO {
            cbSize: size_of::<MONITORINFOEXW>() as u32,
            ..Default::default()
        },
        ..Default::default()
    };
    // SAFETY: cbSize marks the struct as MONITORINFOEXW.
    unsafe { GetMonitorInfoW(h, &mut info as *mut _ as *mut MONITORINFO) }
        .as_bool()
        .then(|| {
            let end = info.szDevice.iter().position(|&c| c == 0).unwrap_or(0);
            String::from_utf16_lossy(&info.szDevice[..end])
        })
}

impl Handle {
    pub fn get(&mut self, code: u8) -> io::Result<Option<VcpValue>> {
        let (mut kind, mut current, mut maximum) = (MC_VCP_CODE_TYPE::default(), 0u32, 0u32);
        // SAFETY: valid physical monitor handle and out pointers.
        let ok = unsafe {
            GetVCPFeatureAndVCPFeatureReply(
                self.monitor.hPhysicalMonitor,
                code,
                Some(&mut kind),
                &mut current,
                Some(&mut maximum),
            )
        };
        if ok == 0 {
            // dxva2 does not tell "unsupported" from a failed transfer.
            return Ok(None);
        }
        Ok(Some(VcpValue {
            current: current as u16,
            maximum: maximum as u16,
            kind: kind.0 as u8,
        }))
    }

    pub fn set(&mut self, code: u8, value: u16) -> io::Result<()> {
        // SAFETY: valid physical monitor handle.
        if unsafe { SetVCPFeature(self.monitor.hPhysicalMonitor, code, value.into()) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    pub fn capabilities(&mut self) -> io::Result<String> {
        let mut len = 0;
        // SAFETY: valid physical monitor handle and out pointer.
        if unsafe { GetCapabilitiesStringLength(self.monitor.hPhysicalMonitor, &mut len) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut buf = vec![0u8; len as usize];
        // SAFETY: the buffer has the length Windows reported.
        if unsafe {
            CapabilitiesRequestAndCapabilitiesReply(self.monitor.hPhysicalMonitor, &mut buf)
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
        Ok(String::from_utf8_lossy(&buf[..end]).into_owned())
    }
}
