//! LaTeX math to Unicode text: a dependency-free approximation for a
//! terminal, used for inline `$…$`, display `$$…$$` and ```` ```math ````.
//!
//! What it does:
//! - Greek letters, operators, relations, arrows and big operators become
//!   their Unicode symbols; `\mathbb{R}` → ℝ, `\mathcal{L}` → ℒ, and the
//!   like, where a code point exists.
//! - `x^2` / `a_{ij}` use Unicode super/subscripts when every character has
//!   one, else `^(…)` / `_(…)` (`^x` / `_x` for a single character).
//! - `\frac{a}{b}`: in display math stacked over a `─` bar; inline (and in
//!   scripts, roots, accents) `a/b`, a side in parentheses unless it is one
//!   token. `\sqrt{x}` → `√x` / `√(x+1)`, `\sqrt[n]{x}` → `ⁿ√x`.
//! - Display math sets the limits of `\sum`, `\prod`, `\lim`, `\max`, … above
//!   and below the operator; `\binom` stacks in tall parentheses (`C(n,k)`
//!   inline); `\underbrace` / `\overbrace` draw a brace with their label.
//! - `\mathbf{…}`, `\boldsymbol`, `\bm` → mathematical bold letters (𝐅).
//! - `\text{…}`, `\mathrm{…}` and friends → plain text; `\left`/`\right`,
//!   sizing and colour commands dropped; `\, \; \quad` → spaces, `\!` → "".
//! - `matrix`/`pmatrix`/`bmatrix`/`Bmatrix`/`vmatrix`/`Vmatrix`/`array`,
//!   `cases`, `aligned`/`align`/`split` and `gathered`: rows on their own
//!   lines, columns padded, brackets drawn from the bracket-piece glyphs
//!   (inline math puts them on one line: `(a b; c d)`).
//! - Unknown commands are kept verbatim with their brace arguments.
//!
//! Never panics on malformed input; a formula nested deeper than
//! [`MAX_DEPTH`] is shown as its source, in time linear in its length. In
//! ASCII mode nothing is converted: the source is shown with
//! `\left`/`\right` and spacing commands removed ([`clean`]).

use super::scripts;
use std::cell::Cell;
use unicode_width::UnicodeWidthStr;

/// Deepest nesting rendered; a formula that goes deeper is shown as its
/// source. A linear pre-scan ([`nesting`]) catches brace, `\sqrt[` and
/// environment nesting before any work; the check at the top of every
/// recursive entry (`Renderer::seq`, `Renderer::command`,
/// `Renderer::environment`) catches the rest (`\not\not…`), so no path can
/// recurse past it on hostile input.
const MAX_DEPTH: usize = 32;

/// Inline math as one line of text.
pub fn render_inline(src: &str, ascii: bool) -> String {
    if ascii {
        return clean(src).join(" ");
    }
    Renderer::new(true).top(src).flatten()
}

/// Display math as one or more lines (environments and `\\` give rows).
pub fn render_display(src: &str, ascii: bool) -> Vec<String> {
    if ascii {
        return clean(src);
    }
    Renderer::new(false)
        .top(src)
        .rows
        .into_iter()
        .map(|r| r.trim_end().to_string())
        .collect()
}

/// The source for ASCII mode: `\left`/`\right`, sizing and spacing commands
/// removed, whitespace collapsed, one entry per non-blank source line.
pub fn clean(src: &str) -> Vec<String> {
    src.lines()
        .map(|line| {
            let chars: Vec<char> = line.chars().collect();
            let mut out = String::new();
            let mut i = 0;
            while i < chars.len() {
                if chars[i] == '\\' && i + 1 < chars.len() {
                    let (name, next) = command_name(&chars, i + 1);
                    match name.as_str() {
                        "," | ":" | ";" | " " | "quad" | "qquad" => {
                            out.push(' ');
                            i = next;
                            continue;
                        }
                        "!" => {
                            i = next;
                            continue;
                        }
                        n if is_dropped(n) => {
                            i = next;
                            // `\left.` / `\right.`: no delimiter at all.
                            if matches!(n, "left" | "right") && chars.get(i) == Some(&'.') {
                                i += 1;
                            }
                            continue;
                        }
                        _ => {
                            out.extend(&chars[i..next]);
                            i = next;
                            continue;
                        }
                    }
                }
                out.push(chars[i]);
                i += 1;
            }
            out.split_whitespace().collect::<Vec<_>>().join(" ")
        })
        .filter(|l| !l.is_empty())
        .collect()
}

/// Commands that render as nothing: delimiter sizing, style switches.
fn is_dropped(name: &str) -> bool {
    matches!(
        name,
        "left"
            | "right"
            | "middle"
            | "big"
            | "Big"
            | "bigg"
            | "Bigg"
            | "bigl"
            | "bigr"
            | "Bigl"
            | "Bigr"
            | "biggl"
            | "biggr"
            | "Biggl"
            | "Biggr"
            | "displaystyle"
            | "textstyle"
            | "scriptstyle"
            | "scriptscriptstyle"
            | "limits"
            | "nolimits"
            | "nonumber"
            | "notag"
            | "hline"
    )
}

/// The command name starting at `chars[i]` (just after a backslash): a run
/// of letters, or one other character. Returns it and the index after it.
fn command_name(chars: &[char], i: usize) -> (String, usize) {
    match chars.get(i) {
        Some(c) if c.is_ascii_alphabetic() => {
            let end = chars[i..]
                .iter()
                .position(|c| !c.is_ascii_alphabetic())
                .map_or(chars.len(), |n| i + n);
            (chars[i..end].iter().collect(), end)
        }
        Some(&c) => (c.to_string(), i + 1),
        None => (String::new(), i),
    }
}

/// A rendered fragment: one or more rows, aligned with neighbours on its
/// baseline row when placed side by side.
#[derive(Debug, Clone)]
struct Block {
    rows: Vec<String>,
    base: usize,
}

impl Block {
    fn text(s: impl Into<String>) -> Self {
        Block {
            rows: vec![s.into()],
            base: 0,
        }
    }

    fn width(&self) -> usize {
        self.rows.iter().map(|r| r.width()).max().unwrap_or(0)
    }

    fn is_empty(&self) -> bool {
        self.rows.iter().all(|r| r.is_empty())
    }

    /// Place `other` to the right, baselines aligned.
    fn append(&mut self, other: Block) {
        if other.rows.len() == 1 && self.rows.len() == 1 {
            self.rows[0].push_str(&other.rows[0]);
            return;
        }
        let w = self.width();
        let above = self.base.max(other.base);
        let below = (self.rows.len() - self.base).max(other.rows.len() - other.base);
        let mut rows = vec![String::new(); above + below];
        for (k, r) in self.rows.iter().enumerate() {
            rows[above - self.base + k] = r.clone();
        }
        for row in rows.iter_mut() {
            let pad = w.saturating_sub(row.width());
            row.push_str(&" ".repeat(pad));
        }
        for (k, r) in other.rows.iter().enumerate() {
            rows[above - other.base + k].push_str(r);
        }
        self.rows = rows;
        self.base = above;
    }

    fn push_str(&mut self, s: &str) {
        self.append(Block::text(s));
    }

    fn ends_with_space(&self) -> bool {
        self.rows[self.base].ends_with(' ')
    }

    /// One line: rows joined with `; ` (a multi-row block met inline or in
    /// a script).
    fn flatten(&self) -> String {
        let rows: Vec<&str> = self
            .rows
            .iter()
            .map(|r| r.trim())
            .filter(|r| !r.is_empty())
            .collect();
        rows.join("; ")
    }
}

struct Renderer {
    /// Lay out on one line: inline math, and inside scripts, roots and
    /// accents of display math (see [`Renderer::linear`]).
    inline: Cell<bool>,
    /// Set when a recursive entry passed [`MAX_DEPTH`]: every level then
    /// returns at once and [`Renderer::top`] shows the source instead.
    capped: Cell<bool>,
}

