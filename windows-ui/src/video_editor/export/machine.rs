//! What this machine can give an export: cores, memory, and the GPU the
//! compositor should use. Detected once per process; every policy below is a
//! pure function of it so the low-end cases can be tested on any machine.
//!
//! Overrides for testing a weaker machine: `APEXSHOT_EXPORT_RAM_MB=<n>` pretends
//! the machine has n MB, all of it free; `APEXSHOT_EXPORT_GPU=0` pretends it has
//! no usable GPU; `APEXSHOT_EXPORT_ADAPTER=high-performance` picks the
//! high-performance adapter instead of the low-power one.
//!
//! The low-power adapter is the default. Every frame is uploaded and read back,
//! and on a hybrid laptop the integrated GPU shares system RAM with the CPU and
//! the Quick Sync encoder: on the development machine (Iris Xe beside an RTX
//! A500) a zoomed 1080p export ran 25-27 ms/frame on the integrated GPU and
//! 26-97 ms/frame on the dedicated one, which also slowed exports that never
//! used it by waking it. A desktop with one card still gets that card.

use std::sync::OnceLock;

const MIB: u64 = 1024 * 1024;

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Gpu {
    pub name: String,
    pub vendor: u32,
    /// Memory on the card itself; integrated GPUs report little or none.
    pub dedicated_memory: u64,
    /// What the OS lets this process use on the GPU, or 0 when unknown.
    pub budget: u64,
    /// The adapter's LUID, to open a device on exactly this adapter.
    pub luid: (u32, i32),
}

impl Gpu {
    /// Under 512 MiB of its own memory: an integrated GPU that shares RAM.
    pub(super) fn integrated(&self) -> bool {
        self.dedicated_memory < 512 * MIB
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Machine {
    pub cores: usize,
    pub total_ram: u64,
    pub available_ram: u64,
    /// The hardware GPU to composite on; None with no GPU or only a software one.
    pub gpu: Option<Gpu>,
}

/// The machine, detected on first use.
pub(super) fn current() -> &'static Machine {
    static MACHINE: OnceLock<Machine> = OnceLock::new();
    MACHINE.get_or_init(|| {
        apply_overrides(
            detect(),
            std::env::var("APEXSHOT_EXPORT_RAM_MB").ok().as_deref(),
            std::env::var("APEXSHOT_EXPORT_GPU").ok().as_deref(),
        )
    })
}

fn apply_overrides(mut machine: Machine, ram_mb: Option<&str>, gpu: Option<&str>) -> Machine {
    if let Some(mb) = ram_mb.and_then(|value| value.trim().parse::<u64>().ok()) {
        machine.total_ram = mb * MIB;
        machine.available_ram = mb * MIB;
    }
    if gpu == Some("0") {
        machine.gpu = None;
    }
    machine
}

fn detect() -> Machine {
    let cores = std::thread::available_parallelism().map_or(1, |n| n.get());
    #[cfg(not(target_os = "windows"))]
    {
        Machine {
            cores,
            total_ram: u64::MAX,
            available_ram: u64::MAX,
            gpu: None,
        }
    }
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};

        let mut status = MEMORYSTATUSEX {
            dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
            ..Default::default()
        };
        let (total_ram, available_ram) = unsafe {
            if GlobalMemoryStatusEx(&mut status).is_ok() {
                (status.ullTotalPhys, status.ullAvailPhys)
            } else {
                (u64::MAX, u64::MAX)
            }
        };
        Machine {
            cores,
            total_ram,
            available_ram,
            gpu: detect_gpu(),
        }
    }
}

