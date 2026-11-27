//! The models and the llama.cpp runtime, and where they live on disk.

use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelTier {
    Fast,
    Better,
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

static FAST: ModelSpec = ModelSpec {
    tier: ModelTier::Fast,
    label: "Fast",
    detail: "Qwen3.5 0.8B",
    file_name: "qwen3.5-0.8b-q4_k_m.gguf",
    url: concat!(
        "https://huggingface.co/unsloth/Qwen3.5-0.8B-GGUF/resolve/main/",
        "Qwen3.5-0.8B-Q4_K_M.gguf"
    ),
    sha256: "bd258782e35f7f458f8aced1adc053e6e92e89bc735ba3be89d38a06121dc517",
    bytes: 533_000_000,
};

static BETTER: ModelSpec = ModelSpec {
    tier: ModelTier::Better,
    label: "Better",
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
        ModelTier::Fast => &FAST,
        ModelTier::Better => &BETTER,
    }
}

pub fn runtime_spec() -> &'static RuntimeSpec {
    &RUNTIME
}

pub fn tier_from_setting(value: &str) -> ModelTier {
    match value.trim() {
        "better" => ModelTier::Better,
        _ => ModelTier::Fast,
    }
}

pub fn tier_setting(tier: ModelTier) -> &'static str {
    match tier {
        ModelTier::Fast => "fast",
        ModelTier::Better => "better",
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
    model_path(tier).map_or(false, |path| path.is_file())
        && runtime_exe().map_or(false, |path| path.is_file())
}
