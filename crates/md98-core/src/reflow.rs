//! Markdown-aware text reflow (on-demand) and single-line live auto-wrap.
//!
//! Preserves fenced code blocks, indented code blocks, GFM tables, headings,
//! horizontal rules, and blank lines verbatim while cleanly wrapping paragraphs,
//! blockquotes, bullet/numbered lists, and GFM task lists to 72/80/100 columns.

/// Supported wrap widths exposed in the UI (72, 80, or 100 columns).
pub const WRAP_WIDTHS: [usize; 3] = [72, 80, 100];

/// Reflow an entire Markdown document or selection to `max_width` columns (LF line endings).
pub fn reflow_markdown(input: &str, max_width: usize) -> String {
    let width = max_width.max(20);
    let has_trailing_nl = input.ends_with('\n');
    let lines: Vec<&str> = input.lines().collect();

    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    let mut in_fence: Option<(char, usize)> = None;
    let mut i = 0usize;

    while i < lines.len() {
        let line = lines[i];

        // 1. Inside or starting a fenced code block (``` or ~~~)
        if let Some((fch, flen)) = in_fence {
            out.push(line.to_string());
            if is_closing_fence(line, fch, flen) {
                in_fence = None;
            }
            i += 1;
            continue;
        }
        if let Some(fence) = parse_opening_fence(line) {
            in_fence = Some(fence);
            out.push(line.to_string());
            i += 1;
            continue;
        }

        // 2. Blank line
        if line.trim().is_empty() {
            out.push(String::new());
            i += 1;
            continue;
        }

        // 3. Verbatim structural lines: indented code, headings, HR, GFM tables
        if is_indented_code(line) || is_heading(line) || is_horizontal_rule(line) || is_table_line(line) {
            out.push(line.to_string());
            i += 1;
            continue;
        }

        // 4. Blockquote block (`> ...`, `> > ...`)
        if let Some((q_prefix, first_rest)) = split_blockquote_prefix(line) {
            let mut body_lines: Vec<&str> = vec![first_rest];
            i += 1;
            while i < lines.len() {
                let next = lines[i];
                if let Some((nq, nrest)) = split_blockquote_prefix(next) {
                    if nq == q_prefix
                        && !nrest.trim().is_empty()
                        && !is_heading(nrest)
                        && !is_horizontal_rule(nrest)
                        && !is_table_line(nrest)
                        && parse_opening_fence(nrest).is_none()
                        && parse_list_prefix(nrest).is_none()
                    {
                        body_lines.push(nrest);
                        i += 1;
                        continue;
                    }
                }
                break;
            }
            wrap_prefixed_paragraph(&body_lines, q_prefix, q_prefix, width, &mut out);
            continue;
        }

        // 5. List / task-list item (`- `, `* `, `+ `, `1. `, `- [x] `, nested indent)
        if let Some((first_prefix, cont_prefix, first_rest)) = parse_list_prefix(line) {
            let mut body_lines: Vec<&str> = vec![first_rest];
            i += 1;
            while i < lines.len() {
                let next = lines[i];
                if next.trim().is_empty()
                    || parse_opening_fence(next).is_some()
                    || is_heading(next)
                    || is_horizontal_rule(next)
                    || is_table_line(next)
                    || split_blockquote_prefix(next).is_some()
                    || parse_list_prefix(next).is_some()
                {
                    break;
                }
                // Continuation line belonging to this list item
                body_lines.push(next.trim_start());
                i += 1;
            }
            wrap_prefixed_paragraph(&body_lines, &first_prefix, &cont_prefix, width, &mut out);
            continue;
        }

        // 6. Regular paragraph: collect consecutive plain lines
        let mut body_lines: Vec<&str> = vec![line];
        i += 1;
        while i < lines.len() {
            let next = lines[i];
            if next.trim().is_empty()
                || parse_opening_fence(next).is_some()
                || is_indented_code(next)
                || is_heading(next)
                || is_horizontal_rule(next)
                || is_table_line(next)
                || split_blockquote_prefix(next).is_some()
                || parse_list_prefix(next).is_some()
            {
                break;
            }
            body_lines.push(next);
            i += 1;
        }
        wrap_prefixed_paragraph(&body_lines, "", "", width, &mut out);
    }

    let mut result = out.join("\n");
    if has_trailing_nl && !result.ends_with('\n') {
        result.push('\n');
    }
    result
}

