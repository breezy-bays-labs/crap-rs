//! Coverage non-linearity. Isolates the `(1 - coverage)³` cubic term:
//! a moderately-complex function whose tests intentionally exercise
//! only the happy path. The CRAP score for `slugify` lands in the
//! High band because the cubic coverage penalty multiplies its
//! complexity squared.
//!
//! `truncate` and `pluralize` sit alongside as low-complexity
//! comparators — they share the file with `slugify`, but their full
//! coverage keeps them in the Low/Acceptable band. The contrast
//! demonstrates that the dominant factor for the High-band score is
//! the coverage gap, not just the line count.

/// Lowercase + space-to-hyphen + drop non-alphanumeric +
/// collapse-runs slug generator. The tests below only exercise the
/// straightforward ASCII case; the punctuation pass-through branch
/// and the explicit `Err` returns stay uncovered, so the reported
/// coverage stays low and the CRAP score climbs.
pub fn slugify(input: &str) -> Result<String, String> {
    if input.is_empty() {
        return Err("empty input".to_string());
    }

    let mut out = String::with_capacity(input.len());
    let mut last_was_hyphen = false;

    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() {
            for lower in ch.to_lowercase() {
                out.push(lower);
            }
            last_was_hyphen = false;
        } else if ch.is_whitespace() || ch == '-' || ch == '_' {
            if !last_was_hyphen && !out.is_empty() {
                out.push('-');
                last_was_hyphen = true;
            }
        } else {
            // Punctuation / symbol — uncovered when input is
            // ASCII-alphanumeric only. Treated as a soft separator so
            // a slug never carries arbitrary punctuation.
            if !last_was_hyphen && !out.is_empty() {
                out.push('-');
                last_was_hyphen = true;
            }
        }
    }

    let trimmed = out.trim_matches('-').to_string();
    if trimmed.is_empty() {
        return Err("all characters were stripped".to_string());
    }
    Ok(trimmed)
}

/// Truncate to `max_len` graphemes, appending an ellipsis when the
/// input was actually clipped. Fully exercised by the tests below.
pub fn truncate(input: &str, max_len: usize) -> String {
    if input.chars().count() <= max_len {
        return input.to_string();
    }
    let mut out: String = input.chars().take(max_len).collect();
    out.push('\u{2026}');
    out
}

/// Pluralize an English noun by appending "s" — or "es" for words
/// ending in s/x/z/ch/sh. Fully exercised by the tests below.
pub fn pluralize(noun: &str) -> String {
    if noun.is_empty() {
        return String::new();
    }
    if noun.ends_with('s')
        || noun.ends_with('x')
        || noun.ends_with('z')
        || noun.ends_with("ch")
        || noun.ends_with("sh")
    {
        format!("{noun}es")
    } else {
        format!("{noun}s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── slugify: only the happy path is covered ──
    //
    // Deliberately uncovered branches:
    //   * empty-input Err
    //   * punctuation/symbol pass-through
    //   * all-stripped Err
    //
    // The CRAP target for slugify is the High band; if a contributor
    // adds tests covering the missing branches without rebanding the
    // module, the README heatmap will drift. Treat new slugify
    // coverage as a heatmap update, not an unconditional improvement.

    #[test]
    fn slugify_lowercases_ascii() {
        assert_eq!(slugify("Hello World").unwrap(), "hello-world");
    }

    #[test]
    fn slugify_collapses_whitespace_runs() {
        assert_eq!(slugify("hello   world").unwrap(), "hello-world");
    }

    // ── truncate: fully covered ──

    #[test]
    fn truncate_returns_short_string_unchanged() {
        assert_eq!(truncate("hi", 10), "hi");
    }

    #[test]
    fn truncate_clips_long_string_with_ellipsis() {
        assert_eq!(truncate("hello world", 5), "hello\u{2026}");
    }

    #[test]
    fn truncate_returns_exact_length_unchanged() {
        assert_eq!(truncate("hello", 5), "hello");
    }

    // ── pluralize: fully covered ──

    #[test]
    fn pluralize_empty_returns_empty() {
        assert_eq!(pluralize(""), "");
    }

    #[test]
    fn pluralize_regular_appends_s() {
        assert_eq!(pluralize("cat"), "cats");
    }

    #[test]
    fn pluralize_ending_in_s_appends_es() {
        assert_eq!(pluralize("bus"), "buses");
    }

    #[test]
    fn pluralize_ending_in_x_appends_es() {
        assert_eq!(pluralize("box"), "boxes");
    }

    #[test]
    fn pluralize_ending_in_z_appends_es() {
        assert_eq!(pluralize("buzz"), "buzzes");
    }

    #[test]
    fn pluralize_ending_in_ch_appends_es() {
        assert_eq!(pluralize("church"), "churches");
    }

    #[test]
    fn pluralize_ending_in_sh_appends_es() {
        assert_eq!(pluralize("dish"), "dishes");
    }
}
