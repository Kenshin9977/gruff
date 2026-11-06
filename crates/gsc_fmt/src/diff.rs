//! Unified diffs for `gruff format --diff`.

use similar::TextDiff;

/// A unified diff (three lines of context) from `old` to `new`, with `name` in both headers.
/// Empty when the texts are equal.
pub fn unified_diff(name: &str, old: &str, new: &str) -> String {
    if old == new {
        return String::new();
    }
    let diff = TextDiff::from_lines(old, new);
    let mut out = diff
        .unified_diff()
        .context_radius(3)
        .header(&format!("a/{name}"), &format!("b/{name}"))
        .to_string();
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn diffs_lines() {
        let d = super::unified_diff("x.gsc", "a\nb  \nc\n", "a\nb\nc\n");
        assert!(d.starts_with("--- a/x.gsc\n+++ b/x.gsc\n@@"), "{d}");
        assert!(d.contains("-b  \n+b\n"), "{d}");
        assert_eq!(super::unified_diff("x", "a", "a"), "");
    }
}