#[cfg(target_os = "windows")]
fn detect_gpu() -> Option<Gpu> {
    use windows::core::Interface;
    use windows::Win32::Graphics::Dxgi::*;

    unsafe {
        let factory: IDXGIFactory1 = CreateDXGIFactory1().ok()?;
        let mut candidates = Vec::new();
        if let Ok(factory6) = factory.cast::<IDXGIFactory6>() {
            for index in 0..16 {
                match factory6.EnumAdapterByGpuPreference::<IDXGIAdapter1>(
                    index,
                    if std::env::var("APEXSHOT_EXPORT_ADAPTER").ok().as_deref()
                        == Some("high-performance")
                    {
                        DXGI_GPU_PREFERENCE_HIGH_PERFORMANCE
                    } else {
                        DXGI_GPU_PREFERENCE_MINIMUM_POWER
                    },
                ) {
                    Ok(adapter) => candidates.push(adapter),
                    Err(_) => break,
                }
            }
        } else {
            for index in 0..16 {
                match factory.EnumAdapters1(index) {
                    Ok(adapter) => candidates.push(adapter),
                    Err(_) => break,
                }
            }
        }

        for adapter in candidates {
            let Ok(desc) = adapter.GetDesc1() else {
                continue;
            };
            if (desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32) != 0 {
                continue;
            }
            if desc.VendorId == 0x1414 && desc.DeviceId == 0x8c {
                continue;
            }
            let name_end = desc
                .Description
                .iter()
                .position(|&unit| unit == 0)
                .unwrap_or(desc.Description.len());
            let budget = match adapter.cast::<IDXGIAdapter3>() {
                Ok(adapter3) => {
                    let mut info = DXGI_QUERY_VIDEO_MEMORY_INFO::default();
                    if adapter3
                        .QueryVideoMemoryInfo(0, DXGI_MEMORY_SEGMENT_GROUP_LOCAL, &mut info)
                        .is_ok()
                    {
                        info.Budget
                    } else {
                        0
                    }
                }
                Err(_) => 0,
            };
            return Some(Gpu {
                name: String::from_utf16_lossy(&desc.Description[..name_end]),
                vendor: desc.VendorId,
                dedicated_memory: desc.DedicatedVideoMemory as u64,
                budget,
                luid: (desc.AdapterLuid.LowPart, desc.AdapterLuid.HighPart),
            });
        }
        None
    }
}

/// Open the adapter the profile chose, to create a device on exactly it.
#[cfg(target_os = "windows")]
pub(super) fn adapter(gpu: &Gpu) -> Option<windows::Win32::Graphics::Dxgi::IDXGIAdapter> {
    use windows::Win32::Foundation::LUID;
    use windows::Win32::Graphics::Dxgi::*;

    unsafe {
        let factory: IDXGIFactory4 = CreateDXGIFactory1().ok()?;
        factory
            .EnumAdapterByLuid::<IDXGIAdapter>(LUID {
                LowPart: gpu.luid.0,
                HighPart: gpu.luid.1,
            })
            .ok()
    }
}

/// Bytes one parallel range holds: its frame buffers, the encoder pipe, and
/// its own ffmpeg decoder and encoder processes.
pub(super) fn range_bytes(source: (u32, u32), output: (u32, u32), software_encoder: bool) -> u64 {
    let source_px = u64::from(source.0) * u64::from(source.1);
    let output_px = u64::from(output.0) * u64::from(output.1);
    let buffers = source_px * 4 * 3 + output_px * 4 * 4 + 32 * MIB;
    let decoder = source_px * 12;
    let encoder = if software_encoder {
        output_px * 60
    } else {
        64 * MIB
    };
    buffers + decoder + encoder
}

/// How many ranges fit in half of the free memory, at least one.
pub(super) fn memory_workers(
    machine: &Machine,
    source: (u32, u32),
    output: (u32, u32),
    software_encoder: bool,
) -> usize {
    let per_range = range_bytes(source, output, software_encoder).max(1);
    ((machine.available_ram / 2) / per_range).clamp(1, 64) as usize
}

/// Whether the GPU stage's textures fit: a hardware GPU, half its budget (when
/// known), and for an integrated GPU, which takes them from system RAM, half
/// the free memory too.
pub(super) fn gpu_stage_fits(machine: &Machine, source: (u32, u32), output: (u32, u32)) -> bool {
    let Some(gpu) = machine.gpu.as_ref() else {
        return false;
    };
    let source_px = u64::from(source.0) * u64::from(source.1);
    let output_px = u64::from(output.0) * u64::from(output.1);
    let need = source_px * 4 * 4 / 3 + source_px * 3 + output_px * 4 * 2 + output_px * 3;
    let fits_budget = gpu.budget == 0 || need * 2 <= gpu.budget;
    let fits_ram = !gpu.integrated() || need * 2 <= machine.available_ram;
    fits_budget && fits_ram
}

