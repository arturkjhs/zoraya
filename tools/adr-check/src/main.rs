//! Checks the ADR files against the ADR-0001 invariants.
//!
//! Usage: `zoraya-adr-check DOCS_DIR` (normally `docs`). Reads
//! `DOCS_DIR/adr/` and resolves evidence against `DOCS_DIR/research/`.
//!
//! - INV-0001-1: every ADR has the template's `##` headings, in order.
//! - INV-0001-2: the status is one of the four values; accepted, rejected
//!   and superseded require a non-empty "Architect's decision" section.
//! - INV-0001-3: every citation (`DM-`, `GR-`, `CP-`, `X-` IDs and
//!   `<file>.md#<section>` references) resolves in the research files, and
//!   the `Evidence` line is exactly the set of citations used in the body.
//! - INV-0001-4: every invariant of an accepted ADR is in `invariants.md`
//!   with the same rule and checking method, and every row there comes from
//!   an ADR. A status is `checked: ...`, `replaced by INV-...`,
//!   `not yet applicable (stage N)` or `not yet applicable (until ...)`. An
//!   accepted ADR's invariant whose status is not `checked` is a warning
//!   (ADR-0001 rule 6). A `checked:` status names at least one command that
//!   exists: a `make` target of `Makefile` (or `ports/Makefile` with `-f`), a
//!   `cargo test -p` package, or a file under `tools/` or `.github/`.
//! - INV-0001-5: `index.md` lists every ADR file with its status.
//!
//! Exits 1 if any check fails, 0 otherwise (warnings do not fail).

mod markdown;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use markdown::{Citation, citations, headings, scan, section, slugs, split_table_row};

struct Adr {
    file: String,
    number: String,
    text: String,
    status: Option<String>,
}

#[derive(Default)]
struct Outcome {
    errors: Vec<String>,
    /// Invariants of accepted ADRs without a check, by status.
    unchecked: BTreeMap<String, Vec<String>>,
}

impl Outcome {
    fn error(&mut self, inv: &str, file: &str, msg: impl AsRef<str>) {
        self.errors.push(format!("{inv}: {file}: {}", msg.as_ref()));
    }
}

fn read(path: &Path) -> Result<String, String> {
    fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))
}

fn status_line(text: &str) -> Option<String> {
    text.lines()
        .find_map(|l| l.strip_prefix("- **Status:**"))
        .map(|s| s.trim().to_string())
}

fn load_adrs(adr_dir: &Path) -> Result<Vec<Adr>, String> {
    let mut adrs = Vec::new();
    let entries = fs::read_dir(adr_dir).map_err(|e| format!("{}: {e}", adr_dir.display()))?;
    for entry in entries {
        let name = entry.map_err(|e| e.to_string())?.file_name();
        let Some(name) = name.to_str() else { continue };
        let is_adr = name.len() > 8
            && name.as_bytes()[..4].iter().all(u8::is_ascii_digit)
            && name.as_bytes()[4] == b'-'
            && name.ends_with(".md");
        if !is_adr || name.starts_with("0000-") {
            continue;
        }
        let text = read(&adr_dir.join(name))?;
        adrs.push(Adr {
            file: name.to_string(),
            number: name[..4].to_string(),
            status: status_line(&text),
            text,
        });
    }
    adrs.sort_by(|a, b| a.file.cmp(&b.file));
    Ok(adrs)
}

fn h2(text: &str) -> Vec<String> {
    headings(text)
        .into_iter()
        .filter(|(level, _)| *level == 2)
        .map(|(_, t)| t)
        .collect()
}

/// INV-0001-1, plus the title line naming the right ADR number.
fn check_headings(adrs: &[Adr], template: &str, out: &mut Outcome) {
    let expected = h2(template);
    for adr in adrs {
        let title = format!("# ADR-{}:", adr.number);
        if !adr
            .text
            .lines()
            .next()
            .is_some_and(|l| l.starts_with(&title))
        {
            out.error(
                "INV-0001-1",
                &adr.file,
                format!("first line is not \"{title} ...\""),
            );
        }
        let got = h2(&adr.text);
        if got != expected {
            out.error(
                "INV-0001-1",
                &adr.file,
                format!("## headings {got:?} differ from the template's {expected:?}"),
            );
        }
    }
}

fn status_is_valid(s: &str) -> bool {
    match s {
        "proposed" | "accepted" | "rejected" => true,
        _ => s
            .strip_prefix("superseded by ADR-")
            .is_some_and(|n| n.len() == 4 && n.bytes().all(|b| b.is_ascii_digit())),
    }
}

