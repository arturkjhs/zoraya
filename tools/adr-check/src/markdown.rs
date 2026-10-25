//! The little Markdown handling the checks need: headings outside code
//! fences, sections, GitHub heading slugs, table cells and citation tokens.

use std::collections::BTreeSet;
use std::fmt;

/// `(level, text)` of every ATX heading outside fenced code blocks.
pub fn headings(text: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut fence: Option<&str> = None;
    for line in text.lines() {
        let t = line.trim_start();
        if let Some(f) = fence {
            if t.starts_with(f) {
                fence = None;
            }
            continue;
        }
        if t.starts_with("```") || t.starts_with("~~~") {
            fence = Some(&t[..3]);
            continue;
        }
        let level = line.bytes().take_while(|&b| b == b'#').count();
        if (1..=6).contains(&level) && line[level..].starts_with(' ') {
            out.push((
                level,
                line[level..]
                    .trim()
                    .trim_end_matches('#')
                    .trim()
                    .to_string(),
            ));
        }
    }
    out
}

/// The body of the `## name` section (up to the next `## ` heading).
pub fn section(text: &str, name: &str) -> Option<String> {
    let mut lines = text.lines();
    lines.find(|l| l.strip_prefix("## ").is_some_and(|h| h.trim() == name))?;
    let body: Vec<&str> = lines.take_while(|l| !l.starts_with("## ")).collect();
    Some(body.join("\n"))
}

/// GitHub's heading anchor: lowercase, keep letters, digits, `_`, `-` and
/// spaces, spaces become `-`.
pub fn slugify(heading: &str) -> String {
    heading
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-' || *c == ' ')
        .map(|c| if c == ' ' { '-' } else { c })
        .collect()
}

/// Every anchor in a file, with GitHub's `-1`, `-2` suffixes for repeats.
pub fn slugs(text: &str) -> BTreeSet<String> {
    let mut seen = BTreeSet::new();
    for (_, h) in headings(text) {
        let base = slugify(&h);
        let mut slug = base.clone();
        let mut n = 0;
        while seen.contains(&slug) {
            n += 1;
            slug = format!("{base}-{n}");
        }
        seen.insert(slug);
    }
    seen
}

/// Cells of a `| a | b |` row; `\|` does not split.
pub fn split_table_row(line: &str) -> Vec<&str> {
    let inner = line.trim().trim_start_matches('|');
    let inner = inner.strip_suffix('|').unwrap_or(inner);
    let mut cells = Vec::new();
    let mut start = 0;
    let bytes = inner.as_bytes();
    for i in 0..bytes.len() {
        if bytes[i] == b'|' && (i == 0 || bytes[i - 1] != b'\\') {
            cells.push(inner[start..i].trim());
            start = i + 1;
        }
    }
    cells.push(inner[start..].trim());
    cells
}

/// An evidence citation.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Citation {
    /// `DM-001`, `GR-010`, `CP-13`, `X-4`.
    Id(String),
    /// `task-02/method.md#tiers-runs-and-sets`.
    Section { file: String, slug: String },
    /// A measured data file cited whole: `task-02/libc-surface.csv`.
    DataFile(String),
}

impl fmt::Display for Citation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Citation::Id(id) => f.write_str(id),
            Citation::Section { file, slug } => write!(f, "{file}#{slug}"),
            Citation::DataFile(file) => f.write_str(file),
        }
    }
}

/// ID prefixes and their digit counts.
const ID_KINDS: [(&str, usize, usize); 4] =
    [("DM-", 3, 3), ("GR-", 3, 3), ("CP-", 2, 2), ("X-", 1, 2)];

fn is_path_char(c: u8) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.' | b'/')
}

/// Every citation in `text`, in order of appearance (with repeats).
pub fn citations(text: &str) -> Vec<Citation> {
    scan(text).into_iter().map(|(c, _)| c).collect()
}

/// Like [`citations`], also telling whether a section reference is
/// abbreviated: `file.md#x-1-…` (an ellipsis right after the slug) stands
/// for the one anchor in that file that starts with `x-1-`.
pub fn scan(text: &str) -> Vec<(Citation, bool)> {
    let b = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        // `CP-13/X-10` cites both IDs, so `/` may precede an ID.
        let prev_ok =
            i == 0 || !(b[i - 1].is_ascii_alphanumeric() || matches!(b[i - 1], b'_' | b'-' | b'.'));
        if prev_ok && let Some((id, len)) = id_at(b, i) {
            out.push((Citation::Id(id), false));
            i += len;
            continue;
        }
        if let Some(len) = data_file_at(text, i) {
            out.push((Citation::DataFile(text[i..i + len].to_string()), false));
            i += len;
            continue;
        }
        if b[i..].starts_with(b".md#") {
            let mut start = i;
            while start > 0 && is_path_char(b[start - 1]) {
                start -= 1;
            }
            let slug_start = i + 4;
            let mut end = slug_start;
            while end < b.len() && (b[end].is_ascii_alphanumeric() || matches!(b[end], b'-' | b'_'))
            {
                end += 1;
            }
            if start < i && end > slug_start {
                let abbreviated = text[end..].starts_with('\u{2026}');
                out.push((
                    Citation::Section {
                        file: text[start..i + 3].to_string(),
                        slug: text[slug_start..end].to_string(),
                    },
                    abbreviated,
                ));
                i = end;
                continue;
            }
        }
        i += 1;
    }
    out
}

