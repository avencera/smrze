#[swift_bridge::bridge]
mod foundation_models_ffi {
    #[swift_bridge(swift_repr = "struct")]
    struct BridgeSummaryRequest {
        title: String,
        turns: Vec<String>,
    }

    #[swift_bridge(swift_repr = "struct")]
    struct BridgeSummaryDocument {
        overview: String,
        key_points: Vec<String>,
        decisions: Vec<String>,
        action_item_owners: Vec<String>,
        action_item_tasks: Vec<String>,
    }

    enum BridgeSummaryError {
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

    extern "Swift" {
        #[swift_bridge(swift_name = "summarizeTranscript")]
        fn summarize_transcript(
            request: BridgeSummaryRequest,
        ) -> Result<BridgeSummaryDocument, BridgeSummaryError>;
    }
}

pub use foundation_models_ffi::{
    BridgeSummaryDocument, BridgeSummaryError, BridgeSummaryRequest, summarize_transcript,
};
