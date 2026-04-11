use clap::ValueEnum;
use gemma4_coreml::Gemma4Variant;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[value(rename_all = "kebab-case")]
pub enum SummaryBackend {
    AppleFoundation,
    Gemma4E2b,
    Gemma4E4b,
}

impl SummaryBackend {
    pub const fn display_name(self) -> &'static str {
        match self {
            Self::AppleFoundation => "Apple Foundation",
            Self::Gemma4E2b => "Gemma 4 E2B",
            Self::Gemma4E4b => "Gemma 4 E4B",
        }
    }

    pub const fn gemma_variant(self) -> Option<Gemma4Variant> {
        match self {
            Self::AppleFoundation => None,
            Self::Gemma4E2b => Some(Gemma4Variant::E2b),
            Self::Gemma4E4b => Some(Gemma4Variant::E4b),
        }
    }

    pub const fn gemma_context(self) -> usize {
        match self {
            Self::AppleFoundation => 0,
            Self::Gemma4E2b | Self::Gemma4E4b => 1_024,
        }
    }

    pub const fn gemma_max_chunk_chars(self) -> usize {
        match self {
            Self::AppleFoundation => 0,
            Self::Gemma4E2b | Self::Gemma4E4b => 2_000,
        }
    }

    pub const fn gemma_max_new_tokens(self) -> usize {
        match self {
            Self::AppleFoundation => 0,
            Self::Gemma4E2b | Self::Gemma4E4b => 192,
        }
    }
}