/// The deepest nesting of `{…}`, `\sqrt[…]` and `\begin…\end` in `src`, in
/// one linear pass. Unbalanced closers never go below zero.
fn nesting(src: &str) -> usize {
    let chars: Vec<char> = src.chars().collect();
    let (mut depth, mut max, mut brackets) = (0usize, 0usize, 0usize);
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '\\' => {
                let (name, next) = command_name(&chars, i + 1);
                match name.as_str() {
                    "begin" => depth += 1,
                    "end" => depth = depth.saturating_sub(1),
                    "sqrt" => {
                        let mut j = next;
                        while chars.get(j).is_some_and(|c| c.is_whitespace()) {
                            j += 1;
                        }
                        if chars.get(j) == Some(&'[') {
                            depth += 1;
                            brackets += 1;
                            i = j + 1;
                            max = max.max(depth);
                            continue;
                        }
                    }
                    _ => {}
                }
                i = next.max(i + 1);
                max = max.max(depth);
                continue;
            }
            '{' => depth += 1,
            '}' => depth = depth.saturating_sub(1),
            ']' if brackets > 0 => {
                brackets -= 1;
                depth = depth.saturating_sub(1);
            }
            _ => {}
        }
        max = max.max(depth);
        i += 1;
    }
    max
}

/// The source as written, for a formula too deep to render: one entry per
/// non-blank line.
fn as_source(src: &str) -> Block {
    let rows: Vec<String> = src
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();
    if rows.is_empty() {
        return Block::text("");
    }
    Block { rows, base: 0 }
}

/// Where `Renderer::seq` stops.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Close {
    /// The end of input.
    End,
    /// The `}` closing a group.
    Brace,
    /// The `]` closing an optional argument.
    Bracket,
}

/// A cursor over the characters of one source string.
struct Cursor {
    s: Vec<char>,
    i: usize,
}

impl Cursor {
    fn new(src: &str) -> Self {
        Cursor {
            s: src.chars().collect(),
            i: 0,
        }
    }

    fn peek(&self) -> Option<char> {
        self.s.get(self.i).copied()
    }

    fn skip_ws(&mut self) {
        while self.peek().is_some_and(char::is_whitespace) {
            self.i += 1;
        }
    }

    /// The raw text of a brace group whose `{` was just consumed, up to its
    /// matching `}` (consumed too; end of input if unbalanced).
    fn raw_group(&mut self) -> String {
        let start = self.i;
        let mut depth = 1usize;
        while let Some(c) = self.peek() {
            self.i += 1;
            match c {
                '\\' => self.i += 1, // an escaped brace does not count
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        return self.s[start..self.i - 1].iter().collect();
                    }
                }
                _ => {}
            }
        }
        self.i = self.i.min(self.s.len());
        self.s[start.min(self.s.len())..].iter().collect()
    }

    /// A raw `{…}` argument after optional whitespace, or the next single
    /// character when there are no braces.
    fn raw_arg(&mut self) -> String {
        self.skip_ws();
        match self.peek() {
            Some('{') => {
                self.i += 1;
                self.raw_group()
            }
            Some(c) => {
                self.i += 1;
                c.to_string()
            }
            None => String::new(),
        }
    }
}

impl Renderer {
    fn new(inline: bool) -> Self {
        Renderer {
            inline: Cell::new(inline),
            capped: Cell::new(false),
        }
    }

    /// Run `f` with one-line layout: no stacked fractions or limits.
    fn linear<T>(&self, f: impl FnOnce() -> T) -> T {
        let was = self.inline.replace(true);
        let out = f();
        self.inline.set(was);
        out
    }

    /// Marks the render as too deep; every level unwinds from here.
    fn cap(&self) -> Block {
        self.capped.set(true);
        Block::text("")
    }

    /// A whole formula: split into rows on top-level `\\` (display) and
    /// aligned on `&` when it has any. Too deep to render: its source.
    fn top(&self, src: &str) -> Block {
        if nesting(src) > MAX_DEPTH {
            return as_source(src);
        }
        let out = self.layout(src);
        if self.capped.get() {
            return as_source(src);
        }
        out
    }

