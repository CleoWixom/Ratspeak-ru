//! Plain-text projection of Markdown message bodies for native notifications.
//! A preview, not CommonMark: every pass is linear in the input.

const SPOILER: &str = "[Spoiler]";

pub(crate) fn markdown_to_plain_text(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut fence: Option<u8> = None;
    let mut in_block_spoiler = false;
    for raw in input.lines() {
        let line = raw.trim_start();
        let fence_marker = line
            .as_bytes()
            .first()
            .copied()
            .filter(|&b| (b == b'`' || b == b'~') && line.as_bytes().starts_with(&[b; 3]));
        if let Some(marker) = fence {
            if fence_marker == Some(marker) {
                fence = None;
            } else {
                push_line(&mut out, raw);
            }
            continue;
        }
        if let Some(marker) = fence_marker {
            fence = Some(marker);
            in_block_spoiler = false;
            continue;
        }
        if line.starts_with(">!") && !line.contains("!<") {
            if !in_block_spoiler {
                push_line(&mut out, SPOILER);
            }
            in_block_spoiler = true;
            continue;
        }
        in_block_spoiler = false;
        if is_thematic_break(line) {
            continue;
        }
        let mut text = line;
        while let Some(rest) = text.strip_prefix('>').filter(|rest| !rest.starts_with('!')) {
            text = rest.trim_start();
        }
        let hashes = text.bytes().take_while(|&b| b == b'#').count();
        if (1..=6).contains(&hashes)
            && text[hashes..]
                .chars()
                .next()
                .is_none_or(char::is_whitespace)
        {
            text = text[hashes..].trim();
        }
        let mut plain = String::with_capacity(text.len() + 4);
        if let Some(item) = ["- ", "* ", "+ "]
            .iter()
            .find_map(|marker| text.strip_prefix(marker))
        {
            plain.push_str("• ");
            text = item.trim_start();
        }
        push_inline(&mut plain, text);
        push_line(&mut out, &plain);
    }
    out
}

fn push_line(out: &mut String, line: &str) {
    if !out.is_empty() {
        out.push('\n');
    }
    out.push_str(line);
}

fn is_thematic_break(line: &str) -> bool {
    let mut marker = None;
    let mut count = 0;
    for b in line.bytes() {
        match b {
            b' ' | b'\t' => {}
            b'-' | b'*' | b'_' if marker.is_none_or(|m| m == b) => {
                marker = Some(b);
                count += 1;
            }
            _ => return false,
        }
    }
    count >= 3
}

fn positions(bytes: &[u8], pattern: &[u8; 2]) -> Vec<usize> {
    let mut found = Vec::new();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i..i + 2] == *pattern {
            found.push(i);
            i += 2;
        } else {
            i += 1;
        }
    }
    found
}

fn push_inline(out: &mut String, text: &str) {
    let bytes = text.as_bytes();
    let n = bytes.len();

    // Each code-span opener pairs with the next backtick run of equal length.
    let mut runs: Vec<(usize, usize)> = Vec::new();
    let mut i = 0;
    while i < n {
        if bytes[i] == b'\\' {
            i += 2;
        } else if bytes[i] == b'`' {
            let start = i;
            while i < n && bytes[i] == b'`' {
                i += 1;
            }
            runs.push((start, i - start));
        } else {
            i += 1;
        }
    }
    let mut code_close = vec![None; runs.len()];
    let mut later = std::collections::HashMap::new();
    for (index, &(_, len)) in runs.iter().enumerate().rev() {
        code_close[index] = later.insert(len, index);
    }
    let mut next_paren = vec![None; n + 1];
    for j in (0..n).rev() {
        next_paren[j] = if bytes[j] == b')' {
            Some(j)
        } else {
            next_paren[j + 1]
        };
    }
    let pipes = positions(bytes, b"||");
    let spoiler_closes = positions(bytes, b"!<");
    let (mut run, mut pipe, mut close) = (0, 0, 0);

    let mut buf: Vec<u8> = Vec::with_capacity(n);
    let mut deleted: Vec<usize> = Vec::new();
    let mut brackets: Vec<usize> = Vec::new();
    let flank = |at: Option<&u8>, alnum: bool| {
        at.is_some_and(|b| {
            if alnum {
                b.is_ascii_alphanumeric()
            } else {
                !b.is_ascii_whitespace()
            }
        })
    };
    i = 0;
    while i < n {
        let b = bytes[i];
        match b {
            b'\\' if bytes.get(i + 1).is_some_and(u8::is_ascii_punctuation) => {
                buf.push(bytes[i + 1]);
                i += 2;
            }
            b'`' => {
                while run < runs.len() && runs[run].0 < i {
                    run += 1;
                }
                match runs.get(run).filter(|&&(start, _)| start == i) {
                    Some(&(_, len)) => match code_close[run] {
                        Some(end) => {
                            let end_at = runs[end].0;
                            buf.extend_from_slice(text[i + len..end_at].trim().as_bytes());
                            i = end_at + len;
                        }
                        None => i += len,
                    },
                    None => i += 1,
                }
            }
            b'|' if bytes.get(i + 1) == Some(&b'|') => {
                while pipe < pipes.len() && pipes[pipe] < i {
                    pipe += 1;
                }
                match pipes.get(pipe + 1) {
                    Some(&end) if pipes.get(pipe) == Some(&i) => {
                        buf.extend_from_slice(SPOILER.as_bytes());
                        i = end + 2;
                    }
                    _ => i += 2,
                }
            }
            b'>' if bytes.get(i + 1) == Some(&b'!') => {
                while close < spoiler_closes.len() && spoiler_closes[close] < i + 2 {
                    close += 1;
                }
                match spoiler_closes.get(close) {
                    Some(&end) => {
                        buf.extend_from_slice(SPOILER.as_bytes());
                        i = end + 2;
                    }
                    None => {
                        buf.extend_from_slice(b">!");
                        i += 2;
                    }
                }
            }
            b'~' if bytes.get(i + 1) == Some(&b'~') => i += 2,
            b'*' | b'_' => {
                let start = i;
                while i < n && bytes[i] == b {
                    i += 1;
                }
                let before = start.checked_sub(1).and_then(|j| bytes.get(j));
                let after = bytes.get(i);
                let literal = if b == b'_' {
                    flank(before, true) && flank(after, true)
                } else {
                    !flank(before, false) && !flank(after, false)
                };
                if literal {
                    buf.extend_from_slice(&bytes[start..i]);
                }
            }
            b'[' => {
                if i > 0 && bytes[i - 1] == b'!' && buf.last() == Some(&b'!') {
                    brackets.push(buf.len() - 1);
                }
                brackets.push(buf.len());
                buf.push(b'[');
                i += 1;
            }
            b']' if bytes.get(i + 1) == Some(&b'(') && !brackets.is_empty() => {
                match next_paren[i + 2] {
                    Some(end) => {
                        let open = brackets.pop().unwrap_or_default();
                        deleted.push(open);
                        if let Some(&bang) = brackets.last()
                            && bang + 1 == open
                            && buf[bang] == b'!'
                        {
                            deleted.push(brackets.pop().unwrap_or_default());
                        }
                        i = end + 1;
                    }
                    None => {
                        buf.push(b']');
                        i += 1;
                    }
                }
            }
            _ => {
                buf.push(b);
                i += 1;
            }
        }
    }
    deleted.sort_unstable();
    let mut skip = deleted.into_iter().peekable();
    let kept: Vec<u8> = buf
        .into_iter()
        .enumerate()
        .filter(|(index, _)| {
            if skip.peek() == Some(index) {
                skip.next();
                false
            } else {
                true
            }
        })
        .map(|(_, b)| b)
        .collect();
    out.push_str(&String::from_utf8_lossy(&kept));
}