/// Live auto-wrap helper for a single edited line in the editor.
///
/// Returns `Some((break_char_offset, insert_str))` if the line exceeds `max_width`
/// and has a safe whitespace break point (not inside a fenced code block, heading,
/// or table). Replacing `[break_char_offset .. break_char_offset + 1]` (the space)
/// with `insert_str` (e.g. `"\r\n"` or `"\r\n  "` or `"\r\n> "`) wraps the line
/// seamlessly while keeping the cursor at the end of the wrapped word.
pub fn compute_live_wrap_break(
    line: &str,
    max_width: usize,
    in_fenced_code: bool,
) -> Option<(usize, String)> {
    let width = max_width.max(20);
    if in_fenced_code {
        return None;
    }
    let char_count = line.chars().count();
    if char_count <= width {
        return None;
    }
    if is_indented_code(line)
        || is_heading(line)
        || is_horizontal_rule(line)
        || is_table_line(line)
        || parse_opening_fence(line).is_some()
    {
        return None;
    }

    // Determine continuation prefix (blockquote or list hanging indent)
    let (prefix_chars, cont_prefix) = if let Some((q, _)) = split_blockquote_prefix(line) {
        (q.chars().count(), q.to_string())
    } else if let Some((fp, cp, _)) = parse_list_prefix(line) {
        (fp.chars().count(), cp)
    } else {
        (0usize, String::new())
    };

    // Find the last space at or before `width` (strictly after any structural prefix)
    let mut last_space_idx: Option<usize> = None;
    for (ci, ch) in line.chars().enumerate() {
        if ci > width {
            break;
        }
        if ch == ' ' && ci > prefix_chars {
            last_space_idx = Some(ci);
        }
    }

    let break_idx = last_space_idx?;
    // Ensure there is non-space text after the break so we don't break trailing spaces prematurely
    let after: String = line.chars().skip(break_idx + 1).collect();
    if after.trim().is_empty() {
        return None;
    }

    let mut insert = String::with_capacity(2 + cont_prefix.len());
    insert.push_str("\r\n");
    insert.push_str(&cont_prefix);
    Some((break_idx, insert))
}

/// Scan lines `[0 .. target_line)` to determine whether `target_line` is inside a fenced code block.
pub fn is_line_in_fenced_code(text_lf: &str, target_line: usize) -> bool {
    let mut in_fence: Option<(char, usize)> = None;
    for (idx, line) in text_lf.lines().enumerate() {
        if idx >= target_line {
            break;
        }
        if let Some((fch, flen)) = in_fence {
            if is_closing_fence(line, fch, flen) {
                in_fence = None;
            }
        } else if let Some(fence) = parse_opening_fence(line) {
            in_fence = Some(fence);
        }
    }
    in_fence.is_some()
}

fn wrap_prefixed_paragraph(
    lines: &[&str],
    first_prefix: &str,
    cont_prefix: &str,
    width: usize,
    out: &mut Vec<String>,
) {
    // Split on explicit Markdown two-space hard line breaks (`"  "`)
    let mut segments: Vec<(String, bool)> = Vec::new();
    let mut cur = String::new();
    for (idx, &raw) in lines.iter().enumerate() {
        let hard = raw.ends_with("  ");
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            if !cur.is_empty() {
                cur.push(' ');
            }
            cur.push_str(trimmed);
        }
        if hard && idx + 1 < lines.len() {
            segments.push((core::mem::take(&mut cur), true));
        }
    }
    if !cur.is_empty() || segments.is_empty() {
        let trailing_hard = lines.last().is_some_and(|l| l.ends_with("  "));
        segments.push((cur, trailing_hard));
    }

    let mut is_first_line = true;
    for (seg, hard_break) in segments {
        let words: Vec<&str> = seg.split_whitespace().collect();
        if words.is_empty() {
            let pfx = if is_first_line { first_prefix } else { cont_prefix };
            out.push(pfx.trim_end().to_string());
            is_first_line = false;
            continue;
        }

        let mut line_buf = String::new();
        let mut line_chars = 0usize;

        let start_line = |buf: &mut String, chars: &mut usize, first: &mut bool| {
            let pfx = if *first { first_prefix } else { cont_prefix };
            buf.clear();
            buf.push_str(pfx);
            *chars = pfx.chars().count();
            *first = false;
        };

        start_line(&mut line_buf, &mut line_chars, &mut is_first_line);
        let mut has_word_on_line = false;

        for word in words {
            let wlen = word.chars().count();
            if has_word_on_line && line_chars + 1 + wlen > width {
                out.push(core::mem::take(&mut line_buf));
                start_line(&mut line_buf, &mut line_chars, &mut is_first_line);
                has_word_on_line = false;
            }
            if has_word_on_line {
                line_buf.push(' ');
                line_chars += 1;
            }
            line_buf.push_str(word);
            line_chars += wlen;
            has_word_on_line = true;
        }
        if hard_break {
            line_buf.push_str("  ");
        }
        out.push(line_buf);
    }
}

