#[cfg(target_os = "macos")]
use crate::foundation_models_bridge::{
    BridgeSummaryError, BridgeSummaryRequest, summarize_transcript as summarize_transcript_bridge,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SummaryTurn {
    pub speaker: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SummaryRequest {
    pub title: String,
    pub turns: Vec<SummaryTurn>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SummaryError {
    DeviceNotEligible,
    AppleIntelligenceNotEnabled,
    ModelNotReady,
    UnsupportedLocale { message: String },
    ExceededContextWindow { message: String },
    GuardrailViolation { message: String },
    Refusal { message: String },
    DecodingFailure { message: String },
    RateLimited { message: String },
    ConcurrentRequests { message: String },
    Internal { message: String },
}

#[cfg(target_os = "macos")]
pub fn summarize_transcript(request: SummaryRequest) -> Result<String, SummaryError> {
    let bridge_request = BridgeSummaryRequest {
        title: request.title,
        turns: request
            .turns
            .into_iter()
            .map(|turn| {
                if turn.speaker.trim().is_empty() {
                    return turn.text.trim().to_owned();
                }

                format!("{}: {}", turn.speaker.trim(), turn.text.trim())
            })
            .collect(),
    };
    let summary = summarize_transcript_bridge(bridge_request)?;
    Ok(summary.text)
}

#[cfg(not(target_os = "macos"))]
pub fn summarize_transcript(_request: SummaryRequest) -> Result<String, SummaryError> {
    Err(SummaryError::Internal {
        message: "local summaries require macOS".to_owned(),
    })
}

impl SummaryError {
    pub fn message(&self) -> String {
        match self {
            Self::DeviceNotEligible => {
                "Apple Intelligence is unavailable because this Mac is not eligible".to_owned()
            }
            Self::AppleIntelligenceNotEnabled => {
                "Apple Intelligence is not enabled on this Mac".to_owned()
            }
            Self::ModelNotReady => {
                "Apple Intelligence models are not ready yet on this Mac".to_owned()
            }
            Self::UnsupportedLocale { message }
            | Self::ExceededContextWindow { message }
            | Self::GuardrailViolation { message }
            | Self::Refusal { message }
            | Self::DecodingFailure { message }
            | Self::RateLimited { message }
            | Self::ConcurrentRequests { message }
            | Self::Internal { message } => message.clone(),
        }
    }
}

#[cfg(target_os = "macos")]
impl From<BridgeSummaryError> for SummaryError {
    fn from(error: BridgeSummaryError) -> Self {
        match error {
            BridgeSummaryError::DeviceNotEligible => Self::DeviceNotEligible,
            BridgeSummaryError::AppleIntelligenceNotEnabled => Self::AppleIntelligenceNotEnabled,
            BridgeSummaryError::ModelNotReady => Self::ModelNotReady,
            BridgeSummaryError::UnsupportedLocale { message } => {
                Self::UnsupportedLocale { message }
            }
            BridgeSummaryError::ExceededContextWindow { message } => {
                Self::ExceededContextWindow { message }
            }
            BridgeSummaryError::GuardrailViolation { message } => {
                Self::GuardrailViolation { message }
            }
            BridgeSummaryError::Refusal { message } => Self::Refusal { message },
            BridgeSummaryError::DecodingFailure { message } => Self::DecodingFailure { message },
            BridgeSummaryError::RateLimited { message } => Self::RateLimited { message },
            BridgeSummaryError::ConcurrentRequests { message } => {
                Self::ConcurrentRequests { message }
            }
            BridgeSummaryError::Internal { message } => Self::Internal { message },
        }
    }
}