    fn layout(&self, src: &str) -> Block {
        let rows = split_top(src);
        if rows.len() == 1 && rows[0].len() == 1 {
            return self.render(&rows[0][0], 0);
        }
        if self.inline.get() {
            let lines: Vec<String> = rows
                .iter()
                .map(|cells| {
                    cells
                        .iter()
                        .map(|c| self.render(c, 1).flatten())
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .collect();
            return Block::text(lines.join("; "));
        }
        let kind = if rows.iter().any(|r| r.len() > 1) {
            "aligned"
        } else {
            "gathered"
        };
        self.grid(kind, &rows, 1)
    }

    /// Render `src` as a sequence, at nesting `depth`.
    fn render(&self, src: &str, depth: usize) -> Block {
        let mut cur = Cursor::new(src);
        self.seq(&mut cur, depth, Close::End)
    }

    /// Render up to `close`: the end of input, or up to and including the
    /// `}` / `]` that closes the group. Past [`MAX_DEPTH`], or once any
    /// level has been, it stops at once (see [`Renderer::capped`]).
    fn seq(&self, cur: &mut Cursor, depth: usize, close: Close) -> Block {
        if depth > MAX_DEPTH {
            return self.cap();
        }
        let mut out = Block::text("");
        while let Some(c) = cur.peek() {
            if self.capped.get() {
                return out;
            }
            match c {
                '}' => {
                    cur.i += 1;
                    if close == Close::Brace {
                        return out;
                    }
                    out.push_str("}");
                }
                ']' if close == Close::Bracket => {
                    cur.i += 1;
                    return out;
                }
                '{' => {
                    cur.i += 1;
                    let g = self.group(cur, depth);
                    out.append(g);
                }
                '^' | '_' => {
                    cur.i += 1;
                    let arg = self.linear(|| self.arg(cur, depth)).flatten();
                    let arg: String = arg.chars().filter(|c| !c.is_whitespace()).collect();
                    out.push_str(&script(&arg, c == '^'));
                }
                '\\' => {
                    cur.i += 1;
                    self.command(cur, depth, &mut out);
                }
                c if c.is_whitespace() => {
                    cur.skip_ws();
                    if !out.is_empty() && !out.ends_with_space() {
                        out.push_str(" ");
                    }
                }
                '&' => {
                    cur.i += 1;
                    if !out.ends_with_space() {
                        out.push_str(" ");
                    }
                }
                '\'' => {
                    cur.i += 1;
                    out.push_str("′");
                }
                '~' => {
                    cur.i += 1;
                    out.push_str(" ");
                }
                _ => {
                    cur.i += 1;
                    out.push_str(c.encode_utf8(&mut [0; 4]));
                }
            }
        }
        out
    }

    /// A brace group whose `{` was just consumed. Past the depth cap its
    /// content is shown as written (checked in `seq`).
    fn group(&self, cur: &mut Cursor, depth: usize) -> Block {
        self.seq(cur, depth + 1, Close::Brace)
    }

    /// One argument: a brace group, a command, or a single character.
    fn arg(&self, cur: &mut Cursor, depth: usize) -> Block {
        cur.skip_ws();
        match cur.peek() {
            Some('{') => {
                cur.i += 1;
                self.group(cur, depth)
            }
            Some('\\') => {
                cur.i += 1;
                let mut out = Block::text("");
                self.command(cur, depth + 1, &mut out);
                out
            }
            Some(c) => {
                cur.i += 1;
                Block::text(c.to_string())
            }
            None => Block::text(""),
        }
    }

    /// A command; its backslash was just consumed. Past [`MAX_DEPTH`] the
    /// render stops (see [`Renderer::capped`]).
    fn command(&self, cur: &mut Cursor, depth: usize, out: &mut Block) {
        if depth > MAX_DEPTH || self.capped.get() {
            self.cap();
            return;
        }
        let (name, next) = command_name(&cur.s, cur.i);
        cur.i = next;
        match name.as_str() {
            "" => out.push_str("\\"),
            "," | ":" | ";" | " " => out.push_str(" "),
            "quad" => out.push_str("  "),
            "qquad" => out.push_str("    "),
            "!" => {}
            "\\" => out.push_str(" "),
            "left" | "right" | "middle" => {
                cur.skip_ws();
                if cur.peek() == Some('.') {
                    cur.i += 1;
                }
            }
            n if is_dropped(n) => {}
            "frac" | "dfrac" | "cfrac" if !self.inline.get() => {
                let a = self.arg(cur, depth);
                let b = self.arg(cur, depth);
                out.append(fraction(a, b));
            }
            "frac" | "dfrac" | "tfrac" | "cfrac" => {
                let a = self.linear(|| self.arg(cur, depth)).flatten();
                let b = self.linear(|| self.arg(cur, depth)).flatten();
                out.push_str(&format!("{}/{}", operand(&a), operand(&b)));
            }
            "binom" | "dbinom" if !self.inline.get() => {
                let n = self.arg(cur, depth);
                let k = self.arg(cur, depth);
                out.append(binomial(n, k));
            }
            "binom" | "dbinom" | "tbinom" => {
                let n = self.linear(|| self.arg(cur, depth)).flatten();
                let k = self.linear(|| self.arg(cur, depth)).flatten();
                out.push_str(&format!("C({},{})", n.trim(), k.trim()));
            }
            "underbrace" | "overbrace" => {
                let under = name == "underbrace";
                if self.inline.get() {
                    let a = self.arg(cur, depth).flatten();
                    let label = self.label(cur, depth, if under { '_' } else { '^' });
                    let label = label.map(|l| script(&l, !under)).unwrap_or_default();
                    out.push_str(&format!("{}{label}", operand(&a)));
                } else {
                    let a = self.arg(cur, depth);
                    let label = self.label(cur, depth, if under { '_' } else { '^' });
                    out.append(braced(a, label, under));
                }
            }
            "sqrt" => {
                cur.skip_ws();
                let index = if cur.peek() == Some('[') {
                    cur.i += 1;
                    // Rendered in place up to its `]`: one pass, depth-capped.
                    let n = self
                        .linear(|| self.seq(cur, depth + 1, Close::Bracket))
                        .flatten();
                    let n: String = n.chars().filter(|c| !c.is_whitespace()).collect();
                    match scripts::to_superscript(&n) {
                        Some(s) => s,
                        None if n.is_empty() => String::new(),
                        None => format!("({n})"),
                    }
                } else {
                    String::new()
                };
                if !self.inline.get() {
                    let x = self.arg(cur, depth);
                    // A single token reads fine as `√x`; anything longer
                    // gets a drawn radical with a bar over it.
                    if x.rows.len() > 1 || !is_token(&x.flatten()) {
                        out.append(radical(x, &index));
                        return;
                    }
                    out.push_str(&format!("{index}√{}", x.flatten()));
                    return;
                }
                let x = self.linear(|| self.arg(cur, depth)).flatten();
                let x = if is_token(&x) { x } else { format!("({x})") };
                out.push_str(&format!("{index}√{x}"));
            }
            "text" | "textrm" | "textnormal" | "textit" | "textbf" | "texttt" | "textsf"
            | "textup" | "emph" | "mbox" | "hbox" => {
                out.push_str(&cur.raw_arg());
            }
            "mathbf" | "boldsymbol" | "bm" => {
                let a = self.arg(cur, depth);
                out.append(Block {
                    rows: a
                        .rows
                        .iter()
                        .map(|r| r.chars().map(|c| bold(c).unwrap_or(c)).collect())
                        .collect(),
                    base: a.base,
                });
            }
            "mathrm" | "mathit" | "mathsf" | "mathtt" | "mathnormal" | "operatorname" => {
                let a = self.arg(cur, depth);
                out.append(a);
            }
            "mathbb" | "Bbb" | "mathcal" | "mathscr" | "mathfrak" => {
                let a = self.linear(|| self.arg(cur, depth)).flatten();
                let map: fn(char) -> Option<char> = match name.as_str() {
                    "mathbb" | "Bbb" => double_struck,
                    "mathfrak" => fraktur,
                    _ => script_letter,
                };
                out.push_str(&a.chars().map(|c| map(c).unwrap_or(c)).collect::<String>());
            }
            "hat" | "widehat" | "check" | "breve" | "acute" | "grave" | "vec" | "dot" | "ddot"
            | "tilde" | "widetilde" | "bar" | "overline" | "underline" | "overrightarrow" => {
                let a = self.linear(|| self.arg(cur, depth)).flatten();
                out.push_str(&accent(&name, &a));
            }
            "not" => {
                cur.skip_ws();
                let next = self.linear(|| self.arg(cur, depth)).flatten();
                out.push_str(&match next.as_str() {
                    "=" => "≠".to_string(),
                    "∈" => "∉".to_string(),
                    "⊂" => "⊄".to_string(),
                    "≡" => "≢".to_string(),
                    _ => format!("{next}\u{338}"),
                });
            }
            "pmod" => {
                let a = self.linear(|| self.arg(cur, depth)).flatten();
                out.push_str(&format!(" (mod {a})"));
            }
            "bmod" | "mod" => out.push_str(" mod "),
            "color" => {
                cur.raw_arg();
            }
            "textcolor" | "colorbox" => {
                cur.raw_arg();
                let a = self.arg(cur, depth);
                out.append(a);
            }
            "label" => {
                cur.raw_arg();
            }
            "tag" => {
                let a = cur.raw_arg();
                out.push_str(&format!("  ({a})"));
            }
            "begin" => {
                let env = cur.raw_arg();
                let block = self.environment(cur, env.trim(), depth);
                out.append(block);
            }
            _ => {
                let named = symbol(&name).or(is_function(&name).then_some(name.as_str()));
                if let Some(text) = named {
                    // `\partial x`, `\nabla f`: the space only ends the
                    // command name; the operator binds to its operand.
                    if matches!(name.as_str(), "partial" | "nabla") {
                        let start = cur.i;
                        cur.skip_ws();
                        let operand = match cur.peek() {
                            Some('\\') => {
                                let (next, _) = command_name(&cur.s, cur.i + 1);
                                symbol(&next).is_some_and(|s| s.starts_with(char::is_alphabetic))
                                    || next.starts_with("math")
                                    || matches!(next.as_str(), "boldsymbol" | "bm")
                            }
                            Some(c) => c.is_alphanumeric(),
                            None => false,
                        };
                        if !operand {
                            cur.i = start;
                        }
                    }
                    match self.limits(cur, depth, &name, text) {
                        // TeX's thin space after an operator: `\lim_{x}\frac`
                        // would otherwise run into the fraction bar.
                        Some(block) => {
                            out.append(block);
                            if cur.peek().is_some_and(|c| !c.is_whitespace()) {
                                out.push_str(" ");
                            }
                        }
                        None => out.push_str(text),
                    }
                } else {
                    // Unknown: kept as written, brace arguments included.
                    out.push_str(&format!("\\{name}"));
                    while cur.peek() == Some('{') {
                        cur.i += 1;
                        let g = self.linear(|| self.group(cur, depth)).flatten();
                        out.push_str(&format!("{{{g}}}"));
                    }
                }
            }
        }
    }

    /// Display math: the `_` / `^` limits after a big operator or `\lim`
    /// stacked below and above it. `None` (cursor untouched) inline, for
    /// other commands, with `\nolimits`, or when there are no limits.
    fn limits(&self, cur: &mut Cursor, depth: usize, name: &str, op: &str) -> Option<Block> {
        if self.inline.get() || !takes_limits(name) {
            return None;
        }
        let start = cur.i;
        let (mut sup, mut sub) = (None, None);
        loop {
            // Whitespace after the last limit stays for `seq` to see.
            let before = cur.i;
            cur.skip_ws();
            match cur.peek() {
                Some(c @ ('^' | '_')) => {
                    cur.i += 1;
                    let a = self.linear(|| self.arg(cur, depth)).flatten();
                    let a: String = a.chars().filter(|c| !c.is_whitespace()).collect();
                    *(if c == '^' { &mut sup } else { &mut sub }) = Some(Block::text(a));
                }
                Some('\\') if command_name(&cur.s, cur.i + 1).0 == "limits" => {
                    cur.i = command_name(&cur.s, cur.i + 1).1;
                }
                _ => {
                    cur.i = before;
                    break;
                }
            }
        }
        if sup.is_none() && sub.is_none() {
            cur.i = start;
            return None;
        }
        Some(stack(sup, Block::text(op), sub))
    }

    /// The `_…` / `^…` label after `\underbrace{…}` / `\overbrace{…}`, if
    /// the next token is `mark`; the cursor is untouched otherwise.
    fn label(&self, cur: &mut Cursor, depth: usize, mark: char) -> Option<String> {
        let start = cur.i;
        cur.skip_ws();
        if cur.peek() != Some(mark) {
            cur.i = start;
            return None;
        }
        cur.i += 1;
        let a = self.linear(|| self.arg(cur, depth)).flatten();
        Some(a.chars().filter(|c| !c.is_whitespace()).collect())
    }

    /// `\begin{env}` … `\end{env}`; the `\begin{env}` was just consumed.
    fn environment(&self, cur: &mut Cursor, env: &str, depth: usize) -> Block {
        if depth + 1 > MAX_DEPTH || self.capped.get() {
            return self.cap();
        }
        // Column spec of `array`, position of `aligned`: not needed.
        if env == "array" {
            cur.raw_arg();
        }
        cur.skip_ws();
        if cur.peek() == Some('[') {
            while cur.peek().is_some_and(|c| c != ']') {
                cur.i += 1;
            }
            cur.i = (cur.i + 1).min(cur.s.len());
        }
        let body = end_of_environment(cur);
        let rows = split_top(&body);
        if self.inline.get() {
            let (l, r) = match env {
                "pmatrix" => ("(", ")"),
                "bmatrix" => ("[", "]"),
                "Bmatrix" => ("{", "}"),
                "vmatrix" => ("|", "|"),
                "Vmatrix" => ("‖", "‖"),
                "cases" => ("{ ", ""),
                _ => ("", ""),
            };
            let lines: Vec<String> = rows
                .iter()
                .map(|cells| {
                    cells
                        .iter()
                        .map(|c| self.render(c, depth + 1).flatten())
                        .filter(|c| !c.is_empty())
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .collect();
            return Block::text(format!("{l}{}{r}", lines.join("; ")));
        }
        self.grid(env, &rows, depth + 1)
    }

    /// Lay out rows of cells for display: columns padded by the
    /// environment's alignment, cells aligned on their baselines, brackets
    /// drawn around matrices and cases. Rows taller than one line (a stacked
    /// fraction in a cell) are separated by a blank line.
    fn grid(&self, env: &str, rows: &[Vec<String>], depth: usize) -> Block {
        let cells: Vec<Vec<Block>> = rows
            .iter()
            .map(|r| r.iter().map(|c| self.render(c, depth)).collect())
            .collect();
        let ncols = cells.iter().map(|r| r.len()).max().unwrap_or(0);
        let mut widths = vec![0usize; ncols];
        for r in &cells {
            for (j, c) in r.iter().enumerate() {
                widths[j] = widths[j].max(c.width());
            }
        }
        #[derive(Clone, Copy)]
        enum Align {
            Left,
            Right,
            Center,
        }
        let aligned = matches!(
            env,
            "aligned"
                | "align"
                | "align*"
                | "alignat"
                | "alignat*"
                | "split"
                | "eqnarray"
                | "eqnarray*"
                | "flalign"
                | "flalign*"
        );
        let (align_of, gap): (Box<dyn Fn(usize) -> Align>, &str) = if aligned {
            // `lhs &= rhs`: right, left, right, left, …
            (
                Box::new(|j| {
                    if j % 2 == 0 {
                        Align::Right
                    } else {
                        Align::Left
                    }
                }),
                " ",
            )
        } else if env == "cases" {
            (Box::new(|_| Align::Left), "  ")
        } else {
            (Box::new(|_| Align::Center), "  ")
        };
        let tall = cells.iter().flatten().any(|c| c.rows.len() > 1);
        let blank = Block::text("");
        let mut lines: Vec<String> = Vec::new();
        for (ri, r) in cells.iter().enumerate() {
            if tall && ri > 0 {
                lines.push(String::new());
            }
            let above = r.iter().map(|c| c.base).max().unwrap_or(0);
            let below = r
                .iter()
                .map(|c| c.rows.len().saturating_sub(c.base))
                .max()
                .unwrap_or(1)
                .max(1);
            let mut row_lines = vec![String::new(); above + below];
            for (j, w) in widths.iter().enumerate() {
                let c = r.get(j).unwrap_or(&blank);
                // An empty aligned cell (`&= b` with nothing before) adds
                // no gap.
                let gap_here = j > 0 && !(aligned && c.is_empty());
                let cw = c.width();
                let free = w.saturating_sub(cw);
                let (l, rt) = match align_of(j) {
                    Align::Left => (0, free),
                    Align::Right => (free, 0),
                    Align::Center => (free / 2, free - free / 2),
                };
                let top = above - c.base;
                for (k, line) in row_lines.iter_mut().enumerate() {
                    if gap_here {
                        line.push_str(gap);
                    }
                    let text = k
                        .checked_sub(top)
                        .and_then(|i| c.rows.get(i))
                        .map_or("", String::as_str);
                    line.push_str(&" ".repeat(l));
                    line.push_str(text);
                    line.push_str(&" ".repeat(rt + cw.saturating_sub(text.width())));
                }
            }
            lines.extend(row_lines);
        }
        if aligned {
            // Columns with no content anywhere (`&=` at a line start) leave
            // leading blanks: remove the common indent.
            let indent = lines
                .iter()
                .filter(|l| !l.trim().is_empty())
                .map(|l| l.len() - l.trim_start_matches(' ').len())
                .min()
                .unwrap_or(0);
            for l in &mut lines {
                l.drain(..indent.min(l.len()));
            }
        }
        if lines.is_empty() {
            lines.push(String::new());
        }
        let n = lines.len();
        let brackets = match env {
            "pmatrix" => Some((PARENS.0, PARENS.1)),
            "bmatrix" => Some((["[", "⎡", "⎢", "⎣"], ["]", "⎤", "⎥", "⎦"])),
            "Bmatrix" => Some((["{", "⎧", "⎪", "⎩"], ["}", "⎫", "⎪", "⎭"])),
            "vmatrix" => Some((["|", "│", "│", "│"], ["|", "│", "│", "│"])),
            "Vmatrix" => Some((["‖", "‖", "‖", "‖"], ["‖", "‖", "‖", "‖"])),
            "cases" => Some((["{", "⎧", "⎪", "⎩"], ["", "", "", ""])),
            _ => None,
        };
        let rows = match brackets {
            None => lines,
            Some((left, right)) => {
                bracketed(&lines, left, right, env == "cases" || env == "Bmatrix")
            }
        };
        Block {
            base: (n - 1) / 2,
            rows,
        }
    }
}

/// Bracket pieces, left and right: one-line, top, middle, bottom.
type Pieces = [&'static str; 4];

const PARENS: (Pieces, Pieces) = (["(", "⎛", "⎜", "⎝"], [")", "⎞", "⎟", "⎠"]);

/// `lines` between brackets as tall as they are; a brace gets its middle
/// point. An empty right side draws nothing (`cases`).
fn bracketed(lines: &[String], left: Pieces, right: Pieces, brace: bool) -> Vec<String> {
    let n = lines.len();
    let w = lines.iter().map(|l| l.width()).max().unwrap_or(0);
    lines
        .iter()
        .enumerate()
        .map(|(k, l)| {
            let piece = |set: Pieces| -> &'static str {
                if n == 1 {
                    set[0]
                } else if k == 0 {
                    set[1]
                } else if k == n - 1 {
                    set[3]
                } else if brace && k == (n - 1) / 2 {
                    // The brace's middle point.
                    match set[1] {
                        "⎧" => "⎨",
                        "⎫" => "⎬",
                        other => other,
                    }
                } else {
                    set[2]
                }
            };
            let pad = " ".repeat(w.saturating_sub(l.width()));
            let r = piece(right);
            if r.is_empty() {
                format!("{} {l}", piece(left))
            } else {
                format!("{} {l}{pad} {r}", piece(left))
            }
        })
        .collect()
}

/// `block`'s rows centred in `w` columns, padded to the full width so a
/// following space is seen (`Block::ends_with_space`).
fn centred(block: &Block, w: usize) -> Vec<String> {
    let pad = " ".repeat(w.saturating_sub(block.width()) / 2);
    block
        .rows
        .iter()
        .map(|r| {
            let right = w.saturating_sub(pad.len() + r.width());
            format!("{pad}{r}{}", " ".repeat(right))
        })
        .collect()
}

/// `top`, `mid` and `bottom` centred over one another; the baseline is
/// `mid`'s.
fn stack(top: Option<Block>, mid: Block, bottom: Option<Block>) -> Block {
    let w = [top.as_ref(), Some(&mid), bottom.as_ref()]
        .into_iter()
        .flatten()
        .map(Block::width)
        .max()
        .unwrap_or(0);
    let mut rows = top.map(|b| centred(&b, w)).unwrap_or_default();
    let base = rows.len() + mid.base;
    rows.extend(centred(&mid, w));
    if let Some(b) = bottom {
        rows.extend(centred(&b, w));
    }
    Block { rows, base }
}

/// A display fraction: numerator over a `─` bar one column wider than
/// either side on each end, over the denominator; the bar is the baseline.
fn fraction(num: Block, den: Block) -> Block {
    let w = num.width().max(den.width()) + 2;
    stack(Some(num), Block::text("─".repeat(w)), Some(den))
}

/// A display root: `╲╱` rising along the radicand's rows to a `_` bar over
/// it, the index (already superscript) before the foot. The radicand keeps
/// its baseline; the bar adds a row on top.
///
/// ```text
///    ______        ____
/// ³╲╱ x + 1       ╱  a
///                ╱  ───
///              ╲╱    b
/// ```
fn radical(body: Block, index: &str) -> Block {
    let h = body.rows.len();
    let w = body.width();
    let lead = " ".repeat(index.width());
    // Gutter: the foot `╲` in column 0, then one `╱` per row, rising.
    let gutter = h + 1;
    let mut rows = vec![format!("{lead}{}{}", " ".repeat(gutter), "_".repeat(w + 1))];
    for (i, r) in body.rows.iter().enumerate() {
        let slash = h - i; // column of this row's `╱`
        let mut g: Vec<char> = vec![' '; gutter];
        g[slash] = '╱';
        let prefix = if i == h - 1 {
            g[0] = '╲';
            index
        } else {
            lead.as_str()
        };
        rows.push(format!("{prefix}{} {r}", g.into_iter().collect::<String>()));
    }
    Block {
        rows,
        base: body.base + 1,
    }
}

/// A display binomial: `n` over `k` in parentheses as tall as both.
fn binomial(n: Block, k: Block) -> Block {
    let w = n.width().max(k.width());
    let mut lines = centred(&n, w);
    lines.extend(centred(&k, w));
    let base = (lines.len() - 1) / 2;
    Block {
        rows: bracketed(&lines, PARENS.0, PARENS.1, false),
        base,
    }
}

/// `\underbrace` / `\overbrace` in display: the brace drawn under (over)
/// `body` with its tip at the middle, the label beyond it.
fn braced(body: Block, label: Option<String>, under: bool) -> Block {
    let w = body.width();
    let (l, tip, r) = if under {
        ('╰', '┬', '╯')
    } else {
        ('╭', '┴', '╮')
    };
    let line: String = (0..w)
        .map(|i| match i {
            0 => l,
            _ if i == w - 1 => r,
            _ if i == w / 2 => tip,
            _ => '─',
        })
        .collect();
    let line = Block::text(line);
    let label = label.map(Block::text);
    if under {
        stack(None, body, Some(stack(None, line, label)))
    } else {
        stack(Some(stack(label, line, None)), body, None)
    }
}

/// Operators whose display-math limits go above and below them.
fn takes_limits(name: &str) -> bool {
    matches!(
        name,
        "sum"
            | "prod"
            | "coprod"
            | "bigcup"
            | "bigcap"
            | "bigsqcup"
            | "bigvee"
            | "bigwedge"
            | "bigodot"
            | "bigoplus"
            | "bigotimes"
            | "biguplus"
            | "lim"
            | "liminf"
            | "limsup"
            | "max"
            | "min"
            | "sup"
            | "inf"
            | "det"
            | "gcd"
            | "Pr"
    )
}

/// Mathematical bold (`\mathbf`): A–Z, a–z and 0–9 have code points.
fn bold(c: char) -> Option<char> {
    let (start, first) = match c {
        'A'..='Z' => (0x1D400, 'A'),
        'a'..='z' => (0x1D41A, 'a'),
        '0'..='9' => (0x1D7CE, '0'),
        _ => return None,
    };
    char::from_u32(start + (c as u32 - first as u32))
}

/// Everything up to the `\end{…}` that closes the environment just opened
/// (nested environments counted); the cursor ends after it. Unclosed: the
/// rest of the input.
fn end_of_environment(cur: &mut Cursor) -> String {
    let start = cur.i;
    let mut depth = 1usize;
    while cur.i < cur.s.len() {
        if cur.s[cur.i] == '\\' {
            let (name, next) = command_name(&cur.s, cur.i + 1);
            match name.as_str() {
                "begin" => depth += 1,
                "end" => {
                    depth -= 1;
                    if depth == 0 {
                        let body: String = cur.s[start..cur.i].iter().collect();
                        cur.i = next;
                        cur.raw_arg();
                        return body;
                    }
                }
                _ => {}
            }
            cur.i = next.max(cur.i + 1);
            continue;
        }
        cur.i += 1;
    }
    cur.s[start..].iter().collect()
}

/// Split at top-level `\\` (rows) and `&` (cells); braces and nested
/// environments are not split. A trailing empty row is dropped.
fn split_top(src: &str) -> Vec<Vec<String>> {
    let chars: Vec<char> = src.chars().collect();
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut cells: Vec<String> = Vec::new();
    let mut cell = String::new();
    let (mut braces, mut envs) = (0usize, 0usize);
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '\\' => {
                let (name, next) = command_name(&chars, i + 1);
                match name.as_str() {
                    "\\" if braces == 0 && envs == 0 => {
                        cells.push(std::mem::take(&mut cell));
                        rows.push(std::mem::take(&mut cells));
                        // `\\[2pt]`: an optional spacing argument.
                        i = next;
                        if chars.get(i) == Some(&'[') {
                            while i < chars.len() && chars[i] != ']' {
                                i += 1;
                            }
                            i += 1;
                        }
                        continue;
                    }
                    "begin" => envs += 1,
                    "end" => envs = envs.saturating_sub(1),
                    _ => {}
                }
                cell.extend(&chars[i..next.min(chars.len())]);
                i = next.max(i + 1);
                continue;
            }
            '{' => braces += 1,
            '}' => braces = braces.saturating_sub(1),
            '&' if braces == 0 && envs == 0 => {
                cells.push(std::mem::take(&mut cell));
                i += 1;
                continue;
            }
            _ => {}
        }
        cell.push(c);
        i += 1;
    }
    cells.push(cell);
    rows.push(cells);
    if rows.len() > 1
        && rows
            .last()
            .is_some_and(|r| r.iter().all(|c| c.trim().is_empty()))
    {
        rows.pop();
    }
    for row in &mut rows {
        for cell in row.iter_mut() {
            *cell = cell.trim().to_string();
        }
    }
    rows
}

/// `x` raised or lowered: Unicode when every character maps, else
/// `^x` / `^(xy)`.
fn script(arg: &str, sup: bool) -> String {
    if arg.is_empty() {
        return String::new();
    }
    if sup && arg.chars().all(|c| c == '′') {
        return arg.to_string();
    }
    if sup {
        scripts::superscript(arg, false, true)
    } else {
        scripts::subscript(arg, false, true)
    }
}

/// One token: a run of letters and digits (any script, super/subscripts
/// included), or something already in parentheses.
fn is_token(s: &str) -> bool {
    if s.is_empty() {
        return true;
    }
    if s.starts_with('(') && s.ends_with(')') {
        // Only when the opening parenthesis closes at the very end.
        let mut depth = 0i32;
        for (k, c) in s.char_indices() {
            match c {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        return k == s.len() - 1;
                    }
                }
                _ => {}
            }
        }
        return false;
    }
    s.chars().all(|c| {
        c.is_alphanumeric()
            || matches!(c, '.' | '′' | '∞' | '∂' | '∇')
            || ('\u{300}'..='\u{36f}').contains(&c)
            || ('\u{20d0}'..='\u{20ff}').contains(&c)
            || scripts_char(c)
    })
}

