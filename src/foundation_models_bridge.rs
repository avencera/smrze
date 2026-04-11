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

    #[swift_bridge(swift_repr = "struct")]
    struct BridgeGemmaRequest {
        model_id: String,
        local_model_path: Option<String>,
        prompt: String,
        max_new_tokens: usize,
    }

    #[swift_bridge(swift_repr = "struct")]
    struct BridgeGemmaResponse {
        text: String,
    }

    enum BridgeGemmaError {
        InvalidModelPath { message: String },
        DownloadFailure { message: String },
        LoadFailure { message: String },
        GenerateFailure { message: String },
        Internal { message: String },
    }

    extern "Swift" {
        #[swift_bridge(swift_name = "summarizeTranscript")]
        fn summarize_transcript(
            request: BridgeSummaryRequest,
        ) -> Result<BridgeSummaryDocument, BridgeSummaryError>;

        #[swift_bridge(swift_name = "generateGemmaText")]
        fn generate_gemma_text(
            request: BridgeGemmaRequest,
        ) -> Result<BridgeGemmaResponse, BridgeGemmaError>;
    }
}

pub use foundation_models_ffi::{
    BridgeGemmaError, BridgeGemmaRequest, BridgeSummaryDocument, BridgeSummaryError,
    BridgeSummaryRequest, generate_gemma_text, summarize_transcript,
};
