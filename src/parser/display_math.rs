//! Multi-line `$$ … $$` display math as a block, the way GitHub reads it.
//!
//! CommonMark sees such a block as a paragraph, so a continuation line that
//! starts with `+ `, `- ` or `1. ` becomes a list item and Markdown escapes
//! like `\!` are eaten. `fence` rewrites each block into a ```` ```math ````
//! fence before parsing, keeping the line count so source line mapping holds.

/// Turn every closed multi-line `$$` block (opening at indent ≤ 3, outside
/// code fences) into a `math` fenced block with the same number of lines.
pub fn fence(source: &str) -> String {
    let lines: Vec<&str> = source.split_inclusive('\n').collect();
    let mut out = String::with_capacity(source.len() + 64);
    // Open code fence: (char, run length).
    let mut code: Option<(char, usize)> = None;
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let body = line.trim_end_matches(['\n', '\r']);
        let indent = body.len() - body.trim_start_matches(' ').len();
        let rest = &body[indent..];
        if let Some((ch, len)) = code {
            let run = rest.chars().take_while(|&c| c == ch).count();
            if indent <= 3 && run >= len && rest[run..].trim().is_empty() {
                code = None;
            }
        } else if indent <= 3 {
            if let Some(f) = code_fence_open(rest) {
                code = Some(f);
            } else if let Some(n) = math_block_len(&lines[i..], rest) {
                write_block(&mut out, &lines[i..i + n], &body[..indent]);
                i += n;
                continue;
            }
        }
        out.push_str(line);
        i += 1;
    }
    out
}

fn code_fence_open(rest: &str) -> Option<(char, usize)> {
    let ch = rest.chars().next().filter(|&c| c == '`' || c == '~')?;
    let run = rest.chars().take_while(|&c| c == ch).count();
    // A backtick fence's info string cannot contain backticks.
    (run >= 3 && !(ch == '`' && rest[run..].contains('`'))).then_some((ch, run))
}

/// Lines in the `$$` block starting at `lines[0]` (whose text after the
/// indent is `rest`), or None if it is not a closed multi-line block. A blank
/// line ends the search, as it would end the paragraph.
fn math_block_len(lines: &[&str], rest: &str) -> Option<usize> {
    let after = rest.strip_prefix("$$")?;
    if after.contains("$$") {
        return None; // single-line `$$…$$`: inline math handles it
    }
    for (k, l) in lines.iter().enumerate().skip(1) {
        let t = l.trim_end();
        if t.is_empty() {
            return None;
        }
        if t.ends_with("$$") {
            return Some(k + 1);
        }
    }
    None
}

fn write_block(out: &mut String, block: &[&str], indent: &str) {
    let n = block.len();
    let strip = |l: &str| l.trim_end_matches(['\n', '\r']).to_string();
    let first = strip(block[0]);
    let open = first.trim_start_matches(' ')[2..].trim().to_string();
    let last = strip(block[n - 1]);
    let close = last.trim_end()[..last.trim_end().len() - 2].to_string();
    let mut pieces: Vec<String> = Vec::with_capacity(n);
    if !open.is_empty() {
        pieces.push(open);
    }
    pieces.extend(block[1..n - 1].iter().map(|l| strip(l)));
    if !close.trim().is_empty() {
        pieces.push(close);
    }
    // Two fence lines replace the `$$` lines; fold extra pieces (math on
    // the `$$` lines) into their neighbours to keep the line count.
    let room = n - 2;
    if room == 0 {
        // `$$ a\nb $$`: no room for both fences and the math.
        for l in block {
            out.push_str(l);
        }
        return;
    }
    while pieces.len() > room {
        let second = pieces.remove(1);
        pieces[0] = format!("{} {}", pieces[0].trim_end(), second.trim_start());
    }
    while pieces.len() < room {
        pieces.push(String::new());
    }
    let longest = pieces
        .iter()
        .flat_map(|p| p.split(|c| c != '`'))
        .map(str::len)
        .max()
        .unwrap_or(0);
    let fence = "`".repeat((longest + 1).max(3));
    let nl = |l: &str| if l.ends_with("\r\n") { "\r\n" } else { "\n" };
    out.push_str(&format!("{indent}{fence}math{}", nl(block[0])));
    for p in &pieces {
        out.push_str(p);
        out.push('\n');
    }
    out.push_str(indent);
    out.push_str(&fence);
    if block[n - 1].ends_with('\n') {
        out.push_str(nl(block[n - 1]));
    }
}
