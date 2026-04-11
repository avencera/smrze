#[cfg(target_os = "macos")]
use crate::foundation_models_bridge::{
    BridgeGemmaError, BridgeGemmaRequest, generate_gemma_text as generate_gemma_text_bridge,
};

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
) -> Result<String, GemmaError> {
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
