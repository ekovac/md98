//! UTF-8 <-> Windows-1252 (CP1252) conversion for Win32 ANSI (`*A`) APIs.

/// Convert UTF-8 text to Windows-1252 (CP1252) bytes.
pub fn utf8_to_cp1252(input: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len());
    utf8_to_cp1252_into(input, &mut out);
    out
}

/// Append UTF-8 text converted to CP1252 into an existing buffer (for arena reuse).
pub fn utf8_to_cp1252_into(input: &str, out: &mut Vec<u8>) {
    for ch in input.chars() {
        let code = ch as u32;
        if code <= 0x7F || (0xA0..=0xFF).contains(&code) {
            out.push(code as u8);
        } else {
            let mapped = match ch {
                '\u{20AC}' => 0x80, // €
                '\u{201A}' => 0x82, // ‚
                '\u{0192}' => 0x83, // ƒ
                '\u{201E}' => 0x84, // „
                '\u{2026}' => 0x85, // …
                '\u{2020}' => 0x86, // †
                '\u{2021}' => 0x87, // ‡
                '\u{02C6}' => 0x88, // ˆ
                '\u{2030}' => 0x89, // ‰
                '\u{0160}' => 0x8A, // Š
                '\u{2039}' => 0x8B, // ‹
                '\u{0152}' => 0x8C, // Œ
                '\u{017D}' => 0x8E, // Ž
                '\u{2018}' => 0x91, // ‘
                '\u{2019}' => 0x92, // ’
                '\u{201C}' => 0x93, // “
                '\u{201D}' => 0x94, // ”
                '\u{2022}' => 0x95, // •
                '\u{2013}' => 0x96, // –
                '\u{2014}' => 0x97, // —
                '\u{02DC}' => 0x98, // ˜
                '\u{2122}' => 0x99, // ™
                '\u{0161}' => 0x9A, // š
                '\u{203A}' => 0x9B, // ›
                '\u{0153}' => 0x9C, // œ
                '\u{017E}' => 0x9E, // ž
                '\u{0178}' => 0x9F, // Ÿ
                _ => b'?',
            };
            out.push(mapped);
        }
    }
}

/// Convert Windows-1252 (CP1252) bytes back to UTF-8.
pub fn cp1252_to_utf8(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len());
    for &b in bytes {
        let ch = match b {
            0x80 => '\u{20AC}',
            0x82 => '\u{201A}',
            0x83 => '\u{0192}',
            0x84 => '\u{201E}',
            0x85 => '\u{2026}',
            0x86 => '\u{2020}',
            0x87 => '\u{2021}',
            0x88 => '\u{02C6}',
            0x89 => '\u{2030}',
            0x8A => '\u{0160}',
            0x8B => '\u{2039}',
            0x8C => '\u{0152}',
            0x8E => '\u{017D}',
            0x91 => '\u{2018}',
            0x92 => '\u{2019}',
            0x93 => '\u{201C}',
            0x94 => '\u{201D}',
            0x95 => '\u{2022}',
            0x96 => '\u{2013}',
            0x97 => '\u{2014}',
            0x98 => '\u{02DC}',
            0x99 => '\u{2122}',
            0x9A => '\u{0161}',
            0x9B => '\u{203A}',
            0x9C => '\u{0153}',
            0x9E => '\u{017E}',
            0x9F => '\u{0178}',
            _ => b as char,
        };
        out.push(ch);
    }
    out
}

/// Normalize LF -> CRLF for Win32 RichEdit display.
pub fn normalize_to_crlf(input: &str) -> String {
    let mut out = String::with_capacity(input.len() + input.len() / 16);
    let mut prev_cr = false;
    for ch in input.chars() {
        if ch == '\n' && !prev_cr {
            out.push('\r');
        }
        out.push(ch);
        prev_cr = ch == '\r';
    }
    out
}

/// Normalize CRLF / CR -> LF for clean Markdown parsing and UTF-8 saving.
pub fn normalize_to_lf(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\r' {
            if chars.peek() == Some(&'\n') {
                chars.next();
            }
            out.push('\n');
        } else {
            out.push(ch);
        }
    }
    out
}
