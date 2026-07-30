//! Secret detection in terminal text.
//!
//! LOCAL FORK: extracted from `ai/blocklist/block/secret_redaction.rs`. These
//! functions have nothing to do with the agent — they scan arbitrary text
//! against the enterprise/user secret regexes and are used by workflows, the
//! notebooks view and the MCP settings editor. They lived in the agent tree
//! only because the agent's block renderer was their first caller, which would
//! have made them collateral damage when `app/src/ai` is deleted.
//!
//! The redaction *rendering* half stayed behind: it is written against
//! `AIAgentOutput` / `AIBlockAction` and goes with the agent UI.

use std::sync::Arc;

use warp_errors::report_error;
use warpui::elements::SecretRange;

use crate::terminal::model::secrets::{SECRETS_REGEX, SecretLevel, SecretsRegex};

pub const SECRET_REDACTION_REPLACEMENT_CHARACTER: &str = "*";

/// Returns the ranges of detected secrets in the given text.
pub(crate) fn find_secrets_in_text(text: &str) -> Vec<SecretRange> {
    find_secrets_in_text_with_levels(text)
        .into_iter()
        .map(|(range, _level)| range)
        .collect()
}

/// Returns the ranges of detected secrets in the given text along with their SecretLevel.
pub(crate) fn find_secrets_in_text_with_levels(text: &str) -> Vec<(SecretRange, SecretLevel)> {
    let secrets_regex: Arc<SecretsRegex> = { SECRETS_REGEX.lock().clone() };

    find_secrets_in_text_with_levels_using_regex(text, &secrets_regex)
}

pub(crate) fn find_secrets_in_text_with_levels_using_regex(
    text: &str,
    secrets_regex: &SecretsRegex,
) -> Vec<(SecretRange, SecretLevel)> {
    let SecretsRegex {
        regex,
        level_metadata,
        ..
    } = secrets_regex;

    let mut secret_ranges = vec![];
    let mut byte_to_char_index = vec![0; text.len() + 1]; // Map byte index to char index

    // Track the current character index while iterating through the string.
    let mut char_index = 0;
    for (byte_index, _) in text.char_indices() {
        byte_to_char_index[byte_index] = char_index;
        char_index += 1;
    }
    byte_to_char_index[text.len()] = char_index; // Map the last byte to the last character index

    // Iterate over the text once, finding all matches against secret regex. Map the byte ranges
    // to character ranges and store them.
    for mat in regex.find_iter(text) {
        let start_byte = mat.start();
        let end_byte = mat.end();
        let start_char = byte_to_char_index[start_byte];
        let end_char = byte_to_char_index[end_byte];

        // Determine which pattern matched by getting the pattern ID and map via counts
        let pattern_id = mat.pattern().as_usize();
        let total_patterns = level_metadata.enterprise_count + level_metadata.user_count;
        if pattern_id >= total_patterns {
            report_error!(
                "Secret level not found for pattern ID",
                extra: { "pattern_id" => %pattern_id }
            );
            continue;
        }
        let secret_level = if pattern_id < level_metadata.enterprise_count {
            SecretLevel::Enterprise
        } else {
            SecretLevel::User
        };

        secret_ranges.push((
            SecretRange {
                char_range: start_char..end_char,
                byte_range: start_byte..end_byte,
            },
            secret_level,
        ));
    }

    // Merge overlapping ranges, preserving the highest priority SecretLevel
    merge_sorted_ranges_with_levels(secret_ranges)
}

/// Merges overlapping ranges while preserving the highest priority SecretLevel
fn merge_sorted_ranges_with_levels(
    ranges: Vec<(SecretRange, SecretLevel)>,
) -> Vec<(SecretRange, SecretLevel)> {
    if ranges.is_empty() {
        return ranges;
    }

    let mut merged_ranges = vec![];
    let mut current_range = ranges[0].0.clone();
    let mut current_level = ranges[0].1;

    for (range, level) in ranges.into_iter().skip(1) {
        // We can merge based on character ranges since non-overlapping character ranges result in non-overlapping byte ranges.
        if range.char_range.start <= current_range.char_range.end {
            // Extend the current range to include the overlapping range.
            current_range.extend_range_end(&range);
            // Keep the highest priority level
            if level.priority() > current_level.priority() {
                current_level = level;
            }
        } else {
            // No overlap, push the current range and move to the next.
            merged_ranges.push((current_range, current_level));
            current_range = range;
            current_level = level;
        }
    }

    // Add the last range.
    merged_ranges.push((current_range, current_level));

    merged_ranges
}
