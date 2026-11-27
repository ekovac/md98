//! Pure-Rust Find and Replace helpers operating cleanly on CP1252-LF indices
//! (1:1 aligned with Windows 98 `RichEdit 2.0` character offsets).

use crate::cp1252::{cp1252_to_utf8, utf8_to_cp1252};

#[inline]
fn cp1252_lower(b: u8) -> u8 {
    match b {
        b'A'..=b'Z' => b + 32,
        0xC0..=0xD6 | 0xD8..=0xDE => b + 32, // Latin-1 uppercase accented
        0x8A => 0x9A,                        // Š -> š
        0x8C => 0x9C,                        // Œ -> œ
        0x8E => 0x9E,                        // Ž -> ž
        0x9F => 0xFF,                        // Ÿ -> ÿ
        _ => b,
    }
}

#[inline]
fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b >= 0xC0
}

fn matches_at(haystack: &[u8], needle: &[u8], pos: usize, match_case: bool, whole_word: bool) -> bool {
    if pos + needle.len() > haystack.len() {
        return false;
    }
    for i in 0..needle.len() {
        let a = haystack[pos + i];
        let b = needle[i];
        let eq = if match_case {
            a == b
        } else {
            cp1252_lower(a) == cp1252_lower(b)
        };
        if !eq {
            return false;
        }
    }
    if whole_word {
        if pos > 0 && is_word_byte(haystack[pos - 1]) {
            return false;
        }
        let end = pos + needle.len();
        if end < haystack.len() && is_word_byte(haystack[end]) {
            return false;
        }
    }
    true
}

/// Find the next occurrence of `needle` in `haystack` (both LF-normalized CP1252).
///
/// - When `down == true`, searches forward from `start_pos` (wrapping around to 0 if not found).
/// - When `down == false`, searches backward before `start_pos` (wrapping around to end if not found).
/// Returns `Some((start_idx, end_idx))` in CP1252-LF offsets (exact match for `RichEdit 2.0` `CHARRANGE`).
pub fn find_cp1252(
    haystack: &[u8],
    needle: &[u8],
    start_pos: usize,
    down: bool,
    match_case: bool,
    whole_word: bool,
) -> Option<(usize, usize)> {
    if needle.is_empty() || needle.len() > haystack.len() {
        return None;
    }
    let last_valid = haystack.len() - needle.len();

    if down {
        let start = start_pos.min(haystack.len());
        if start <= last_valid {
            for pos in start..=last_valid {
                if matches_at(haystack, needle, pos, match_case, whole_word) {
                    return Some((pos, pos + needle.len()));
                }
            }
        }
        // Wrap around from beginning
        let wrap_end = start.min(last_valid + 1);
        for pos in 0..wrap_end {
            if matches_at(haystack, needle, pos, match_case, whole_word) {
                return Some((pos, pos + needle.len()));
            }
        }
        None
    } else {
        // Search strictly before `start_pos`
        if start_pos > 0 {
            let mut pos = (start_pos - 1).min(last_valid);
            loop {
                if matches_at(haystack, needle, pos, match_case, whole_word) {
                    return Some((pos, pos + needle.len()));
                }
                if pos == 0 {
                    break;
                }
                pos -= 1;
            }
        }
        // Wrap around from end
        let mut pos = last_valid;
        while pos >= start_pos {
            if matches_at(haystack, needle, pos, match_case, whole_word) {
                return Some((pos, pos + needle.len()));
            }
            if pos == 0 {
                break;
            }
            pos -= 1;
        }
        None
    }
}

/// Replace all occurrences of `needle` with `replacement` in UTF-8 LF text,
/// matching with CP1252 case/whole-word semantics. Returns `(new_utf8, count)`.
pub fn replace_all_text(
    text_utf8: &str,
    needle_utf8: &str,
    replacement_utf8: &str,
    match_case: bool,
    whole_word: bool,
) -> (String, usize) {
    let hay = utf8_to_cp1252(text_utf8);
    let ndl = utf8_to_cp1252(needle_utf8);
    let rep = utf8_to_cp1252(replacement_utf8);
    if ndl.is_empty() || ndl.len() > hay.len() {
        return (text_utf8.to_string(), 0);
    }

    let mut out = Vec::with_capacity(hay.len());
    let mut i = 0usize;
    let mut count = 0usize;
    while i < hay.len() {
        if i + ndl.len() <= hay.len() && matches_at(&hay, &ndl, i, match_case, whole_word) {
            out.extend_from_slice(&rep);
            i += ndl.len();
            count += 1;
        } else {
            out.push(hay[i]);
            i += 1;
        }
    }
    if count == 0 {
        (text_utf8.to_string(), 0)
    } else {
        (cp1252_to_utf8(&out), count)
    }
}