/// INV-0001-2.
fn check_status(adrs: &[Adr], out: &mut Outcome) {
    for adr in adrs {
        let Some(status) = &adr.status else {
            out.error("INV-0001-2", &adr.file, "no \"- **Status:**\" line");
            continue;
        };
        let lines = adr
            .text
            .lines()
            .filter(|l| l.starts_with("- **Status:**"))
            .count();
        if lines > 1 {
            out.error("INV-0001-2", &adr.file, format!("{lines} Status lines"));
        }
        if !status_is_valid(status) {
            out.error(
                "INV-0001-2",
                &adr.file,
                format!("invalid status {status:?}"),
            );
            continue;
        }
        if let Some(n) = status.strip_prefix("superseded by ADR-")
            && !adrs.iter().any(|a| a.number == n)
        {
            out.error(
                "INV-0001-2",
                &adr.file,
                format!("superseded by ADR-{n}, which does not exist"),
            );
        }
        if status != "proposed" {
            let decision = section(&adr.text, "Architect's decision").unwrap_or_default();
            if decision.trim().is_empty() {
                out.error(
                    "INV-0001-2",
                    &adr.file,
                    format!("status {status} with an empty \"Architect's decision\" section"),
                );
            }
        }
    }
}

/// IDs defined in the research files: `### ID:` headings, and the first
/// cell of table rows (`| X-1 | ...`).
fn defined_ids(research: &Path) -> Result<BTreeSet<String>, String> {
    let mut ids = BTreeSet::new();
    let files: [(&str, &[&str]); 3] = [
        ("design-mistakes.md", &["DM"]),
        ("held-up-well.md", &["GR"]),
        ("candidate-principles.md", &["CP", "X"]),
    ];
    for (file, prefixes) in files {
        let text = read(&research.join(file))?;
        for line in text.lines() {
            let token = if let Some(rest) = line.strip_prefix("### ") {
                rest.split(':').next()
            } else if line.starts_with('|') {
                split_table_row(line).into_iter().next()
            } else {
                None
            };
            let Some(token) = token.map(str::trim) else {
                continue;
            };
            if let Some(Citation::Id(id)) = citations(token).into_iter().next()
                && id == token
                && prefixes.iter().any(|p| id.starts_with(&format!("{p}-")))
            {
                ids.insert(id);
            }
        }
    }
    Ok(ids)
}

fn evidence_line(text: &str) -> Option<(usize, &str)> {
    text.lines()
        .enumerate()
        .find_map(|(i, l)| l.strip_prefix("- **Evidence:**").map(|e| (i, e)))
}

