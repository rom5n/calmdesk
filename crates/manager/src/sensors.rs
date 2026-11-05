//! Readings of this PC for the system monitor and Bluetooth battery widgets: processor, memory
//! and graphics load, and the battery level Bluetooth devices report. Worker thread only.

use windows::Win32::Devices::DeviceAndDriverInstallation::*;
use windows::Win32::Devices::Properties::{DEVPKEY_Device_FriendlyName, DEVPROPTYPE};
use windows::Win32::Foundation::{DEVPROPKEY, FILETIME};
use windows::Win32::System::Performance::*;
use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows::Win32::System::Threading::GetSystemTimes;
use windows::core::{GUID, w};

/// Load of the processor, memory and graphics, in percent, since the last call.
#[derive(Default)]
pub struct Load {
    /// Idle and total processor time at the last call.
    cpu: (u64, u64),
    gpu: Option<(PDH_HQUERY, PDH_HCOUNTER)>,
    gpu_failed: bool,
}

impl Load {
    /// (processor, memory, graphics; None without a counter), and memory (used, total) bytes.
    pub fn read(&mut self) -> (f32, f32, Option<f32>, (u64, u64)) {
        let ticks = |t: FILETIME| (t.dwHighDateTime as u64) << 32 | t.dwLowDateTime as u64;
        let (mut idle, mut kernel, mut user) = Default::default();
        let _ = unsafe { GetSystemTimes(Some(&mut idle), Some(&mut kernel), Some(&mut user)) };
        // Kernel time includes idle time.
        let now = (ticks(idle), ticks(kernel) + ticks(user));
        let (di, dt) = (now.0.saturating_sub(self.cpu.0), now.1.saturating_sub(self.cpu.1));
        let cpu = if dt == 0 || self.cpu.1 == 0 { 0.0 } else { 100.0 * (1.0 - di as f32 / dt as f32) };
        self.cpu = now;

        let mut mem = MEMORYSTATUSEX { dwLength: size_of::<MEMORYSTATUSEX>() as u32, ..Default::default() };
        let _ = unsafe { GlobalMemoryStatusEx(&mut mem) };
        let used = mem.ullTotalPhys - mem.ullAvailPhys;
        (cpu.clamp(0.0, 100.0), mem.dwMemoryLoad as f32, self.gpu(), (used, mem.ullTotalPhys))
    }

    /// Sum of the 3D engine use of all processes, as Task Manager shows it.
    fn gpu(&mut self) -> Option<f32> {
        if self.gpu.is_none() && !self.gpu_failed {
            let (mut q, mut c) = (PDH_HQUERY::default(), PDH_HCOUNTER::default());
            let ok = unsafe {
                PdhOpenQueryW(None, 0, &mut q) == 0
                    && PdhAddEnglishCounterW(q, w!(r"\GPU Engine(*engtype_3D)\Utilization Percentage"), 0, &mut c) == 0
                    && PdhCollectQueryData(q) == 0
            };
            self.gpu_failed = !ok;
            if !ok {
                return None;
            }
            self.gpu = Some((q, c));
            // A rate needs two samples: the next call has one.
            return Some(0.0);
        }
        let (q, c) = self.gpu?;
        unsafe {
            if PdhCollectQueryData(q) != 0 {
                return None;
            }
            let (mut size, mut count) = (0u32, 0u32);
            let _ = PdhGetFormattedCounterArrayW(c, PDH_FMT_DOUBLE, &mut size, &mut count, None);
            if size == 0 {
                return Some(0.0);
            }
            // u64 items keep the buffer aligned for the structs.
            let mut buf = vec![0u64; (size as usize).div_ceil(8)];
            let items = buf.as_mut_ptr() as *mut PDH_FMT_COUNTERVALUE_ITEM_W;
            if PdhGetFormattedCounterArrayW(c, PDH_FMT_DOUBLE, &mut size, &mut count, Some(items)) != 0 {
                return None;
            }
            let sum: f64 = std::slice::from_raw_parts(items, count as usize).iter().map(|i| i.FmtValue.Anonymous.doubleValue).sum();
            Some(sum.clamp(0.0, 100.0) as f32)
        }
    }
}

/// `DEVPKEY_Bluetooth_Battery`: the level a Bluetooth device reports, one byte.
const BATTERY: DEVPROPKEY = DEVPROPKEY { fmtid: GUID::from_u128(0x104ea319_6ee2_4701_bd47_8ddbf425bbe5), pid: 2 };

/// (name, percent) of the connected devices that report a battery level, by name.
pub fn bluetooth_batteries() -> Vec<(String, u8)> {
    let mut out: Vec<(String, u8)> = Vec::new();
    unsafe {
        let Ok(set) = SetupDiGetClassDevsW(None, None, None, DIGCF_PRESENT | DIGCF_ALLCLASSES) else { return out };
        let mut dev = SP_DEVINFO_DATA { cbSize: size_of::<SP_DEVINFO_DATA>() as u32, ..Default::default() };
        let mut i = 0;
        while SetupDiEnumDeviceInfo(set, i, &mut dev).is_ok() {
            i += 1;
            let mut kind = DEVPROPTYPE::default();
            let mut level = [0u8; 1];
            if SetupDiGetDevicePropertyW(set, &dev, &BATTERY, &mut kind, Some(&mut level), None, 0).is_err() {
                continue;
            }
            let mut name = [0u8; 512];
            let mut size = 0u32;
            if SetupDiGetDevicePropertyW(set, &dev, &DEVPKEY_Device_FriendlyName, &mut kind, Some(&mut name), Some(&mut size), 0).is_err() {
                continue;
            }
            let wide: Vec<u16> = name[..size as usize].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).take_while(|&c| c != 0).collect();
            let name = String::from_utf16_lossy(&wide);
            // Headsets show up once per profile (hands-free, audio).
            if !name.is_empty() && !out.iter().any(|(n, _)| *n == name) {
                out.push((name, level[0].min(100)));
            }
        }
        let _ = SetupDiDestroyDeviceInfoList(set);
    }
    out.sort();
    out
}