/// A Unicode super/subscript character (they count as part of a token).
fn scripts_char(c: char) -> bool {
    "⁰¹²³⁴⁵⁶⁷⁸⁹⁺⁻⁼⁽⁾₀₁₂₃₄₅₆₇₈₉₊₋₌₍₎".contains(c)
}

/// A fraction side: in parentheses unless it is one token.
fn operand(s: &str) -> String {
    if is_token(s) {
        s.to_string()
    } else {
        format!("({s})")
    }
}

/// An accent: a combining mark on a single character (on every character
/// for bars and underlines), else `name(text)`.
fn accent(name: &str, text: &str) -> String {
    let (mark, every) = match name {
        "hat" | "widehat" => ('\u{302}', false),
        "check" => ('\u{30c}', false),
        "breve" => ('\u{306}', false),
        "acute" => ('\u{301}', false),
        "grave" => ('\u{300}', false),
        "vec" | "overrightarrow" => ('\u{20d7}', false),
        "dot" => ('\u{307}', false),
        "ddot" => ('\u{308}', false),
        "tilde" | "widetilde" => ('\u{303}', false),
        "bar" => ('\u{304}', false),
        "overline" => ('\u{305}', true),
        _ => ('\u{332}', true), // underline
    };
    let n = text.chars().count();
    if n == 1 || (every && n > 0) {
        text.chars().flat_map(|c| [c, mark]).collect()
    } else {
        format!("{name}({text})")
    }
}