/// INV-0001-3.
fn check_evidence(adrs: &[Adr], research: &Path, out: &mut Outcome) -> Result<(), String> {
    let ids = defined_ids(research)?;
    let mut slug_cache: BTreeMap<PathBuf, Option<BTreeSet<String>>> = BTreeMap::new();
    for adr in adrs {
        let Some((ev_index, ev)) = evidence_line(&adr.text) else {
            out.error("INV-0001-3", &adr.file, "no \"- **Evidence:**\" line");
            continue;
        };
        let body: String = adr
            .text
            .lines()
            .enumerate()
            .filter(|(i, _)| *i != ev_index)
            .map(|(_, l)| format!("{l}\n"))
            .collect();
        let mut used: BTreeSet<Citation> = BTreeSet::new();
        for (c, abbreviated) in scan(&body) {
            let Citation::Section { file, slug } = &c else {
                used.insert(c);
                continue;
            };
            let path = research.join(file);
            let anchors = slug_cache
                .entry(path.clone())
                .or_insert_with(|| fs::read_to_string(&path).ok().map(|t| slugs(&t)));
            match (abbreviated, anchors) {
                (true, Some(anchors)) => {
                    let matches: Vec<&String> = anchors
                        .iter()
                        .filter(|a| a.starts_with(slug.as_str()))
                        .collect();
                    match matches.as_slice() {
                        [one] => {
                            used.insert(Citation::Section {
                                file: file.clone(),
                                slug: (*one).clone(),
                            });
                        }
                        [] => out.error(
                            "INV-0001-3",
                            &adr.file,
                            format!("{file} has no section starting #{slug}"),
                        ),
                        _ => out.error(
                            "INV-0001-3",
                            &adr.file,
                            format!("{file}#{slug}\u{2026} matches several sections"),
                        ),
                    }
                }
                _ => {
                    used.insert(c);
                }
            }
        }
        let listed: BTreeSet<Citation> = citations(ev).into_iter().collect();
        // Every token on the Evidence line must be a citation.
        for item in ev.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            let c = citations(item);
            if c.len() != 1 || c[0].to_string() != item {
                out.error(
                    "INV-0001-3",
                    &adr.file,
                    format!("Evidence entry {item:?} is not a citation"),
                );
            }
        }
        for c in used.difference(&listed) {
            out.error(
                "INV-0001-3",
                &adr.file,
                format!("{c} is cited in the body but not on the Evidence line"),
            );
        }
        for c in listed.difference(&used) {
            out.error(
                "INV-0001-3",
                &adr.file,
                format!("{c} is on the Evidence line but not cited in the body"),
            );
        }
        for c in used.union(&listed) {
            match c {
                Citation::Id(id) => {
                    if !ids.contains(id) {
                        out.error(
                            "INV-0001-3",
                            &adr.file,
                            format!("{id} is not defined in the research files"),
                        );
                    }
                }
                Citation::DataFile(file) => {
                    if !research.join(file).is_file() {
                        out.error(
                            "INV-0001-3",
                            &adr.file,
                            format!("{file} does not exist in docs/research"),
                        );
                    }
                }
                Citation::Section { file, slug } => {
                    let path = research.join(file);
                    let slugs = slug_cache
                        .entry(path.clone())
                        .or_insert_with(|| fs::read_to_string(&path).ok().map(|t| slugs(&t)));
                    match slugs {
                        None => out.error(
                            "INV-0001-3",
                            &adr.file,
                            format!("{file} does not exist in docs/research"),
                        ),
                        Some(s) if !s.contains(slug) => {
                            out.error(
                                "INV-0001-3",
                                &adr.file,
                                format!("{file} has no section #{slug}"),
                            );
                        }
                        Some(_) => {}
                    }
                }
            }
        }
    }
    Ok(())
}

/// `- **INV-NNNN-k.** rule *Check:* method` items of an ADR.
fn adr_invariants(adr: &Adr) -> Vec<(String, String, String)> {
    let Some(sec) = section(&adr.text, "Invariants") else {
        return Vec::new();
    };
    let mut items: Vec<String> = Vec::new();
    for line in sec.lines() {
        if line.starts_with("- **INV-") {
            items.push(line.to_string());
        } else if let Some(last) = items.last_mut().filter(|_| !line.trim().is_empty()) {
            last.push(' ');
            last.push_str(line.trim());
        }
    }
    items
        .into_iter()
        .filter_map(|item| {
            let rest = item.strip_prefix("- **")?;
            let (id, rest) = rest.split_once(".**")?;
            let (rule, method) = rest.split_once("*Check:*")?;
            Some((id.to_string(), norm(rule), norm(method)))
        })
        .collect()
}

fn norm(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The things a repository's commands can name, found once per run: the
/// targets of `Makefile` and `ports/Makefile`, and the package names of every
/// `Cargo.toml`. `None` where the repository's root has no `Makefile` (the
/// unit tests' trees), so that nothing is checked there.
struct Commands {
    make: BTreeSet<String>,
    ports: BTreeSet<String>,
    packages: BTreeSet<String>,
    root: PathBuf,
}

fn make_targets(text: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for line in text.lines() {
        let head = if let Some(rest) = line.strip_prefix(".PHONY:") {
            rest
        } else if line.starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_' || c == '$') {
            match line.split_once(':') {
                Some((t, rest)) if !rest.starts_with('=') && !t.contains('=') => t,
                _ => continue,
            }
        } else {
            continue;
        };
        out.extend(head.split_whitespace().map(str::to_string));
    }
    out
}

fn collect_packages(dir: &Path, depth: usize, out: &mut BTreeSet<String>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let path = e.path();
        let name = e.file_name();
        let name = name.to_string_lossy();
        if path.is_dir() {
            if depth < 4 && !matches!(&*name, "target" | "build" | ".git" | "ports" | "docs") {
                collect_packages(&path, depth + 1, out);
            }
        } else if name == "Cargo.toml"
            && let Ok(text) = fs::read_to_string(&path)
            && let Some(line) = text.lines().find(|l| l.trim_start().starts_with("name"))
            && let Some(n) = line.split('"').nth(1)
        {
            out.insert(n.to_string());
        }
    }
}

