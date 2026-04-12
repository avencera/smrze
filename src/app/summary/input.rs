use color_eyre::{Result, eyre::Context};
use std::fs;
use std::path::Path;

use super::super::transcription::TranscriptionPipeline;
use crate::input::{
    ResolvedMediaInput, is_url, local_file_source_key, resolve_existing_path, resolve_media_input,
};
use crate::speakers::SpeakerTurn;
use crate::transcript::parse_transcript;
use crate::utils::{file_stem_name, hash_string, sanitize_name};

#[derive(Debug, Clone)]
pub(super) struct SummaryInput {
    pub(super) display_name: String,
    pub(super) source_key: String,
    pub(super) transcript_hash: String,
    pub(super) turns: Vec<SpeakerTurn>,
}

pub(crate) struct SummaryInputResolver<'a> {
    transcription: &'a TranscriptionPipeline<'a>,
}

impl<'a> SummaryInputResolver<'a> {
    pub(super) fn new(transcription: &'a TranscriptionPipeline<'a>) -> Self {
        Self { transcription }
    }

    pub(super) fn resolve(&self, input: &str) -> Result<SummaryInput> {
        if is_url(input) {
            return self.summary_input_from_media(&resolve_media_input(input)?);
        }

        let path = resolve_existing_path(input)?;
        if let Some(summary_input) = self.try_load_transcript_file(&path)? {
            return Ok(summary_input);
        }

        self.summary_input_from_media(&resolve_media_input(input)?)
    }

    fn summary_input_from_media(
        &self,
        resolved_input: &ResolvedMediaInput,
    ) -> Result<SummaryInput> {
        let transcript = self
            .transcription
            .transcribe_resolved_input(resolved_input)?;
        Ok(SummaryInput {
            display_name: transcript.display_name,
            source_key: transcript.source_key,
            transcript_hash: transcript.transcript_hash,
            turns: transcript.turns,
        })
    }

    fn try_load_transcript_file(&self, path: &Path) -> Result<Option<SummaryInput>> {
        let transcript_text = match fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::InvalidData => return Ok(None),
            Err(error) => {
                return Err(error).with_context(|| format!("failed to read {}", path.display()));
            }
        };
        let Some(turns) = parse_transcript(&transcript_text) else {
            return Ok(None);
        };

        Ok(Some(SummaryInput {
            display_name: sanitize_name(&file_stem_name(path)?),
            source_key: local_file_source_key(path)?,
            transcript_hash: hash_string(&transcript_text),
            turns,
        }))
    }
}
