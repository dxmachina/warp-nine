use super::*;

#[test]
fn test_merge_per_cell_styles_merges_runs() {
    let per_cell = b"\x1b[31mr\x1b[0m\x1b[31me\x1b[0m\x1b[31md\x1b[0m plain \
                     \x1b[44;1mB\x1b[0m\x1b[44;1mB\x1b[0m\r\n";
    assert_eq!(
        merge_per_cell_styles(per_cell).as_deref(),
        Some(&b"\x1b[31mred\x1b[0m plain \x1b[44;1mBB\x1b[0m\r\n"[..])
    );
}

#[test]
fn test_merge_per_cell_styles_keeps_resets_between_different_styles() {
    // A style that only extends another (31 vs 31;1) is still a different style.
    let per_cell = b"\x1b[31ma\x1b[0m\x1b[31;1mb\x1b[0m\x1b[31mc\x1b[0m";
    assert_eq!(merge_per_cell_styles(per_cell), None);
}

#[test]
fn test_merge_per_cell_styles_keeps_resets_at_row_ends() {
    let per_cell = b"\x1b[32ma\x1b[0m\x1b[32mb\x1b[0m\r\n\x1b[32mc\x1b[0m\x1b[32md\x1b[0m\r\n";
    assert_eq!(
        merge_per_cell_styles(per_cell).as_deref(),
        Some(&b"\x1b[32mab\x1b[0m\r\n\x1b[32mcd\x1b[0m\r\n"[..])
    );
}

#[test]
fn test_merge_per_cell_styles_leaves_per_run_output_alone() {
    assert_eq!(merge_per_cell_styles(b"plain text\r\n"), None);
    assert_eq!(
        merge_per_cell_styles(b"\x1b[31mred\x1b[0m \x1b[32mgreen\x1b[0m\r\n"),
        None
    );
    assert_eq!(merge_per_cell_styles(b""), None);
}

#[test]
fn test_merge_per_cell_styles_handles_truncated_sequences() {
    // An escape that never terminates is copied through untouched.
    let output = b"\x1b[31ma\x1b[0m\x1b[31mb\x1b[0m\x1b[3";
    assert_eq!(
        merge_per_cell_styles(output).as_deref(),
        Some(&b"\x1b[31mab\x1b[0m\x1b[3"[..])
    );
}

#[test]
fn test_last_rows() {
    let output = b"row one\r\nrow two\r\nrow three\r\n";
    assert_eq!(last_rows(output, 100), &output[..]);
    // Cutting inside "row two" drops the rest of it and starts at the next row.
    assert_eq!(last_rows(output, 15), b"row three\r\n");
    assert_eq!(last_rows(output, 3), b"");
}
