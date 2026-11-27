//! Whether this computer can run the model fast enough: memory, cores, and
//! AVX2 on the processors whose runtime needs it.

use super::catalog::ModelTier;

pub struct Recommendation {
    pub tier: Option<ModelTier>,
    pub ram_gb: f64,
    pub cores: usize,
    /// The processor can run the shipped runtime: AVX2 on x86_64, and any
    /// aarch64 build.
    pub avx2: bool,
}

pub fn recommend() -> Recommendation {
    let ram_gb = total_ram_gb();
    let cores = std::thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(1);
    let avx2 = capable_processor();
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

#[cfg(target_os = "linux")]
fn total_ram_gb() -> f64 {
    std::fs::read_to_string("/proc/meminfo")
        .ok()
        .and_then(|raw| total_ram_gb_from_meminfo(&raw))
        .unwrap_or(0.0)
}

#[cfg(not(target_os = "linux"))]
fn total_ram_gb() -> f64 {
    0.0
}

/// Total physical memory from a `/proc/meminfo` body, in GiB.
///
/// Every line is `Name: <number> kB`; only `MemTotal` is needed. A body
/// without it (or with a malformed number) reports nothing so the caller can
/// fall back to "not enough memory" instead of guessing.
fn total_ram_gb_from_meminfo(raw: &str) -> Option<f64> {
    let line = raw
        .lines()
        .find(|line| line.trim_start().starts_with("MemTotal:"))?;
    let kilobytes: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kilobytes as f64 / (1024.0 * 1024.0))
}

#[cfg(target_arch = "x86_64")]
fn capable_processor() -> bool {
    std::arch::is_x86_feature_detected!("avx2")
}

#[cfg(target_arch = "aarch64")]
fn capable_processor() -> bool {
    // The aarch64 runtime is built for the base instruction set every
    // supported ARM64 desktop has, so no feature probe is needed.
    true
}

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
fn capable_processor() -> bool {
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

    #[test]
    fn too_few_cores_disables_ai_names() {
        assert_eq!(recommend_for(32.0, 3, true), None);
    }

    #[test]
    fn meminfo_totals_the_physical_memory() {
        let raw = "MemTotal:       16384000 kB\nMemFree:         1000 kB\n";
        let ram = total_ram_gb_from_meminfo(raw).unwrap();
        assert!((ram - 15.625).abs() < 0.0001);
    }

    #[test]
    fn meminfo_without_memtotal_reports_nothing() {
        assert_eq!(total_ram_gb_from_meminfo("MemFree: 1000 kB\n"), None);
    }

    #[test]
    fn meminfo_with_a_malformed_total_reports_nothing() {
        assert_eq!(total_ram_gb_from_meminfo("MemTotal: lots kB\n"), None);
    }
}
