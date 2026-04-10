#[cfg(target_os = "macos")]
use crate::foundation_models_bridge::{
    BridgeSummaryDocument, BridgeSummaryError, BridgeSummaryRequest,
    summarize_transcript as summarize_transcript_bridge,
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
pub struct SummaryActionItem {
    pub owner: Option<String>,
    pub task: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SummaryDocument {
    pub overview: String,
    pub key_points: Vec<String>,
    pub decisions: Vec<String>,
    pub action_items: Vec<SummaryActionItem>,
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
pub fn summarize_transcript(request: SummaryRequest) -> Result<SummaryDocument, SummaryError> {
    let bridge_request = BridgeSummaryRequest {
        title: request.title,
        turns: request
            .turns
            .into_iter()
            .map(|turn| format!("{}: {}", turn.speaker.trim(), turn.text.trim()))
            .collect(),
    };
    let summary = summarize_transcript_bridge(bridge_request)?;
    Ok(SummaryDocument::from(summary))
}

#[cfg(not(target_os = "macos"))]
pub fn summarize_transcript(_request: SummaryRequest) -> Result<SummaryDocument, SummaryError> {
    Err(SummaryError::Internal {
        message: "local summaries require macOS".to_owned(),
    })
}

impl SummaryDocument {
    #[cfg(target_os = "macos")]
    fn from(summary: BridgeSummaryDocument) -> Self {
        let action_items = summary
            .action_item_tasks
            .into_iter()
            .enumerate()
            .map(|(index, task)| SummaryActionItem {
                owner: summary
                    .action_item_owners
                    .get(index)
                    .cloned()
                    .and_then(optional_not_empty),
                task,
            })
            .collect();

        Self {
            overview: summary.overview,
            key_points: summary.key_points,
            decisions: summary.decisions,
            action_items,
        }
    }
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

fn optional_not_empty(value: String) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }

    Some(trimmed.to_owned())
}