fn parse_opening_fence(line: &str) -> Option<(char, usize)> {
    let trimmed = line.trim_start();
    let indent = line.len() - trimmed.len();
    if indent >= 4 {
        return None;
    }
    let first = trimmed.chars().next()?;
    if first != '`' && first != '~' {
        return None;
    }
    let count = trimmed.chars().take_while(|&c| c == first).count();
    if count >= 3 {
        Some((first, count))
    } else {
        None
    }
}

fn is_closing_fence(line: &str, ch: char, min_len: usize) -> bool {
    let trimmed = line.trim();
    trimmed.len() >= min_len && trimmed.chars().all(|c| c == ch)
}

fn is_indented_code(line: &str) -> bool {
    line.starts_with("    ") || line.starts_with('\t')
}

fn is_heading(line: &str) -> bool {
    let t = line.trim_start();
    let hashes = t.chars().take_while(|&c| c == '#').count();
    (1..=6).contains(&hashes) && t[hashes..].starts_with(' ')
}

fn is_horizontal_rule(line: &str) -> bool {
    let t = line.trim();
    if t.len() < 3 {
        return false;
    }
    let mut marker: Option<char> = None;
    let mut count = 0usize;
    for ch in t.chars() {
        if ch == ' ' || ch == '\t' {
            continue;
        }
        if ch != '-' && ch != '*' && ch != '_' {
            return false;
        }
        match marker {
            None => {
                marker = Some(ch);
                count = 1;
            }
            Some(m) if m == ch => count += 1,
            _ => return false,
        }
    }
    count >= 3
}

fn is_table_line(line: &str) -> bool {
    let t = line.trim();
    t.starts_with('|') && t.len() >= 2
}

fn split_blockquote_prefix(line: &str) -> Option<(&str, &str)> {
    let bytes = line.as_bytes();
    let mut pos = 0usize;
    let mut found = false;
    while pos < bytes.len() {
        let mut p = pos;
        let mut spaces = 0;
        while p < bytes.len() && bytes[p] == b' ' && spaces < 3 {
            p += 1;
            spaces += 1;
        }
        if p < bytes.len() && bytes[p] == b'>' {
            found = true;
            pos = p + 1;
            if pos < bytes.len() && bytes[pos] == b' ' {
                pos += 1;
            }
        } else {
            break;
        }
    }
    if found {
        Some((&line[..pos], &line[pos..]))
    } else {
        None
    }
}

fn parse_list_prefix(line: &str) -> Option<(String, String, &str)> {
    let bytes = line.as_bytes();
    let mut indent = 0usize;
    while indent < bytes.len() && bytes[indent] == b' ' {
        indent += 1;
    }
    let rest = &line[indent..];
    if rest.is_empty() {
        return None;
    }

    // Unordered list (`- `, `* `, `+ `) + optional GFM task marker (`[ ] ` / `[x] `)
    let first = rest.as_bytes()[0];
    if (first == b'-' || first == b'*' || first == b'+') && rest.len() >= 2 && rest.as_bytes()[1] == b' ' {
        let mut marker_len = 2usize;
        let after_bullet = &rest[2..];
        if after_bullet.len() >= 4
            && after_bullet.as_bytes()[0] == b'['
            && matches!(after_bullet.as_bytes()[1], b' ' | b'x' | b'X')
            && after_bullet.as_bytes()[2] == b']'
            && after_bullet.as_bytes()[3] == b' '
        {
            marker_len += 4;
        }
        let prefix = line[..indent + marker_len].to_string();
        let cont = " ".repeat(indent + 2);
        return Some((prefix, cont, &line[indent + marker_len..]));
    }

    // Ordered list (`1. `, `12) `)
    let digits = rest.bytes().take_while(|b| b.is_ascii_digit()).count();
    if digits > 0 && digits <= 6 && rest.len() > digits + 1 {
        let punct = rest.as_bytes()[digits];
        if (punct == b'.' || punct == b')') && rest.as_bytes()[digits + 1] == b' ' {
            let marker_len = digits + 2;
            let prefix = line[..indent + marker_len].to_string();
            let cont = " ".repeat(indent + marker_len);
            return Some((prefix, cont, &line[indent + marker_len..]));
        }
    }

    None
}
