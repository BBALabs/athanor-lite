//! GPU probing.
//!
//! Strategy (in order of trust):
//! 1. NVML — exact VRAM totals/usage, driver + CUDA versions, live telemetry (NVIDIA only).
//! 2. WMI `Win32_VideoController` — enumerates every adapter (any vendor), but its
//!    `AdapterRAM` field is a u32 and silently caps at 4 GB. Never trusted for size.
//! 3. Registry `HardwareInformation.qwMemorySize` — the accurate 64-bit VRAM total,
//!    matched to WMI adapters by driver description.

use std::sync::OnceLock;

use nvml_wrapper::enum_wrappers::device::TemperatureSensor;
use nvml_wrapper::Nvml;

use super::{GpuInfo, GpuVendor};

/// NVML handle, initialized once per process. `None` on machines without an
/// NVIDIA driver — every caller degrades gracefully.
pub fn nvml() -> Option<&'static Nvml> {
    static NVML: OnceLock<Option<Nvml>> = OnceLock::new();
    NVML.get_or_init(|| match Nvml::init() {
        Ok(n) => Some(n),
        Err(e) => {
            log::info!(target: "hw", "NVML unavailable ({e}); falling back to WMI probe");
            None
        }
    })
    .as_ref()
}

fn classify_vendor(name: &str) -> GpuVendor {
    let n = name.to_ascii_lowercase();
    if ["nvidia", "geforce", "rtx", "gtx", "quadro", "tesla"]
        .iter()
        .any(|k| n.contains(k))
    {
        GpuVendor::Nvidia
    } else if n.contains("amd") || n.contains("radeon") || n.contains("firepro") {
        GpuVendor::Amd
    } else if n.contains("intel") || n.contains("arc") || n.contains("iris") || n.contains("uhd") {
        GpuVendor::Intel
    } else {
        GpuVendor::Other
    }
}

fn detect_nvml_gpus() -> Vec<GpuInfo> {
    let Some(nvml) = nvml() else {
        return Vec::new();
    };

    let driver = nvml.sys_driver_version().ok();
    let cuda = nvml.sys_cuda_driver_version().ok().map(|v| {
        format!(
            "{}.{}",
            nvml_wrapper::cuda_driver_version_major(v),
            nvml_wrapper::cuda_driver_version_minor(v)
        )
    });

    let count = nvml.device_count().unwrap_or(0);
    let mut gpus = Vec::with_capacity(count as usize);

    for i in 0..count {
        let Ok(dev) = nvml.device_by_index(i) else {
            continue;
        };
        let mem = dev.memory_info().ok();
        let util = dev.utilization_rates().ok();
        gpus.push(GpuInfo {
            name: dev.name().unwrap_or_else(|_| "NVIDIA GPU".into()),
            vendor: GpuVendor::Nvidia,
            vram_total_bytes: mem.as_ref().map(|m| m.total),
            vram_used_bytes: mem.as_ref().map(|m| m.used),
            driver_version: driver.clone(),
            cuda_version: cuda.clone(),
            temperature_c: dev.temperature(TemperatureSensor::Gpu).ok(),
            utilization_pct: util.map(|u| u.gpu),
            source: "nvml".into(),
        });
    }
    gpus
}

/// Full adapter sweep. NVML devices first (authoritative), then any adapter WMI
/// knows about that NVML didn't cover, with VRAM totals pulled from the registry.
pub fn detect_gpus() -> Vec<GpuInfo> {
    let mut gpus = detect_nvml_gpus();

    #[cfg(windows)]
    {
        for wmi_gpu in windows_probe::detect_wmi_gpus() {
            let already_covered = gpus.iter().any(|g| {
                g.name.eq_ignore_ascii_case(&wmi_gpu.name)
                    || g.name.contains(&wmi_gpu.name)
                    || wmi_gpu.name.contains(&g.name)
            });
            if !already_covered {
                gpus.push(wmi_gpu);
            }
        }
    }

    // Largest VRAM first — the primary inference device leads everywhere in the UI.
    gpus.sort_by_key(|g| std::cmp::Reverse(g.vram_total_bytes.unwrap_or(0)));
    gpus
}

#[cfg(windows)]
mod windows_probe {
    use std::collections::HashMap;

    use serde::Deserialize;
    use winreg::enums::HKEY_LOCAL_MACHINE;
    use winreg::RegKey;

    use super::{classify_vendor, GpuInfo};

    #[derive(Deserialize, Debug)]
    struct Win32VideoController {
        #[serde(rename = "Name")]
        name: Option<String>,
        #[serde(rename = "DriverVersion")]
        driver_version: Option<String>,
    }

    /// Display-class GUID under which Windows records the true 64-bit VRAM size.
    const DISPLAY_CLASS_KEY: &str =
        r"SYSTEM\CurrentControlSet\Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}";

    /// DriverDesc -> qwMemorySize, for matching accurate VRAM totals to WMI names.
    fn registry_vram_by_desc() -> HashMap<String, u64> {
        let mut map = HashMap::new();
        let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
        let Ok(class) = hklm.open_subkey(DISPLAY_CLASS_KEY) else {
            return map;
        };
        for sub in class.enum_keys().flatten() {
            if !sub.chars().all(|c| c.is_ascii_digit()) {
                continue;
            }
            let Ok(key) = class.open_subkey(&sub) else {
                continue;
            };
            let desc: Result<String, _> = key.get_value("DriverDesc");
            let size: Result<u64, _> = key.get_value("HardwareInformation.qwMemorySize");
            if let (Ok(desc), Ok(size)) = (desc, size) {
                if size > 0 {
                    map.insert(desc, size);
                }
            }
        }
        map
    }

    pub fn detect_wmi_gpus() -> Vec<GpuInfo> {
        let controllers: Vec<Win32VideoController> = (|| {
            let com = wmi::COMLibrary::new()?;
            let conn = wmi::WMIConnection::new(com)?;
            conn.raw_query("SELECT Name, DriverVersion FROM Win32_VideoController")
        })()
        .unwrap_or_else(|e: wmi::WMIError| {
            log::warn!(target: "hw", "WMI video controller query failed: {e}");
            Vec::new()
        });

        let vram_map = registry_vram_by_desc();

        controllers
            .into_iter()
            .filter_map(|c| {
                let name = c.name?;
                // Virtual/remote display adapters are noise, not compute devices.
                let lower = name.to_ascii_lowercase();
                if lower.contains("microsoft") || lower.contains("virtual") {
                    return None;
                }
                let vram = vram_map.get(&name).copied();
                Some(GpuInfo {
                    vendor: classify_vendor(&name),
                    vram_total_bytes: vram,
                    vram_used_bytes: None,
                    driver_version: c.driver_version,
                    cuda_version: None,
                    temperature_c: None,
                    utilization_pct: None,
                    source: "wmi".into(),
                    name,
                })
            })
            .collect()
    }
}
