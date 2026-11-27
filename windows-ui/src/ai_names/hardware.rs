//! Whether this computer can run the model fast enough: memory, cores, and AVX2.

use super::catalog::ModelTier;

pub struct Recommendation {
    pub tier: Option<ModelTier>,
    pub ram_gb: f64,
    pub cores: usize,
    pub avx2: bool,
}

pub fn recommend() -> Recommendation {
    let ram_gb = total_ram_gb();
    let cores = std::thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(1);
    let avx2 = avx2_available();
    Recommendation {
        tier: recommend_for(ram_gb, cores, avx2),
        ram_gb,
        cores,
        avx2,
    }
}

pub fn recommend_for(ram_gb: f64, cores: usize, avx2: bool) -> Option<ModelTier> {
    // The model needs about 2 GB of memory while it names a capture.
    if !avx2 || ram_gb < 7.5 || cores < 4 {
        None
    } else {
        Some(ModelTier::Standard)
    }
}

#[cfg(target_os = "windows")]
fn total_ram_gb() -> f64 {
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};

    let mut status: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
    status.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
    if unsafe { GlobalMemoryStatusEx(&mut status) } == 0 {
        return 0.0;
    }
    status.ullTotalPhys as f64 / (1024.0 * 1024.0 * 1024.0)
}

#[cfg(not(target_os = "windows"))]
fn total_ram_gb() -> f64 {
    0.0
}

#[cfg(target_arch = "x86_64")]
fn avx2_available() -> bool {
    std::arch::is_x86_feature_detected!("avx2")
}

#[cfg(not(target_arch = "x86_64"))]
fn avx2_available() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_typical_machine_gets_the_model() {
        assert_eq!(recommend_for(32.0, 20, true), Some(ModelTier::Standard));
        assert_eq!(recommend_for(8.0, 4, true), Some(ModelTier::Standard));
    }

    #[test]
    fn too_little_memory_disables_ai_names() {
        assert_eq!(recommend_for(4.0, 8, true), None);
    }

    #[test]
    fn a_machine_without_avx2_disables_ai_names() {
        assert_eq!(recommend_for(32.0, 20, false), None);
    }
}