fn double_struck(c: char) -> Option<char> {
    Some(match c {
        'C' => 'ℂ',
        'H' => 'ℍ',
        'N' => 'ℕ',
        'P' => 'ℙ',
        'Q' => 'ℚ',
        'R' => 'ℝ',
        'Z' => 'ℤ',
        'A'..='Z' => char::from_u32(0x1D538 + (c as u32 - 'A' as u32))?,
        'a'..='z' => char::from_u32(0x1D552 + (c as u32 - 'a' as u32))?,
        '0'..='9' => char::from_u32(0x1D7D8 + (c as u32 - '0' as u32))?,
        _ => return None,
    })
}

fn script_letter(c: char) -> Option<char> {
    Some(match c {
        'B' => 'ℬ',
        'E' => 'ℰ',
        'F' => 'ℱ',
        'H' => 'ℋ',
        'I' => 'ℐ',
        'L' => 'ℒ',
        'M' => 'ℳ',
        'R' => 'ℛ',
        'A'..='Z' => char::from_u32(0x1D49C + (c as u32 - 'A' as u32))?,
        'e' => 'ℯ',
        'g' => 'ℊ',
        'o' => 'ℴ',
        'a'..='z' => char::from_u32(0x1D4B6 + (c as u32 - 'a' as u32))?,
        _ => return None,
    })
}