/// `task-02/NAME.csv` or `task-02b/NAME.tsv` (data files, which have no
/// sections) starting at `i`.
fn data_file_at(text: &str, i: usize) -> Option<usize> {
    let b = text.as_bytes();
    if b[i] != b't' || (i > 0 && is_path_char(b[i - 1])) {
        return None;
    }
    let rest = &text[i..];
    let after_dir = rest
        .strip_prefix("task-02/")
        .or_else(|| rest.strip_prefix("task-02b/"))?;
    let name_len = after_dir
        .bytes()
        .take_while(|&c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.'))
        .count();
    let name = after_dir[..name_len].trim_end_matches('.');
    if name.ends_with(".csv") || name.ends_with(".tsv") {
        Some(rest.len() - after_dir.len() + name.len())
    } else {
        None
    }
}

fn id_at(b: &[u8], i: usize) -> Option<(String, usize)> {
    for (prefix, min, max) in ID_KINDS {
        if !b[i..].starts_with(prefix.as_bytes()) {
            continue;
        }
        let digits = b[i + prefix.len()..]
            .iter()
            .take_while(|c| c.is_ascii_digit())
            .count();
        let end = i + prefix.len() + digits;
        let next_ok = end == b.len() || !(b[end].is_ascii_alphanumeric() || b[end] == b'_');
        if (min..=max).contains(&digits) && next_ok {
            return Some((String::from_utf8_lossy(&b[i..end]).into_owned(), end - i));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs_match_github() {
        assert_eq!(
            slugify("Fuchsia / Zircon (incl. Starnix)"),
            "fuchsia--zircon-incl-starnix"
        );
        assert_eq!(
            slugify("W10: background jobs, `wait`, concurrent pipelines (run W10-parallel-r1)"),
            "w10-background-jobs-wait-concurrent-pipelines-run-w10-parallel-r1"
        );
        assert_eq!(
            slugify("X-1 capability-only access (CP-01, CP-02)"),
            "x-1-capability-only-access-cp-01-cp-02"
        );
        let s = slugs("# A\n## A\n```\n## A\n```\n### A\n");
        assert_eq!(
            s,
            ["a", "a-1", "a-2"].iter().map(|s| s.to_string()).collect()
        );
    }

    #[test]
    fn citations_found() {
        let c = citations(
            "See DM-001 and CP-13's point, X-4, not INV-0001-1, not DM-xxx, not X-123; \
             (`task-02/method.md#tiers-runs-and-sets`) and alternatives-postmortem.md#qnx.",
        );
        let s: Vec<String> = c.iter().map(|c| c.to_string()).collect();
        assert_eq!(
            s,
            [
                "DM-001",
                "CP-13",
                "X-4",
                "task-02/method.md#tiers-runs-and-sets",
                "alternatives-postmortem.md#qnx"
            ]
        );
        assert!(
            citations("task-02/x.md#x-15-env")
                .iter()
                .all(|c| matches!(c, Citation::Section { .. }))
        );
        assert!(citations("a.md without section").is_empty());
        assert_eq!(
            citations("CP-13/X-10"),
            [Citation::Id("CP-13".into()), Citation::Id("X-10".into())]
        );
        assert_eq!(
            citations("(`task-02/libc-surface.csv`). task-02/method.md"),
            [Citation::DataFile("task-02/libc-surface.csv".into())]
        );
        let s = scan("`task-02/c.md#x-1-\u{2026}`, d.md#y");
        assert_eq!(s.iter().map(|(_, a)| *a).collect::<Vec<_>>(), [true, false]);
    }

    #[test]
    fn table_rows() {
        assert_eq!(
            split_table_row("| a | b \\| c | d |"),
            ["a", "b \\| c", "d"]
        );
    }

    #[test]
    fn sections_and_headings() {
        let t = "# T\n## One\nx\n```\n## Not\n```\n## Two\ny\n";
        assert_eq!(section(t, "One").unwrap(), "x\n```");
        assert_eq!(headings(t).len(), 3);
    }
}
