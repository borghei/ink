//! Unicode superscript and subscript forms, shared by markdown `^x^` /
//! `~x~` and math `x^2` / `x_i`.
//!
//! Unicode has raised and lowered forms for the digits, `+ - = ( )` and most
//! (not all) Latin letters. A run converts only when every character has a
//! form — a half-raised `xⁿ+1` reads wrong — otherwise the caller falls back
//! to `^(…)` / `_(…)`, which is also the only form in ASCII mode.

fn superscript_char(c: char) -> Option<char> {
    Some(match c {
        '0' => '⁰',
        '1' => '¹',
        '2' => '²',
        '3' => '³',
        '4' => '⁴',
        '5' => '⁵',
        '6' => '⁶',
        '7' => '⁷',
        '8' => '⁸',
        '9' => '⁹',
        '+' => '⁺',
        '-' | '−' => '⁻',
        '=' => '⁼',
        '(' => '⁽',
        ')' => '⁾',
        'a' => 'ᵃ',
        'b' => 'ᵇ',
        'c' => 'ᶜ',
        'd' => 'ᵈ',
        'e' => 'ᵉ',
        'f' => 'ᶠ',
        'g' => 'ᵍ',
        'h' => 'ʰ',
        'i' => 'ⁱ',
        'j' => 'ʲ',
        'k' => 'ᵏ',
        'l' => 'ˡ',
        'm' => 'ᵐ',
        'n' => 'ⁿ',
        'o' => 'ᵒ',
        'p' => 'ᵖ',
        'r' => 'ʳ',
        's' => 'ˢ',
        't' => 'ᵗ',
        'u' => 'ᵘ',
        'v' => 'ᵛ',
        'w' => 'ʷ',
        'x' => 'ˣ',
        'y' => 'ʸ',
        'z' => 'ᶻ',
        'A' => 'ᴬ',
        'B' => 'ᴮ',
        'D' => 'ᴰ',
        'E' => 'ᴱ',
        'G' => 'ᴳ',
        'H' => 'ᴴ',
        'I' => 'ᴵ',
        'J' => 'ᴶ',
        'K' => 'ᴷ',
        'L' => 'ᴸ',
        'M' => 'ᴹ',
        'N' => 'ᴺ',
        'O' => 'ᴼ',
        'P' => 'ᴾ',
        'R' => 'ᴿ',
        'T' => 'ᵀ',
        'U' => 'ᵁ',
        'V' => 'ⱽ',
        'W' => 'ᵂ',
        'α' => 'ᵅ',
        'β' => 'ᵝ',
        'γ' => 'ᵞ',
        'δ' => 'ᵟ',
        'ε' => 'ᵋ',
        'θ' => 'ᶿ',
        'φ' => 'ᵠ',
        'χ' => 'ᵡ',
        _ => return None,
    })
}

fn subscript_char(c: char) -> Option<char> {
    Some(match c {
        '0' => '₀',
        '1' => '₁',
        '2' => '₂',
        '3' => '₃',
        '4' => '₄',
        '5' => '₅',
        '6' => '₆',
        '7' => '₇',
        '8' => '₈',
        '9' => '₉',
        '+' => '₊',
        '-' | '−' => '₋',
        '=' => '₌',
        '(' => '₍',
        ')' => '₎',
        'a' => 'ₐ',
        'e' => 'ₑ',
        'h' => 'ₕ',
        'i' => 'ᵢ',
        'j' => 'ⱼ',
        'k' => 'ₖ',
        'l' => 'ₗ',
        'm' => 'ₘ',
        'n' => 'ₙ',
        'o' => 'ₒ',
        'p' => 'ₚ',
        'r' => 'ᵣ',
        's' => 'ₛ',
        't' => 'ₜ',
        'u' => 'ᵤ',
        'v' => 'ᵥ',
        'x' => 'ₓ',
        'β' => 'ᵦ',
        'γ' => 'ᵧ',
        'ρ' => 'ᵨ',
        'φ' => 'ᵩ',
        'χ' => 'ᵪ',
        _ => return None,
    })
}

fn convert(text: &str, map: fn(char) -> Option<char>) -> Option<String> {
    if text.is_empty() {
        return None;
    }
    text.chars().map(map).collect()
}

/// `text` in Unicode superscript, when every character has a form.
pub fn to_superscript(text: &str) -> Option<String> {
    convert(text, superscript_char)
}

/// `text` in Unicode subscript, when every character has a form.
pub fn to_subscript(text: &str) -> Option<String> {
    convert(text, subscript_char)
}

/// `text` raised: Unicode where possible (never in ASCII mode), else `^x`
/// for a single character and `^(text)` otherwise when `bare_single`, or
/// always `^(text)`.
pub fn superscript(text: &str, ascii: bool, bare_single: bool) -> String {
    match (!ascii).then(|| to_superscript(text)).flatten() {
        Some(s) => s,
        None => fallback('^', text, bare_single),
    }
}

/// `text` lowered; see [`superscript`].
pub fn subscript(text: &str, ascii: bool, bare_single: bool) -> String {
    match (!ascii).then(|| to_subscript(text)).flatten() {
        Some(s) => s,
        None => fallback('_', text, bare_single),
    }
}

fn fallback(mark: char, text: &str, bare_single: bool) -> String {
    if bare_single && text.chars().count() == 1 {
        format!("{mark}{text}")
    } else {
        format!("{mark}({text})")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_only_whole_runs() {
        assert_eq!(to_superscript("2").as_deref(), Some("²"));
        assert_eq!(to_superscript("n+1").as_deref(), Some("ⁿ⁺¹"));
        assert_eq!(to_superscript("q"), None, "no superscript q");
        assert_eq!(to_superscript("nq"), None);
        assert_eq!(to_subscript("ij").as_deref(), Some("ᵢⱼ"));
        assert_eq!(to_subscript("b"), None, "no subscript b");
        assert_eq!(to_subscript(""), None);
    }

    #[test]
    fn falls_back_to_caret_and_underscore() {
        assert_eq!(superscript("2", false, false), "²");
        assert_eq!(superscript("2", true, false), "^(2)");
        assert_eq!(superscript("2", true, true), "^2");
        assert_eq!(superscript("hello world", false, false), "^(hello world)");
        assert_eq!(subscript("b", false, true), "_b");
        assert_eq!(subscript("2", false, false), "₂");
    }
}