fn fraktur(c: char) -> Option<char> {
    Some(match c {
        'C' => 'ℭ',
        'H' => 'ℌ',
        'I' => 'ℑ',
        'R' => 'ℜ',
        'Z' => 'ℨ',
        'A'..='Z' => char::from_u32(0x1D504 + (c as u32 - 'A' as u32))?,
        'a'..='z' => char::from_u32(0x1D51E + (c as u32 - 'a' as u32))?,
        _ => return None,
    })
}

/// Named functions set upright in LaTeX: shown as their name.
fn is_function(name: &str) -> bool {
    matches!(
        name,
        "sin"
            | "cos"
            | "tan"
            | "cot"
            | "sec"
            | "csc"
            | "arcsin"
            | "arccos"
            | "arctan"
            | "sinh"
            | "cosh"
            | "tanh"
            | "coth"
            | "log"
            | "ln"
            | "lg"
            | "exp"
            | "lim"
            | "liminf"
            | "limsup"
            | "max"
            | "min"
            | "sup"
            | "inf"
            | "det"
            | "dim"
            | "ker"
            | "gcd"
            | "deg"
            | "arg"
            | "hom"
            | "Pr"
    )
}

/// A symbol command's Unicode character.
fn symbol(name: &str) -> Option<&'static str> {
    Some(match name {
        // Greek, lower case
        "alpha" => "α",
        "beta" => "β",
        "gamma" => "γ",
        "delta" => "δ",
        "epsilon" => "ϵ",
        "varepsilon" => "ε",
        "zeta" => "ζ",
        "eta" => "η",
        "theta" => "θ",
        "vartheta" => "ϑ",
        "iota" => "ι",
        "kappa" => "κ",
        "lambda" => "λ",
        "mu" => "μ",
        "nu" => "ν",
        "xi" => "ξ",
        "omicron" => "ο",
        "pi" => "π",
        "varpi" => "ϖ",
        "rho" => "ρ",
        "varrho" => "ϱ",
        "sigma" => "σ",
        "varsigma" => "ς",
        "tau" => "τ",
        "upsilon" => "υ",
        "phi" => "ϕ",
        "varphi" => "φ",
        "chi" => "χ",
        "psi" => "ψ",
        "omega" => "ω",
        // Greek, capitals
        "Gamma" => "Γ",
        "Delta" => "Δ",
        "Theta" => "Θ",
        "Lambda" => "Λ",
        "Xi" => "Ξ",
        "Pi" => "Π",
        "Sigma" => "Σ",
        "Upsilon" => "Υ",
        "Phi" => "Φ",
        "Psi" => "Ψ",
        "Omega" => "Ω",
        // Binary operators
        "times" => "×",
        "cdot" => "·",
        "div" => "÷",
        "pm" => "±",
        "mp" => "∓",
        "ast" => "∗",
        "star" => "⋆",
        "circ" => "∘",
        "bullet" => "•",
        "oplus" => "⊕",
        "ominus" => "⊖",
        "otimes" => "⊗",
        "odot" => "⊙",
        "wedge" | "land" => "∧",
        "vee" | "lor" => "∨",
        "cup" => "∪",
        "cap" => "∩",
        "setminus" => "∖",
        // Relations
        "leq" | "le" => "≤",
        "geq" | "ge" => "≥",
        "neq" | "ne" => "≠",
        "approx" => "≈",
        "equiv" => "≡",
        "sim" => "∼",
        "simeq" => "≃",
        "cong" => "≅",
        "propto" => "∝",
        "ll" => "≪",
        "gg" => "≫",
        "prec" => "≺",
        "succ" => "≻",
        "perp" => "⊥",
        "parallel" => "∥",
        "mid" => "∣",
        "in" => "∈",
        "notin" => "∉",
        "ni" => "∋",
        "subset" => "⊂",
        "subseteq" => "⊆",
        "supset" => "⊃",
        "supseteq" => "⊇",
        "models" => "⊨",
        "vdash" => "⊢",
        // Arrows
        "to" | "rightarrow" => "→",
        "leftarrow" | "gets" => "←",
        "leftrightarrow" => "↔",
        "Rightarrow" => "⇒",
        "Leftarrow" => "⇐",
        "Leftrightarrow" => "⇔",
        "implies" | "Longrightarrow" => "⟹",
        "impliedby" | "Longleftarrow" => "⟸",
        "iff" | "Longleftrightarrow" => "⟺",
        "longrightarrow" => "⟶",
        "longleftarrow" => "⟵",
        "mapsto" => "↦",
        "uparrow" => "↑",
        "downarrow" => "↓",
        "nearrow" => "↗",
        "searrow" => "↘",
        // Big operators
        "sum" => "∑",
        "prod" => "∏",
        "coprod" => "∐",
        "int" => "∫",
        "iint" => "∬",
        "iiint" => "∭",
        "oint" => "∮",
        "bigcup" => "⋃",
        "bigcap" => "⋂",
        "bigoplus" => "⨁",
        "bigotimes" => "⨂",
        // Logic, sets, misc
        "forall" => "∀",
        "exists" => "∃",
        "nexists" => "∄",
        "neg" | "lnot" => "¬",
        "emptyset" | "varnothing" => "∅",
        "infty" => "∞",
        "partial" => "∂",
        "nabla" => "∇",
        "hbar" => "ℏ",
        "ell" => "ℓ",
        "Re" => "ℜ",
        "Im" => "ℑ",
        "aleph" => "ℵ",
        "wp" => "℘",
        "angle" => "∠",
        "triangle" => "△",
        "square" => "□",
        "degree" => "°",
        "prime" => "′",
        "dagger" => "†",
        "ldots" | "dots" | "dotsc" | "dotsb" => "…",
        "cdots" => "⋯",
        "vdots" => "⋮",
        "ddots" => "⋱",
        "therefore" => "∴",
        "because" => "∵",
        // Delimiters
        "langle" => "⟨",
        "rangle" => "⟩",
        "lceil" => "⌈",
        "rceil" => "⌉",
        "lfloor" => "⌊",
        "rfloor" => "⌋",
        "lbrace" | "{" => "{",
        "rbrace" | "}" => "}",
        "lvert" | "rvert" | "vert" => "|",
        "lVert" | "rVert" | "Vert" | "|" => "‖",
        "backslash" => "\\",
        // Escaped characters
        "%" => "%",
        "$" => "$",
        "#" => "#",
        "&" => "&",
        "_" => "_",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inline(s: &str) -> String {
        render_inline(s, false)
    }

    fn display(s: &str) -> Vec<String> {
        render_display(s, false)
    }

    #[test]
    fn greek_letters_and_capitals() {
        assert_eq!(inline(r"\alpha + \beta = \Gamma"), "α + β = Γ");
        assert_eq!(inline(r"\omega\Omega\pi"), "ωΩπ");
    }

    #[test]
    fn operators_relations_and_arrows() {
        assert_eq!(
            inline(r"a \times b \cdot c \div d \pm e \mp f"),
            "a × b · c ÷ d ± e ∓ f"
        );
        assert_eq!(
            inline(
                r"\leq \geq \neq \approx \equiv \to \rightarrow \leftarrow \Rightarrow \Leftrightarrow"
            ),
            "≤ ≥ ≠ ≈ ≡ → → ← ⇒ ⇔"
        );
        assert_eq!(
            inline(
                r"\infty \partial \nabla \sum \prod \int \in \notin \subset \subseteq \cup \cap"
            ),
            "∞ ∂ ∇ ∑ ∏ ∫ ∈ ∉ ⊂ ⊆ ∪ ∩"
        );
        assert_eq!(
            inline(r"\forall \exists \emptyset \ldots \cdots \dots \langle \rangle \circ \degree"),
            "∀ ∃ ∅ … ⋯ … ⟨ ⟩ ∘ °"
        );
    }

    #[test]
    fn blackboard_calligraphic_and_fraktur() {
        assert_eq!(inline(r"\mathbb{R} \mathbb{N} \mathbb{A}"), "ℝ ℕ 𝔸");
        assert_eq!(inline(r"\mathcal{L} \mathcal{A}"), "ℒ 𝒜");
        assert_eq!(inline(r"\mathfrak{g}"), "𝔤");
        // No code point for punctuation: kept.
        assert_eq!(inline(r"\mathbb{+}"), "+");
    }

    #[test]
    fn text_and_roman_are_plain() {
        assert_eq!(inline(r"\text{if } x > 0"), "if x > 0");
        assert_eq!(inline(r"\mathrm{d}x"), "dx");
        assert_eq!(inline(r"\operatorname{rank}(A)"), "rank(A)");
        // \text keeps its content literal: no math conversion inside.
        assert_eq!(inline(r"\text{a^b}"), "a^b");
    }

    #[test]
    fn left_right_and_spacing_dropped() {
        assert_eq!(inline(r"\left( x \right)"), "( x )");
        assert_eq!(inline(r"\left. f \right|_0"), "f |₀");
        assert_eq!(inline(r"f(x)\,dx"), "f(x) dx");
        assert_eq!(inline(r"a\quad b\qquad c\!d\;e"), "a  b    cd e");
    }

    #[test]
    fn superscripts_and_subscripts() {
        assert_eq!(inline("x^2"), "x²");
        assert_eq!(inline("x_i"), "xᵢ");
        assert_eq!(inline("x^{n+1}"), "xⁿ⁺¹");
        assert_eq!(inline("a_{ij}"), "aᵢⱼ");
        assert_eq!(inline("x^{n + 1}"), "xⁿ⁺¹", "spaces are not significant");
        // No superscript q / subscript b: the fallback forms.
        assert_eq!(inline("x^q"), "x^q");
        assert_eq!(inline("x^{qq}"), "x^(qq)");
        assert_eq!(inline("y_{ab}"), "y_(ab)");
        assert_eq!(inline(r"e^{\alpha}"), "eᵅ");
        assert_eq!(inline(r"f'(x)"), "f′(x)");
    }

    #[test]
    fn fractions() {
        assert_eq!(inline(r"\frac{a}{b}"), "a/b");
        assert_eq!(inline(r"\frac{a+b}{c}"), "(a+b)/c");
        assert_eq!(inline(r"\frac{1}{x+1}"), "1/(x+1)");
        assert_eq!(inline(r"\frac{(a+b)}{(c)}"), "(a+b)/(c)");
        assert_eq!(inline(r"\frac12"), "1/2");
        assert_eq!(inline(r"\frac{\partial f}{\partial x}"), "∂f/∂x");
        assert_eq!(inline(r"\frac{\partial{f}}{\partial{x}}"), "∂f/∂x");
    }

    #[test]
    fn roots() {
        assert_eq!(inline(r"\sqrt{x}"), "√x");
        assert_eq!(inline(r"\sqrt{x+1}"), "√(x+1)");
        assert_eq!(inline(r"\sqrt[n]{x}"), "ⁿ√x");
        assert_eq!(inline(r"\sqrt[3]{8}"), "³√8");
        assert_eq!(inline(r"\sqrt[q]{8}"), "(q)√8");
    }

    #[test]
    fn unknown_commands_are_kept() {
        assert_eq!(inline(r"\foo x"), r"\foo x");
        assert_eq!(inline(r"\foo{n}{k}"), r"\foo{n}{k}");
        assert_eq!(inline(r"\unknown{a^2}"), r"\unknown{a²}");
    }

    #[test]
    fn accents_and_negation() {
        assert_eq!(inline(r"\hat{x}"), "x\u{302}");
        assert_eq!(inline(r"\vec{v}"), "v\u{20d7}");
        assert_eq!(inline(r"\overline{AB}"), "A\u{305}B\u{305}");
        assert_eq!(inline(r"\hat{xy}"), "hat(xy)");
        assert_eq!(inline(r"a \not= b"), "a ≠ b");
        assert_eq!(inline(r"a \not\in B"), "a ∉ B");
    }

    #[test]
    fn malformed_input_never_panics() {
        for src in [
            "",
            "\\",
            "{",
            "}",
            "x^",
            "x_",
            "^",
            "\\frac",
            "\\frac{a",
            "\\sqrt[",
            "\\sqrt[3",
            "\\begin",
            "\\begin{",
            "\\begin{matrix}",
            "\\begin{matrix} a & b \\\\",
            "\\end{matrix}",
            "\\left",
            "\\right",
            "\\text{",
            "\\mathbb{",
            "\\not",
            "a & b",
            "\\\\",
            "\\\\[",
            "x^{",
            "{{{}",
            "}}}{",
            "\\begin{array}",
            "\\tag{",
            "é^ü_ß",
            "\u{0}\u{1b}",
        ] {
            let _ = render_inline(src, false);
            let _ = render_display(src, false);
            let _ = render_inline(src, true);
            let _ = render_display(src, true);
        }
    }

    #[test]
    fn deep_nesting_is_capped() {
        let deep = format!("{}x{}", "{".repeat(10_000), "}".repeat(10_000));
        assert!(inline(&deep).contains('x'));
        let fracs = r"\frac{".repeat(10_000);
        let _ = inline(&fracs);
        let roots = format!("{}x{}", r"\sqrt{".repeat(5000), "}".repeat(5000));
        assert!(inline(&roots).contains('x'));
        let envs = format!(
            "{}a{}",
            r"\begin{pmatrix}".repeat(3000),
            r"\end{pmatrix}".repeat(3000)
        );
        assert!(display(&envs).concat().contains('a'));
        let scripts = "x^".repeat(10_000);
        let _ = inline(&scripts);
    }

    #[test]
    fn ascii_mode_shows_cleaned_source() {
        assert_eq!(
            render_inline(r"\left( \frac{a}{b} \right)\,dx \quad \alpha", true),
            r"( \frac{a}{b} ) dx \alpha"
        );
        assert_eq!(render_inline(r"\left. x \right|", true), "x |");
        assert_eq!(
            render_display("\\begin{cases}\n a & b \\\\\n c & d\n\\end{cases}", true),
            ["\\begin{cases}", "a & b \\\\", "c & d", "\\end{cases}"]
        );
    }

    // ── Real formulas, exact output ──

    #[test]
    fn quadratic_formula() {
        assert_eq!(
            inline(r"x = \frac{-b \pm \sqrt{b^2 - 4ac}}{2a}"),
            "x = (-b ± √(b² - 4ac))/2a"
        );
    }

    #[test]
    fn eulers_identity() {
        assert_eq!(inline(r"e^{i\pi} + 1 = 0"), "e^(iπ) + 1 = 0");
    }

    #[test]
    fn sum_with_limits() {
        assert_eq!(
            inline(r"\sum_{i=1}^{n} i = \frac{n(n+1)}{2}"),
            "∑ᵢ₌₁ⁿ i = (n(n+1))/2"
        );
    }

    #[test]
    fn integral() {
        assert_eq!(
            inline(r"\int_0^\infty e^{-x^2}\,dx = \frac{\sqrt{\pi}}{2}"),
            "∫₀^∞ e^(-x²) dx = (√π)/2"
        );
    }

    #[test]
    fn matrix_display() {
        assert_eq!(
            display(r"A = \begin{pmatrix} a & b \\ c & d \end{pmatrix}"),
            ["A = ⎛ a  b ⎞", "    ⎝ c  d ⎠"]
        );
        assert_eq!(
            display(r"\begin{bmatrix} 1 & 0 & 0 \\ 0 & 10 & 0 \\ 0 & 0 & 1 \end{bmatrix}"),
            ["⎡ 1  0   0 ⎤", "⎢ 0  10  0 ⎥", "⎣ 0  0   1 ⎦"]
        );
        assert_eq!(
            inline(r"\begin{pmatrix} a & b \\ c & d \end{pmatrix}"),
            "(a b; c d)"
        );
    }

    #[test]
    fn cases_display() {
        assert_eq!(
            display(
                r"|x| = \begin{cases} x & \text{if } x \geq 0 \\ -x & \text{otherwise} \end{cases}"
            ),
            ["|x| = ⎧ x   if x ≥ 0", "      ⎩ -x  otherwise"]
        );
        assert_eq!(
            display(r"\begin{cases} 1 \\ 2 \\ 3 \end{cases}"),
            ["⎧ 1", "⎨ 2", "⎩ 3"]
        );
    }

    #[test]
    fn aligned_display() {
        assert_eq!(
            display(r"\begin{aligned} f(x) &= (x+1)^2 \\ &= x^2 + 2x + 1 \end{aligned}"),
            ["f(x) = (x+1)²", "     = x² + 2x + 1"]
        );
        // Top-level rows and alignment without an environment.
        assert_eq!(display(r"a &= b \\ cc &= d"), [" a = b", "cc = d"]);
    }

    #[test]
    fn display_fractions_stack() {
        assert_eq!(
            display(r"x = \frac{a+b}{2}"),
            ["     a+b", "x = ─────", "      2"]
        );
        // Nested: the inner fraction stacks too.
        assert_eq!(
            display(r"\frac{\frac{1}{2}}{3}"),
            ["  1", " ───", "  2", "─────", "  3"]
        );
        // Inline, `\tfrac`, and inside a script: one line.
        assert_eq!(inline(r"x = \frac{a+b}{2}"), "x = (a+b)/2");
        assert_eq!(display(r"\tfrac{1}{2}"), ["1/2"]);
        assert_eq!(display(r"e^{\frac{a}{b}}"), ["e^(a/b)"]);
    }

    #[test]
    fn differential_operators_bind_to_their_operand() {
        assert_eq!(inline(r"\partial N + \partial \sigma"), "∂N + ∂σ");
        assert_eq!(inline(r"\nabla \mathbf{E}"), "∇𝐄");
        // Before another operator the space stays.
        assert_eq!(inline(r"\partial \nabla"), "∂ ∇");
    }

    #[test]
    fn display_limits_above_and_below() {
        assert_eq!(display(r"\sum_{i=1}^{n} i"), [" n", " ∑ i", "i=1"]);
        assert_eq!(
            display(r"\lim_{x\to 0}\frac{1}{x}"),
            ["     1", "lim ───", "x→0  x"]
        );
        // `\nolimits`, integrals and inline math keep scripts at the side.
        assert_eq!(display(r"\sum\nolimits_i x_i"), ["∑ᵢ xᵢ"]);
        assert_eq!(display(r"\int_0^1 x"), ["∫₀¹ x"]);
        assert_eq!(inline(r"\sum_{i=1}^{n} i"), "∑ᵢ₌₁ⁿ i");
    }

    #[test]
    fn binomials_and_braces() {
        assert_eq!(display(r"\binom{n}{k}"), ["⎛ n ⎞", "⎝ k ⎠"]);
        assert_eq!(inline(r"\binom{n}{k}"), "C(n,k)");
        assert_eq!(display(r"\underbrace{a+b}_{n}"), ["a+b", "╰┬╯", " n"]);
        assert_eq!(display(r"\overbrace{a+b}^{2}"), [" 2", "╭┴╮", "a+b"]);
        assert_eq!(inline(r"\underbrace{a+b}_{n}"), "(a+b)ₙ");
    }

    #[test]
    fn bold_letters() {
        assert_eq!(inline(r"\mathbf{F} = m\mathbf{a}"), "𝐅 = m𝐚");
        assert_eq!(inline(r"\boldsymbol{x_1}"), "𝐱₁");
        // No bold code point: kept.
        assert_eq!(inline(r"\mathbf{\nabla}"), "∇");
    }

    #[test]
    fn aligned_cells_align_on_the_fraction_bar() {
        assert_eq!(
            display(r"\begin{aligned} a &= \frac{1}{2} \\ b &= c \end{aligned}"),
            ["     1", "a = ───", "     2", "", "b = c"]
        );
    }

    /// Runs `f` on a 1 MB stack, as a worker thread would, and fails if it
    /// overflows or is far slower than linear work should be. The bound is
    /// loose on purpose: these inputs take ~0.1 s in a local debug build and
    /// ~0.5 s on a busy CI runner, while the quadratic behaviour this guards
    /// against took minutes.
    fn on_small_stack(name: &str, f: impl FnOnce() + Send + 'static) {
        let start = std::time::Instant::now();
        std::thread::Builder::new()
            .stack_size(1 << 20)
            .spawn(f)
            .unwrap()
            .join()
            .unwrap_or_else(|_| panic!("{name}: overflowed its stack"));
        let took = start.elapsed();
        assert!(took.as_secs() < 20, "{name}: took {took:?}");
    }

    // Regression: `\sqrt[` rendered its index with an unchecked recursive
    // call (and re-scanned the remainder at each level), so 200,000 of them
    // aborted the process. Every recursive entry is capped now.
    #[test]
    fn hostile_nesting_is_bounded_and_linear() {
        const N: usize = 200_000;
        let cases: Vec<(&str, String)> = vec![
            ("sqrt index", format!("{}x", r"\sqrt[".repeat(N))),
            ("frac", format!("{}x", r"\frac{".repeat(N))),
            ("superscript", format!("x{}y", "^{".repeat(N))),
            ("subscript", format!("x{}y", "_{".repeat(N))),
            ("matrix", format!("{}a", r"\begin{matrix}".repeat(N))),
            ("left", format!("{}x", r"\left(".repeat(N))),
            ("not", format!("{}x", r"\not".repeat(N))),
            (
                "mixed",
                format!(
                    "{}x",
                    r"\sqrt[\frac{^{_{\begin{pmatrix}\left(\mathbb{".repeat(N / 7)
                ),
            ),
        ];
        for (name, src) in cases {
            on_small_stack(name, move || {
                let a = render_inline(&src, false);
                let b = render_display(&src, false);
                assert!(!a.is_empty() && !b.is_empty());
            });
        }
    }

    #[test]
    fn display_roots_are_drawn() {
        assert_eq!(display(r"\sqrt{x+1}"), ["  ____", "╲╱ x+1"]);
        assert_eq!(display(r"\sqrt[3]{x+1}"), ["   ____", "³╲╱ x+1"]);
        assert_eq!(
            display(r"\sqrt{\frac{a}{b}}"),
            ["    ____", "   ╱  a", "  ╱  ───", "╲╱    b"]
        );
        // One token, and inline: `√` on one line.
        assert_eq!(display(r"\sqrt{2} + \sqrt{\pi}"), ["√2 + √π"]);
        assert_eq!(inline(r"\sqrt{x+1}"), "√(x+1)");
    }

    #[test]
    fn sqrt_index_still_renders() {
        assert_eq!(inline(r"\sqrt[3]{x}"), "³√x");
        assert_eq!(inline(r"\sqrt[n]{x}"), "ⁿ√x");
        assert_eq!(inline(r"\sqrt[{]}]{x}"), "(])√x");
        // Unclosed index: no panic.
        let _ = inline(r"\sqrt[3");
    }
}