#[cfg(test)]
mod tests {
    use super::markdown_to_plain_text as plain;

    #[test]
    fn strips_emphasis_code_and_strike_delimiters() {
        assert_eq!(plain("**bold** and __strong__"), "bold and strong");
        assert_eq!(plain("*em* _em_ ~~gone~~"), "em em gone");
        assert_eq!(plain("run `cargo *test*` now"), "run cargo *test* now");
        assert_eq!(plain("``a ` b``"), "a ` b");
        assert_eq!(plain("snake_case_name stays"), "snake_case_name stays");
        assert_eq!(plain("2 * 3 = 6"), "2 * 3 = 6");
        assert_eq!(plain("unclosed `tick"), "unclosed tick");
    }

    #[test]
    fn strips_block_markers() {
        assert_eq!(plain("# Title\n## Sub ##"), "Title\nSub ##");
        assert_eq!(plain("#hashtag"), "#hashtag");
        assert_eq!(plain("> quoted\n>> deeper"), "quoted\ndeeper");
        assert_eq!(
            plain("- one\n* two\n+ three\n1. four"),
            "• one\n• two\n• three\n1. four"
        );
        assert_eq!(plain("above\n---\nbelow"), "above\nbelow");
        assert_eq!(
            plain("```rust\nlet *x* = 1;\n```\nafter"),
            "let *x* = 1;\nafter"
        );
        assert_eq!(plain("~~~\n# raw\n~~~"), "# raw");
    }

    #[test]
    fn links_images_and_escapes() {
        assert_eq!(plain("see [docs](https://x.org/a_b) now"), "see docs now");
        assert_eq!(plain("![logo](a.png)"), "logo");
        assert_eq!(
            plain("[not a link] and [x](unclosed"),
            "[not a link] and [x](unclosed"
        );
        assert_eq!(plain(r"\*literal\* \[x\](y) \\"), r"*literal* [x](y) \");
        assert_eq!(plain("- [ ] task"), "• [ ] task");
    }

    #[test]
    fn spoilers_are_hidden() {
        assert_eq!(plain("the end: ||he lives||!"), "the end: [Spoiler]!");
        assert_eq!(plain("a >!secret!< b"), "a [Spoiler] b");
        assert_eq!(plain(">! line one\n>! line two\nafter"), "[Spoiler]\nafter");
        assert_eq!(plain("|| open"), " open");
    }

    #[test]
    fn keeps_unicode_and_is_linear_on_adversarial_input() {
        assert_eq!(plain("**héllo** 🐀"), "héllo 🐀");
        let hostile = "[](".repeat(20_000) + &"`".repeat(5_000) + &"||".repeat(5_000);
        let started = std::time::Instant::now();
        let _ = plain(&hostile);
        let _ = plain(&"*a_".repeat(50_000));
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
    }
}
