use clap::ValueEnum;

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

    pub const fn cache_key(self) -> &'static str {
        match self {
            Self::AppleFoundation => "apple-foundation",
            Self::Gemma4E2b => "gemma4-e2b",
            Self::Gemma4E4b => "gemma4-e4b",
        }
    }

    pub const fn gemma_variant(self) -> Option<GemmaVariant> {
        match self {
            Self::AppleFoundation => None,
            Self::Gemma4E2b => Some(GemmaVariant::E2b),
            Self::Gemma4E4b => Some(GemmaVariant::E4b),
        }
    }

    pub const fn gemma_max_new_tokens(self) -> usize {
        match self {
            Self::AppleFoundation => 0,
            Self::Gemma4E2b | Self::Gemma4E4b => 192,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GemmaVariant {
    E2b,
    E4b,
}

impl GemmaVariant {
    pub const fn model_id(self) -> &'static str {
        match self {
            Self::E2b => "mlx-community/gemma-4-e2b-it-4bit",
            Self::E4b => "mlx-community/gemma-4-e4b-it-4bit",
        }
    }

    pub const fn dir_name(self) -> &'static str {
        match self {
            Self::E2b => "gemma-4-e2b-it",
            Self::E4b => "gemma-4-e4b-it",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{GemmaVariant, SummaryBackend};

    #[test]
    fn gemma_backend_maps_to_expected_model_ids() {
        assert_eq!(
            SummaryBackend::Gemma4E2b
                .gemma_variant()
                .unwrap()
                .model_id(),
            "mlx-community/gemma-4-e2b-it-4bit"
        );
        assert_eq!(
            SummaryBackend::Gemma4E4b
                .gemma_variant()
                .unwrap()
                .model_id(),
            "mlx-community/gemma-4-e4b-it-4bit"
        );
    }

    #[test]
    fn gemma_variant_dir_names_match_cli_layout() {
        assert_eq!(GemmaVariant::E2b.dir_name(), "gemma-4-e2b-it");
        assert_eq!(GemmaVariant::E4b.dir_name(), "gemma-4-e4b-it");
    }
}