impl Commands {
    fn load(docs: &Path) -> Option<Commands> {
        let root = docs.parent()?;
        let root = if root.as_os_str().is_empty() {
            Path::new(".")
        } else {
            root
        };
        let make = make_targets(&fs::read_to_string(root.join("Makefile")).ok()?);
        let ports = fs::read_to_string(root.join("ports/Makefile"))
            .map(|t| make_targets(&t))
            .unwrap_or_default();
        let mut packages = BTreeSet::new();
        collect_packages(root, 0, &mut packages);
        Some(Commands {
            make,
            ports,
            packages,
            root: root.to_path_buf(),
        })
    }

    /// What is wrong with a `checked:` status, if anything (V40): it must name
    /// at least one command that exists, and every `make TARGET`,
    /// `cargo test -p PACKAGE` and `tools/` or `.github/` path it names must.
    fn problem(&self, cmd: &str) -> Option<String> {
        // Commands are in backticks; prose around them may say "make" too.
        let spans: Vec<&str> = if cmd.contains('`') {
            cmd.split('`').skip(1).step_by(2).collect()
        } else {
            vec![cmd]
        };
        let words: Vec<&str> = spans
            .iter()
            .flat_map(|span| span.split(|c: char| c.is_whitespace() || ";,()".contains(c)))
            .filter(|w| !w.is_empty())
            .collect();
        let mut found = 0;
        let mut i = 0;
        while i < words.len() {
            match words[i] {
                "make" => {
                    let mut ports = false;
                    let mut j = i + 1;
                    while j < words.len() && words[j].starts_with('-') {
                        if words[j] == "-f" && words.get(j + 1) == Some(&"ports/Makefile") {
                            ports = true;
                            j += 1;
                        }
                        j += 1;
                    }
                    if let Some(t) = words.get(j).filter(|t| {
                        t.bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
                    }) {
                        found += 1;
                        let set = if ports { &self.ports } else { &self.make };
                        if !set.contains(*t) {
                            return Some(format!("`make {t}` is not a target"));
                        }
                    }
                }
                "cargo" if words.get(i + 1) == Some(&"test") => {
                    let mut j = i + 2;
                    while j < words.len() {
                        if words[j] == "-p" && j + 1 < words.len() {
                            found += 1;
                            if !self.packages.contains(words[j + 1]) {
                                return Some(format!("`{}` is not a package", words[j + 1]));
                            }
                            j += 2;
                        } else if words[j].starts_with('-') || words[j] == "usage" {
                            j += 1;
                        } else {
                            break;
                        }
                    }
                    if j == i + 2 {
                        found += 1;
                    }
                }
                w if (w.starts_with("tools/") || w.starts_with(".github/")) && !w.contains('*') => {
                    found += 1;
                    if !self.root.join(w.trim_end_matches('.')).exists() {
                        return Some(format!("`{w}` does not exist"));
                    }
                }
                _ => {}
            }
            i += 1;
        }
        (found == 0).then(|| {
            "it names no command (make TARGET, cargo test -p PACKAGE, tools/ or .github/ path)"
                .to_string()
        })
    }
}

