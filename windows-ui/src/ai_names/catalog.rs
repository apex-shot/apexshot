//! The models and the llama.cpp runtime, and where they live on disk.

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

static RUNTIME: RuntimeSpec = RuntimeSpec {
    url: concat!(
        "https://github.com/ggml-org/llama.cpp/releases/download/",
        "b12815/llama-b12815-bin-win-cpu-x64.zip"
    ),
    sha256: "8167d30d05420295ade9b9db3e73d0325aca75b72ced25c030d5dd3d7b0fde5a",
    bytes: 25_351_642,
    version: "b12815",
};

pub fn model_spec(tier: ModelTier) -> &'static ModelSpec {
    match tier {
        ModelTier::Standard => &STANDARD,
    }
}

pub fn runtime_spec() -> &'static RuntimeSpec {
    &RUNTIME
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
    ai_dir().map(|dir| dir.join("runtime").join(runtime_spec().version))
}

pub fn runtime_exe() -> Option<PathBuf> {
    runtime_dir().map(|dir| dir.join("llama-server.exe"))
}

pub fn is_installed(tier: ModelTier) -> bool {
    model_path(tier).is_some_and(|path| path.is_file())
        && runtime_exe().is_some_and(|path| path.is_file())
}
