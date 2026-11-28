//! The model and the llama.cpp runtime, and where they live on disk.

use std::path::PathBuf;

/// The model AI names use. One for now: a smaller one named the app instead
/// of the content too often to be worth offering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelTier {
    Standard,
}

pub struct ModelSpec {
    pub tier: ModelTier,
    pub label: &'static str,
    pub detail: &'static str,
    pub file_name: &'static str,
    pub url: &'static str,
    pub sha256: &'static str,
    pub bytes: u64,
}

pub struct RuntimeSpec {
    pub url: &'static str,
    pub sha256: &'static str,
    pub bytes: u64,
    pub version: &'static str,
}

static STANDARD: ModelSpec = ModelSpec {
    tier: ModelTier::Standard,
    label: "Standard",
    detail: "Qwen3.5 2B",
    file_name: "qwen3.5-2b-q4_k_m.gguf",
    url: concat!(
        "https://huggingface.co/unsloth/Qwen3.5-2B-GGUF/resolve/main/",
        "Qwen3.5-2B-Q4_K_M.gguf"
    ),
    sha256: "aaf42c8b7c3cab2bf3d69c355048d4a0ee9973d48f16c731c0520ee914699223",
    bytes: 1_281_000_000,
};

#[cfg(target_arch = "x86_64")]
static RUNTIME: RuntimeSpec = RuntimeSpec {
    url: concat!(
        "https://github.com/ggml-org/llama.cpp/releases/download/",
        "b12815/llama-b12815-bin-ubuntu-x64.tar.gz"
    ),
    sha256: "026ecc42e2b8ba93c5d0508a9715f0921aea89a88df6d75eb17831af0ef250e1",
    bytes: 23_491_803,
    version: "b12815",
};

#[cfg(target_arch = "aarch64")]
static RUNTIME: RuntimeSpec = RuntimeSpec {
    url: concat!(
        "https://github.com/ggml-org/llama.cpp/releases/download/",
        "b12815/llama-b12815-bin-ubuntu-arm64.tar.gz"
    ),
    sha256: "8cc53cbe39dcb150d4b3f6c57c61b4bb523f3b213059162b37a43adf996e0029",
    bytes: 19_050_813,
    version: "b12815",
};

pub fn model_spec(tier: ModelTier) -> &'static ModelSpec {
    match tier {
        ModelTier::Standard => &STANDARD,
    }
}

/// The runtime build for this processor, or None on an architecture the
/// shipped binaries do not cover.
pub fn runtime_spec() -> Option<&'static RuntimeSpec> {
    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    {
        Some(&RUNTIME)
    }
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    {
        None
    }
}

pub fn tier_from_setting(_value: &str) -> ModelTier {
    ModelTier::Standard
}

pub fn tier_setting(tier: ModelTier) -> &'static str {
    match tier {
        ModelTier::Standard => "standard",
    }
}

pub fn ai_dir() -> Option<PathBuf> {
    dirs::data_local_dir().map(|dir| dir.join("apexshot").join("ai"))
}

pub fn model_path(tier: ModelTier) -> Option<PathBuf> {
    ai_dir().map(|dir| dir.join("models").join(model_spec(tier).file_name))
}

pub fn runtime_dir() -> Option<PathBuf> {
    runtime_spec().and_then(|spec| ai_dir().map(|dir| dir.join("runtime").join(spec.version)))
}

pub fn runtime_exe() -> Option<PathBuf> {
    runtime_dir().map(|dir| dir.join("llama-server"))
}

pub fn is_installed(tier: ModelTier) -> bool {
    model_path(tier).is_some_and(|path| path.is_file())
        && runtime_exe().is_some_and(|path| path.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_model_spec_is_stable() {
        let spec = model_spec(ModelTier::Standard);
        assert_eq!(spec.file_name, "qwen3.5-2b-q4_k_m.gguf");
        assert_eq!(spec.bytes, 1_281_000_000);
        assert!(spec.url.to_lowercase().ends_with(spec.file_name));
    }

    #[test]
    fn the_setting_round_trips_through_the_tier() {
        assert_eq!(
            tier_from_setting(tier_setting(ModelTier::Standard)),
            ModelTier::Standard
        );
        assert_eq!(tier_setting(ModelTier::Standard), "standard");
        // Unknown values fall back to the only tier there is.
        assert_eq!(tier_from_setting("something else"), ModelTier::Standard);
    }

    #[test]
    fn the_runtime_executable_has_no_extension_on_linux() {
        if let Some(exe) = runtime_exe() {
            assert_eq!(exe.file_name().unwrap().to_string_lossy(), "llama-server");
        }
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn the_runtime_matches_x86_64() {
        let spec = runtime_spec().expect("x86_64 ships a runtime");
        assert_eq!(spec.version, "b12815");
        assert!(spec.url.ends_with("llama-b12815-bin-ubuntu-x64.tar.gz"));
        assert_eq!(spec.bytes, 23_491_803);
        assert_eq!(
            spec.sha256,
            "026ecc42e2b8ba93c5d0508a9715f0921aea89a88df6d75eb17831af0ef250e1"
        );
    }

    #[cfg(target_arch = "aarch64")]
    #[test]
    fn the_runtime_matches_aarch64() {
        let spec = runtime_spec().expect("aarch64 ships a runtime");
        assert_eq!(spec.version, "b12815");
        assert!(spec.url.ends_with("llama-b12815-bin-ubuntu-arm64.tar.gz"));
        assert_eq!(spec.bytes, 19_050_813);
        assert_eq!(
            spec.sha256,
            "8cc53cbe39dcb150d4b3f6c57c61b4bb523f3b213059162b37a43adf996e0029"
        );
    }

    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    #[test]
    fn other_arches_have_no_runtime() {
        assert!(runtime_spec().is_none());
        assert!(runtime_dir().is_none());
        assert!(runtime_exe().is_none());
    }
}