fn status_is_known(s: &str) -> bool {
    if let Some(cmd) = s.strip_prefix("checked: ") {
        return !cmd.trim().is_empty();
    }
    if s.strip_prefix("replaced by INV-").is_some() {
        return true;
    }
    if let Some(rest) = s.strip_prefix("not yet applicable (until ") {
        return rest
            .strip_suffix(')')
            .is_some_and(|what| !what.trim().is_empty());
    }
    s.strip_prefix("not yet applicable (stage ")
        .and_then(|r| r.strip_suffix(')'))
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

/// INV-0001-4.
fn check_invariants(adrs: &[Adr], adr_dir: &Path, out: &mut Outcome) -> Result<(), String> {
    let commands = adr_dir.parent().and_then(Commands::load);
    let text = read(&adr_dir.join("invariants.md"))?;
    let mut rows: BTreeMap<String, (String, String, String)> = BTreeMap::new();
    for line in text.lines().filter(|l| l.starts_with("| INV-")) {
        let cells = split_table_row(line);
        if cells.len() != 5 {
            out.error(
                "INV-0001-4",
                "invariants.md",
                format!("row does not have 5 cells: {line}"),
            );
            continue;
        }
        rows.insert(
            cells[0].trim().to_string(),
            (
                norm(&cells[2].replace("\\|", "|")),
                norm(&cells[3].replace("\\|", "|")),
                norm(cells[4]),
            ),
        );
    }
    let mut from_adrs = BTreeSet::new();
    for adr in adrs {
        let accepted = adr.status.as_deref() == Some("accepted");
        let invs = adr_invariants(adr);
        if section(&adr.text, "Invariants").is_some_and(|s| s.contains("INV-")) && invs.is_empty() {
            out.error(
                "INV-0001-4",
                &adr.file,
                "Invariants section could not be parsed",
            );
        }
        for (id, rule, method) in invs {
            from_adrs.insert(id.clone());
            let Some((r_rule, r_method, r_status)) = rows.get(&id) else {
                if accepted {
                    out.error(
                        "INV-0001-4",
                        "invariants.md",
                        format!("{id} from {} is missing", adr.file),
                    );
                }
                continue;
            };
            if *r_rule != rule {
                out.error(
                    "INV-0001-4",
                    "invariants.md",
                    format!("{id}: rule differs from {}", adr.file),
                );
            }
            if *r_method != method || method.is_empty() {
                out.error(
                    "INV-0001-4",
                    "invariants.md",
                    format!("{id}: checking method differs from {}", adr.file),
                );
            }
            if !status_is_known(r_status) {
                out.error(
                    "INV-0001-4",
                    "invariants.md",
                    format!("{id}: status {r_status:?} is not \"checked: ...\", \"replaced by INV-...\", \"not yet applicable (stage N)\" or \"not yet applicable (until ...)\""),
                );
            } else if let Some(problem) = r_status
                .strip_prefix("checked: ")
                .and_then(|cmd| commands.as_ref()?.problem(cmd))
            {
                out.error(
                    "INV-0001-4",
                    "invariants.md",
                    format!("{id}: \"checked:\" must name a command that exists: {problem}"),
                );
            } else if accepted
                && !r_status.starts_with("checked")
                && !r_status.starts_with("replaced by ")
            {
                out.unchecked
                    .entry(r_status.clone())
                    .or_default()
                    .push(id.clone());
            }
        }
    }
    for id in rows.keys().filter(|id| !from_adrs.contains(*id)) {
        out.error(
            "INV-0001-4",
            "invariants.md",
            format!("{id} is not in any ADR"),
        );
    }
    check_counts(&text, out);
    Ok(())
}

/// INV-0071-1: the Counts table against the rows.
fn check_counts(text: &str, out: &mut Outcome) {
    let mut error = |message: String| out.error("INV-0071-1", "invariants.md", message);
    let mut per_adr: BTreeMap<String, u32> = BTreeMap::new();
    for line in text.lines().filter(|l| l.starts_with("| INV-")) {
        if let Some(adr) = line.get(6..10) {
            *per_adr.entry(adr.to_string()).or_default() += 1;
        }
    }
    let Some((_, counts)) = text.split_once("## Counts") else {
        error("no Counts section".to_string());
        return;
    };
    let number = |line: &str| {
        let cell = line.trim_end().trim_end_matches('|').rsplit('|').next()?;
        cell.trim().trim_matches('*').parse::<u32>().ok()
    };
    let mut total = None;
    let mut listed = BTreeSet::new();
    for line in counts.lines() {
        if line.starts_with("| **Total**") {
            total = number(line);
        } else if let Some(rest) = line.strip_prefix("| [ADR-")
            && let Some(adr) = rest.get(..4)
        {
            listed.insert(adr.to_string());
            let rows = per_adr.get(adr).copied().unwrap_or(0);
            if number(line) != Some(rows) {
                error(format!(
                    "ADR-{adr}: Counts says {:?}, {rows} rows",
                    number(line)
                ));
            }
        }
    }
    for adr in per_adr.keys().filter(|adr| !listed.contains(*adr)) {
        error(format!("ADR-{adr} has rows and no Counts row"));
    }
    let rows: u32 = per_adr.values().sum();
    if total != Some(rows) {
        error(format!("Total says {total:?}, {rows} rows"));
    }
}

/// INV-0001-5.
fn check_index(adrs: &[Adr], adr_dir: &Path, out: &mut Outcome) -> Result<(), String> {
    let text = read(&adr_dir.join("index.md"))?;
    let mut listed: BTreeMap<String, String> = BTreeMap::new();
    for line in text.lines().filter(|l| l.starts_with("| [ADR-")) {
        let cells = split_table_row(line);
        let file = cells
            .first()
            .and_then(|c| c.split_once("](").and_then(|(_, r)| r.split_once(')')))
            .map(|(f, _)| f.to_string());
        match (file, cells.get(2)) {
            (Some(f), Some(status)) => {
                listed.insert(f, status.trim().to_string());
            }
            _ => out.error("INV-0001-5", "index.md", format!("unreadable row: {line}")),
        }
    }
    for adr in adrs {
        match (listed.remove(&adr.file), &adr.status) {
            (None, _) => out.error(
                "INV-0001-5",
                "index.md",
                format!("{} is not listed", adr.file),
            ),
            (Some(s), Some(status)) if s != *status => out.error(
                "INV-0001-5",
                "index.md",
                format!("{} listed as {s:?}, file says {status:?}", adr.file),
            ),
            _ => {}
        }
    }
    for f in listed.keys() {
        out.error(
            "INV-0001-5",
            "index.md",
            format!("{f} is listed but does not exist"),
        );
    }
    Ok(())
}

fn run(docs: &Path) -> Result<Outcome, String> {
    let adr_dir = docs.join("adr");
    let research = docs.join("research");
    let template = read(&adr_dir.join("0000-template.md"))?;
    let adrs = load_adrs(&adr_dir)?;
    if adrs.is_empty() {
        return Err(format!("no ADR files in {}", adr_dir.display()));
    }
    let mut out = Outcome::default();
    check_headings(&adrs, &template, &mut out);
    check_status(&adrs, &mut out);
    check_evidence(&adrs, &research, &mut out)?;
    check_invariants(&adrs, &adr_dir, &mut out)?;
    check_index(&adrs, &adr_dir, &mut out)?;
    println!(
        "adr-check: {} ADRs checked against INV-0001-1..5",
        adrs.len()
    );
    Ok(out)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [docs] = args.as_slice() else {
        eprintln!("usage: zoraya-adr-check DOCS_DIR");
        return ExitCode::from(2);
    };
    match run(Path::new(docs)) {
        Err(e) => {
            eprintln!("adr-check: {e}");
            ExitCode::from(2)
        }
        Ok(out) => {
            let unchecked: usize = out.unchecked.values().map(Vec::len).sum();
            if unchecked > 0 {
                println!(
                    "adr-check: warning: {unchecked} invariants of accepted ADRs have no check yet (ADR-0001 rule 6):"
                );
                for (status, ids) in &out.unchecked {
                    println!("adr-check:   {status}: {}", ids.join(", "));
                }
            }
            for e in &out.errors {
                println!("adr-check: error: {e}");
            }
            println!(
                "adr-check: {} errors, {unchecked} warnings",
                out.errors.len()
            );
            if out.errors.is_empty() {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEMPLATE: &str = "# ADR-NNNN: <t>\n\n- **Status:** proposed\n- **Evidence:** DM-xxx\n\n\
        ## Context\n## Invariants\n## Architect's decision\n(empty)\n";

    fn adr(status: &str, evidence: &str, body: &str, decision: &str) -> String {
        format!(
            "# ADR-0001: T\n\n- **Status:** {status}\n- **Evidence:** {evidence}\n\n## Context\n{body}\n\
             ## Invariants\n- **INV-0001-1.** Rule one. *Check:* a script.\n\n## Architect's decision\n{decision}"
        )
    }

    const INVARIANTS: &str = "| ID | ADR | Rule | How | Status |\n|---|---|---|---|---|\n\
        | INV-0001-1 | [ADR-0001](0001-t.md) | Rule one. | a script. | checked: `make adr-check` |\n\
        \n## Counts\n\n| [ADR-0001](0001-t.md) T | 1 |\n| **Total** | **1** |\n";

    /// A docs tree in a fresh temporary directory; returns its path.
    fn tree(name: &str, adr_text: &str, invariants: &str, index_status: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("zoraya-adr-check-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("adr")).unwrap();
        fs::create_dir_all(root.join("research/task-02")).unwrap();
        fs::write(root.join("adr/0000-template.md"), TEMPLATE).unwrap();
        fs::write(root.join("adr/0001-t.md"), adr_text).unwrap();
        fs::write(root.join("adr/invariants.md"), invariants).unwrap();
        fs::write(
            root.join("adr/index.md"),
            format!("| ADR | Title | Status |\n|---|---|---|\n| [ADR-0001](0001-t.md) | T | {index_status} |\n"),
        )
        .unwrap();
        fs::write(
            root.join("research/design-mistakes.md"),
            "### DM-001: fork\n",
        )
        .unwrap();
        fs::write(root.join("research/held-up-well.md"), "### GR-010: abi\n").unwrap();
        fs::write(
            root.join("research/candidate-principles.md"),
            "### CP-13: abi\n| X-1 | a |\n",
        )
        .unwrap();
        fs::write(
            root.join("research/task-02/m.md"),
            "# M\n## Tiers, runs and sets\n",
        )
        .unwrap();
        fs::write(root.join("research/task-02/d.csv"), "a,b\n").unwrap();
        root
    }

    fn errors(root: &Path) -> Vec<String> {
        let out = run(root).unwrap();
        fs::remove_dir_all(root).unwrap();
        out.errors
    }

    const EV: &str = "DM-001, CP-13, X-1, task-02/m.md#tiers-runs-and-sets, task-02/d.csv";
    const BODY: &str =
        "DM-001 and CP-13/X-1 (`task-02/m.md#tiers-runs-and-sets`, `task-02/d.csv`).";

    #[test]
    fn consistent_tree_passes() {
        let t = tree(
            "ok",
            &adr("accepted", EV, BODY, "Accepted."),
            INVARIANTS,
            "accepted",
        );
        assert_eq!(errors(&t), Vec::<String>::new());
    }

    #[test]
    fn abbreviated_anchor_resolves() {
        let body = BODY.replace("#tiers-runs-and-sets", "#tiers-\u{2026}");
        let t = tree(
            "abbr",
            &adr("accepted", EV, &body, "Accepted."),
            INVARIANTS,
            "accepted",
        );
        assert_eq!(errors(&t), Vec::<String>::new());
    }

    fn expect(name: &str, adr_text: &str, invariants: &str, index_status: &str, inv: &str) {
        let errs = errors(&tree(name, adr_text, invariants, index_status));
        assert!(
            errs.iter().any(|e| e.starts_with(inv)),
            "{name}: expected {inv}, got {errs:?}"
        );
    }

    #[test]
    fn each_invariant_catches_its_violation() {
        let ok = adr("accepted", EV, BODY, "Accepted.");
        // INV-0001-1: a heading renamed.
        expect(
            "h",
            &ok.replace("## Context", "## Background"),
            INVARIANTS,
            "accepted",
            "INV-0001-1",
        );
        // INV-0001-2: accepted with an empty decision, and an unknown status.
        expect(
            "s1",
            &adr("accepted", EV, BODY, ""),
            INVARIANTS,
            "accepted",
            "INV-0001-2",
        );
        expect(
            "s2",
            &adr("approved", EV, BODY, ""),
            INVARIANTS,
            "approved",
            "INV-0001-2",
        );
        let sup = adr("superseded by ADR-0009", EV, BODY, "Superseded.");
        expect(
            "s3",
            &sup,
            INVARIANTS,
            "superseded by ADR-0009",
            "INV-0001-2",
        );
        let dup = ok.replace("- **Evidence:**", "- **Status:** proposed\n- **Evidence:**");
        expect("s4", &dup, INVARIANTS, "accepted", "INV-0001-2");
        // INV-0001-3: unknown ID, missing section, Evidence/body mismatch.
        expect(
            "e1",
            &adr(
                "accepted",
                &format!("{EV}, DM-002"),
                &format!("{BODY} DM-002"),
                "A.",
            ),
            INVARIANTS,
            "accepted",
            "INV-0001-3",
        );
        expect(
            "e2",
            &adr("accepted", EV, &format!("{BODY} task-02/m.md#nope"), "A."),
            INVARIANTS,
            "accepted",
            "INV-0001-3",
        );
        expect(
            "e3",
            &adr("accepted", EV, &format!("{BODY} GR-010"), "A."),
            INVARIANTS,
            "accepted",
            "INV-0001-3",
        );
        expect(
            "e4",
            &adr("accepted", &format!("{EV}, GR-010"), BODY, "A."),
            INVARIANTS,
            "accepted",
            "INV-0001-3",
        );
        // INV-0001-4: rule changed, row missing, bad status.
        expect(
            "i1",
            &ok,
            &INVARIANTS.replace("Rule one.", "Rule 1."),
            "accepted",
            "INV-0001-4",
        );
        expect(
            "i2",
            &ok,
            "| ID | ADR | Rule | How | Status |\n",
            "accepted",
            "INV-0001-4",
        );
        expect(
            "i3",
            &ok,
            &INVARIANTS.replace("checked: `make adr-check`", "done"),
            "accepted",
            "INV-0001-4",
        );
        // INV-0001-5: index status differs.
        expect("x", &ok, INVARIANTS, "proposed", "INV-0001-5");
    }

    #[test]
    fn unchecked_invariant_of_accepted_adr_warns() {
        let inv = INVARIANTS.replace("checked: `make adr-check`", "not yet applicable (stage 2)");
        let t = tree("w", &adr("accepted", EV, BODY, "A."), &inv, "accepted");
        let out = run(&t).unwrap();
        fs::remove_dir_all(&t).unwrap();
        assert!(out.errors.is_empty(), "{:?}", out.errors);
        assert_eq!(
            out.unchecked["not yet applicable (stage 2)"],
            ["INV-0001-1"]
        );
    }

    /// V40: a `checked:` status names a command that exists.
    #[test]
    fn checked_names_a_command_that_exists() {
        let base = std::env::temp_dir().join(format!("adr-check-cmd-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(base.join("docs")).unwrap();
        fs::create_dir_all(base.join("ports")).unwrap();
        fs::create_dir_all(base.join("tools")).unwrap();
        fs::create_dir_all(base.join("crates/x")).unwrap();
        fs::write(
            base.join("Makefile"),
            ".PHONY: a-target b\nOPT := 1\ntest-one: a-target\n\ttrue\n",
        )
        .unwrap();
        fs::write(base.join("ports/Makefile"), "libc:\n\ttrue\n").unwrap();
        fs::write(base.join("tools/s.sh"), "").unwrap();
        fs::write(
            base.join("crates/x/Cargo.toml"),
            "[package]\nname = \"zoraya-x\"\n",
        )
        .unwrap();
        let c = Commands::load(&base.join("docs")).unwrap();
        for ok in [
            "`make test-one`",
            "make a-target (the log); make b",
            "`make -f ports/Makefile libc`",
            "`cargo test -p zoraya-x --test t`",
            "`tools/s.sh`",
            "`make test-one` (prose: make on it, `make -j8`)",
        ] {
            assert_eq!(c.problem(ok), None, "{ok}");
        }
        for bad in [
            "by type and review; no command fails if it is broken",
            "`make test-gone`",
            "`make -f ports/Makefile test-one`",
            "`cargo test -p zoraya-gone`",
            "`tools/gone.sh`",
            "`make test-one`, `make test-gone`",
        ] {
            assert!(c.problem(bad).is_some(), "{bad}");
        }
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn until_status_is_known_and_warns() {
        let inv = INVARIANTS.replace(
            "checked: `make adr-check`",
            "not yet applicable (until exception channels)",
        );
        let t = tree("u", &adr("accepted", EV, BODY, "A."), &inv, "accepted");
        let out = run(&t).unwrap();
        fs::remove_dir_all(&t).unwrap();
        assert!(out.errors.is_empty(), "{:?}", out.errors);
        assert_eq!(
            out.unchecked["not yet applicable (until exception channels)"],
            ["INV-0001-1"]
        );
        assert!(!status_is_known("not yet applicable (until )"));
    }

    #[test]
    fn replaced_invariant_does_not_warn() {
        let inv = INVARIANTS.replace("checked: `make adr-check`", "replaced by INV-0002-1");
        let t = tree("r", &adr("accepted", EV, BODY, "A."), &inv, "accepted");
        let out = run(&t).unwrap();
        fs::remove_dir_all(&t).unwrap();
        assert!(out.errors.is_empty(), "{:?}", out.errors);
        assert!(out.unchecked.is_empty());
    }

    /// INV-0071-1.
    #[test]
    fn the_counts_must_match_the_rows() {
        let good = adr("accepted", EV, BODY, "Accepted.");
        expect_counts("c1", &good, &INVARIANTS.replace("T | 1 |", "T | 2 |"));
        expect_counts("c2", &good, &INVARIANTS.replace("**1**", "**2**"));
        expect_counts("c3", &good, &INVARIANTS.replace("## Counts", "## Other"));
        expect_counts(
            "c4",
            &good,
            &INVARIANTS.replace("| [ADR-0001](0001-t.md) T | 1 |\n", ""),
        );
    }

    fn expect_counts(name: &str, text: &str, invariants: &str) {
        let root = tree(name, text, invariants, "accepted");
        let found = errors(&root);
        assert!(found.iter().any(|e| e.contains("INV-0071-1")), "{found:?}");
    }

    #[test]
    fn repository_docs_pass() {
        let docs = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs");
        let out = run(&docs).unwrap();
        assert_eq!(out.errors, Vec::<String>::new());
    }
}
