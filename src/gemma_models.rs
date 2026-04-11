#[cfg(target_os = "macos")]
use crate::foundation_models_bridge::{
    BridgeGemmaError, BridgeGemmaRequest, generate_gemma_text as generate_gemma_text_bridge,
};
#[cfg(target_os = "macos")]
use crate::mlx_runtime::{MlxMetallibAsset, MlxRuntimeError};
use crate::paths::AppPaths;
#[cfg(target_os = "macos")]
use std::{env, sync::OnceLock};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GemmaError {
    InvalidModelPath { message: String },
    DownloadFailure { message: String },
    LoadFailure { message: String },
    GenerateFailure { message: String },
    Internal { message: String },
}

#[cfg(target_os = "macos")]
pub fn generate_gemma_text(
    model_id: String,
    local_model_path: Option<String>,
    prompt: String,
    max_new_tokens: usize,
    app_paths: &AppPaths,
) -> Result<String, GemmaError> {
    ensure_mlx_metallib(app_paths)?;

    let response = generate_gemma_text_bridge(BridgeGemmaRequest {
        model_id,
        local_model_path,
        prompt,
        max_new_tokens,
    })?;
    Ok(response.text)
}

#[cfg(not(target_os = "macos"))]
pub fn generate_gemma_text(
    _model_id: String,
    _local_model_path: Option<String>,
    _prompt: String,
    _max_new_tokens: usize,
    _app_paths: &AppPaths,
) -> Result<String, GemmaError> {
    Err(GemmaError::Internal {
        message: "local Gemma summaries require macOS".to_owned(),
    })
}

impl GemmaError {
    pub fn message(&self) -> &str {
        match self {
            Self::InvalidModelPath { message }
            | Self::DownloadFailure { message }
            | Self::LoadFailure { message }
            | Self::GenerateFailure { message }
            | Self::Internal { message } => message,
        }
    }
}

#[cfg(target_os = "macos")]
fn ensure_mlx_metallib(app_paths: &AppPaths) -> Result<(), GemmaError> {
    static MLX_METALLIB_PATH: OnceLock<Result<String, GemmaError>> = OnceLock::new();

    let metallib_path = MLX_METALLIB_PATH
        .get_or_init(|| {
            let asset = MlxMetallibAsset::from_app_paths(app_paths).map_err(map_runtime_error)?;
            let path = asset.ensure_available().map_err(map_runtime_error)?;
            Ok(path.display().to_string())
        })
        .as_ref()
        .map_err(Clone::clone)?;
    unsafe {
        env::set_var("MLX_METAL_LIBRARY_PATH", metallib_path);
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn map_runtime_error(error: MlxRuntimeError) -> GemmaError {
    match error {
        MlxRuntimeError::DownloadFailure { message }
        | MlxRuntimeError::IntegrityFailure { message } => GemmaError::DownloadFailure { message },
        MlxRuntimeError::UnsupportedArch { message }
        | MlxRuntimeError::InstallFailure { message } => GemmaError::Internal { message },
    }
}

#[cfg(target_os = "macos")]
impl From<BridgeGemmaError> for GemmaError {
    fn from(error: BridgeGemmaError) -> Self {
        match error {
            BridgeGemmaError::InvalidModelPath { message } => Self::InvalidModelPath { message },
            BridgeGemmaError::DownloadFailure { message } => Self::DownloadFailure { message },
            BridgeGemmaError::LoadFailure { message } => Self::LoadFailure { message },
            BridgeGemmaError::GenerateFailure { message } => Self::GenerateFailure { message },
            BridgeGemmaError::Internal { message } => Self::Internal { message },
        }
    }
}
