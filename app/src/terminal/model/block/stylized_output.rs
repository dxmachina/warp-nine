//! Helpers for a block's persisted stylized output: its rows as text with SGR escape
//! sequences, separated by `\r\n`.

const SGR_RESET: &[u8] = b"\x1b[0m";

/// Rewrites stylized output saved in the old per-cell form, where every styled cell carried
/// its own escape sequence and reset (`ESC[31ma ESC[0m ESC[31mb ESC[0m`), into the per-run
/// form block output is written in now (`ESC[31mab ESC[0m`). Both parse to the same cells.
///
/// This is a byte-level pass, much cheaper than parsing: it drops each reset that is
/// immediately followed by the escape sequence that was already in effect. Returns `None`
/// when there is nothing to merge, which is the case for output written in the per-run form.
pub fn merge_per_cell_styles(output: &[u8]) -> Option<Vec<u8>> {
    let mut merged: Option<Vec<u8>> = None;
    // The SGR sequence in effect, if any, as written (e.g. `ESC[31m`).
    let mut active: Option<&[u8]> = None;
    // Everything before `copied_to` is already in `merged` (once that exists).
    let mut copied_to = 0;
    let mut i = 0;

    while let Some(offset) = memchr::memchr(0x1b, &output[i..]) {
        i += offset;
        let Some(len) = csi_len(&output[i..]) else {
            i += 1;
            continue;
        };
        let sequence = &output[i..i + len];

        if sequence == SGR_RESET {
            if let Some(active_sequence) = active
                && output[i + len..].starts_with(active_sequence)
            {
                // The next cell continues the run: drop the reset and the repeated sequence.
                let merged = merged.get_or_insert_with(|| Vec::with_capacity(output.len() / 4));
                merged.extend_from_slice(&output[copied_to..i]);
                i += len + active_sequence.len();
                copied_to = i;
                continue;
            }
            active = None;
        } else if sequence.last() == Some(&b'm') {
            active = Some(sequence);
        }
        i += len;
    }

    let mut merged = merged?;
    merged.extend_from_slice(&output[copied_to..]);
    Some(merged)
}

/// Returns the length of the CSI sequence at the start of `bytes`, if it starts with one.
fn csi_len(bytes: &[u8]) -> Option<usize> {
    if !bytes.starts_with(b"\x1b[") {
        return None;
    }
    bytes[2..]
        .iter()
        .position(|byte| (0x40..=0x7e).contains(byte))
        .map(|final_byte| final_byte + 3)
}

/// Returns at most the last `max_bytes` of `output`, starting at a row boundary so no row
/// is cut in half. Every row ends with its styles reset, so the result parses on its own.
pub fn last_rows(output: &[u8], max_bytes: usize) -> &[u8] {
    if output.len() <= max_bytes {
        return output;
    }
    let cut = output.len() - max_bytes;
    match memchr::memchr(b'\n', &output[cut..]) {
        Some(newline) => &output[cut + newline + 1..],
        None => &[],
    }
}

#[cfg(test)]
#[path = "stylized_output_tests.rs"]
mod tests;