/// One line for the export log.
pub(super) fn describe(machine: &Machine) -> String {
    let gb = |bytes: u64| bytes as f64 / (1024.0 * MIB as f64);
    let gpu = match &machine.gpu {
        Some(gpu) => format!(
            "{} ({}, {:.1} GB own, budget {:.1} GB)",
            gpu.name,
            if gpu.integrated() {
                "integrated"
            } else {
                "dedicated"
            },
            gb(gpu.dedicated_memory),
            gb(gpu.budget)
        ),
        None => "no hardware GPU".to_string(),
    };
    format!(
        "{} cores, {:.1} GB RAM ({:.1} GB free), {gpu}",
        machine.cores,
        gb(machine.total_ram),
        gb(machine.available_ram)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn machine(ram_gb: u64, gpu: Option<Gpu>) -> Machine {
        let ram = ram_gb * 1024 * MIB;
        Machine {
            cores: 8,
            total_ram: ram,
            available_ram: ram,
            gpu,
        }
    }

    fn gpu(own_mb: u64, budget_mb: u64) -> Gpu {
        Gpu {
            name: "test adapter".to_string(),
            vendor: 0x10de,
            dedicated_memory: own_mb * MIB,
            budget: budget_mb * MIB,
            luid: (0, 0),
        }
    }

    #[test]
    fn a_low_ram_laptop_runs_fewer_ranges() {
        // 4K: buffers 99,532,800 + 132,710,400 + 33,554,432 = 265,797,632,
        // decoder 99,532,800, software encoder 497,664,000, so 862,994,432
        // bytes a range. Half of 4 GiB is 2,147,483,648: two ranges fit and a
        // third does not.
        assert_eq!(
            memory_workers(&machine(4, None), (3840, 2160), (3840, 2160), true),
            2
        );
        assert!(
            memory_workers(&machine(32, None), (3840, 2160), (3840, 2160), true) >= 4,
            "a 32 GB machine should run at least four 4K ranges"
        );
    }

    #[test]
    fn at_least_one_range_always_runs() {
        let mut low = machine(4, None);
        low.available_ram = 0;
        assert_eq!(memory_workers(&low, (3840, 2160), (3840, 2160), true), 1);
    }

    #[test]
    fn no_gpu_means_no_gpu_stage() {
        assert!(!gpu_stage_fits(
            &machine(16, None),
            (1920, 1080),
            (1920, 1080)
        ));
    }

    #[test]
    fn an_integrated_gpu_needs_free_ram_for_its_textures() {
        let integrated = machine(16, Some(gpu(128, 0)));
        assert!(gpu_stage_fits(&integrated, (1920, 1080), (1920, 1080)));
        let mut starved = integrated;
        starved.available_ram = 10 * MIB;
        assert!(!gpu_stage_fits(&starved, (1920, 1080), (1920, 1080)));
    }

    #[test]
    fn a_small_gpu_budget_keeps_4k_on_the_cpu() {
        // 4K needs 160,358,400 bytes of textures, so 320,716,800 twice over;
        // 720p needs 17,817,600, so 35,635,200 twice over, inside 64 MiB.
        let small = machine(16, Some(gpu(2048, 64)));
        assert!(!gpu_stage_fits(&small, (3840, 2160), (3840, 2160)));
        assert!(gpu_stage_fits(&small, (1280, 720), (1280, 720)));
    }

    #[test]
    fn overrides_pretend_a_weaker_machine() {
        let strong = machine(32, Some(gpu(8192, 8192)));
        let weak = apply_overrides(strong.clone(), Some("2048"), Some("0"));
        assert_eq!(weak.total_ram, 2048 * MIB);
        assert_eq!(weak.available_ram, 2048 * MIB);
        assert_eq!(weak.gpu, None);
        assert_eq!(apply_overrides(strong.clone(), None, None), strong);
    }

    #[test]
    fn the_machine_is_detected() {
        assert!(current().cores >= 1);
        assert!(current().total_ram > 0);
        eprintln!("{}", describe(current()));
    }

    #[test]
    fn the_description_names_the_gpu_kind() {
        assert!(describe(&machine(8, Some(gpu(128, 0)))).contains("integrated"));
        assert!(describe(&machine(8, Some(gpu(8192, 8192)))).contains("dedicated"));
        assert!(describe(&machine(8, None)).contains("no hardware GPU"));
    }
}
