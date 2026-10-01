use crate::parser;
use crate::parser::frontmatter;
use crate::sanitize::sanitize_text;

/// Print document outline (heading structure).
///
/// Every document-derived string printed by this module goes through
/// [`sanitize_text`]: these subcommands write straight to the terminal, so a
/// heading like `# a\x1b]0;TITLE\x07` must not reach it as an escape.
pub fn print_outline(source: &str) {
    let (_, content) = frontmatter::strip_frontmatter(source);
    let doc = parser::parse(&content);

    if doc.headings.is_empty() {
        println!("  (no headings found)");
        return;
    }

    for h in &doc.headings {
        let indent = "  ".repeat((h.level as usize).saturating_sub(1));
        let marker = match h.level {
            1 => "█",
            2 => "▌",
            3 => "▎",
            _ => "·",
        };
        println!("{indent}{marker} {}", sanitize_text(&h.text));
    }
}

/// Print document statistics.
pub fn print_stats(source: &str, filename: &str) {
    let (fm, content) = frontmatter::strip_frontmatter(source);

    let doc = parser::parse(&content);

    let words = word_count(&content);
    let chars = content.chars().count();
    let lines = content.lines().count();
    let reading_time = (words as f64 / 200.0).ceil() as usize;
    let heading_count = doc.headings.len();
    let link_count = count_pattern(&content, "](");
    let code_block_count = content.matches("```").count() / 2;
    let image_count = count_pattern(&content, "![");
    let table_count = content.lines().filter(|l| l.contains("---|")).count();

    println!("╭─ {} ─╮", sanitize_text(filename));
    println!("│");
    println!("│  Words:         {words}");
    println!("│  Characters:    {chars}");
    println!("│  Lines:         {lines}");
    println!("│  Reading time:  ~{reading_time} min");
    println!("│");
    println!("│  Headings:      {heading_count}");
    println!("│  Links:         {link_count}");
    println!("│  Code blocks:   {code_block_count}");
    println!("│  Images:        {image_count}");
    println!("│  Tables:        {table_count}");
    if fm.is_some() {
        println!("│  Frontmatter:   yes");
    }
    println!("│");
    println!("╰───╯");
}

/// Print a real (Myers) line diff between two markdown files. A single
/// inserted line no longer marks everything after it as changed. `color`
/// adds bold headers and red/green lines; without it the output has no
/// escape sequences.
pub fn print_diff(source_a: &str, source_b: &str, name_a: &str, name_b: &str, color: bool) {
    use similar::{ChangeTag, TextDiff};

    let paint = |sgr: &str, text: String| {
        if color {
            format!("\x1b[{sgr}m{text}\x1b[0m")
        } else {
            text
        }
    };
    let (name_a, name_b) = (sanitize_text(name_a), sanitize_text(name_b));
    println!("{}", paint("1", format!("--- {name_a}")));
    println!("{}", paint("1", format!("+++ {name_b}")));
    println!();

    let diff = TextDiff::from_lines(source_a, source_b);
    for change in diff.iter_all_changes() {
        let line = sanitize_text(change.value().trim_end_matches('\n'));
        match change.tag() {
            ChangeTag::Delete => println!("{}", paint("31", format!("- {line}"))),
            ChangeTag::Insert => println!("{}", paint("32", format!("+ {line}"))),
            ChangeTag::Equal => println!("  {line}"),
        }
    }
}

fn count_pattern(text: &str, pattern: &str) -> usize {
    text.matches(pattern).count()
}

/// Compute word count and reading time for status bar display.
pub fn document_stats(source: &str) -> (usize, usize) {
    let words = word_count(source);
    let reading_time = (words as f64 / 200.0).ceil() as usize;
    (words, reading_time)
}

/// Whitespace-token counting undercounts scripts written without spaces: a
/// 600-character Chinese document is one "word". Han and kana characters
/// each count as a word of their own (the usual CJK convention); everything
/// else counts by whitespace tokens as before.
pub fn word_count(text: &str) -> usize {
    let is_cjk = |c: char| {
        matches!(c,
            '\u{3400}'..='\u{4DBF}'   // CJK ext A
            | '\u{4E00}'..='\u{9FFF}' // CJK unified
            | '\u{3040}'..='\u{30FF}' // hiragana + katakana
            | '\u{F900}'..='\u{FAFF}' // CJK compat
        )
    };
    text.split_whitespace()
        .map(|token| {
            let cjk = token.chars().filter(|&c| is_cjk(c)).count();
            let has_other = token.chars().any(|c| !is_cjk(c));
            cjk + usize::from(has_other)
        })
        .sum()
}
