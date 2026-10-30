//! The layout of the test targets. `Cargo.toml` sets `autotests = false`, so a
//! file under `tests/` is built only when a `[[test]]` entry names it or a
//! `[[test]]` root mounts it. These tests fail on a test file nothing builds,
//! on one built twice, and on a standalone binary with no stated reason.
//!
//! A mount is a `#[path = "<stem>.rs"]` line with no `/`, followed by
//! `mod <stem>;`. A suite is a `[[test]]` whose name starts `suite_`.
//!
//! The library has no unit tests (`[lib] test = false`), so
//! `no_library_source_holds_a_unit_test` fails on a `#[cfg(test)]` or
//! `#[test]` line anywhere under `src/`.
//!
//! `every_serial_suite_test_takes_the_lock_first` fails on a test in a file
//! `suite_serial` mounts whose first statement is not `let _serial = serial();`.
//!
//! `no_default_test_asserts_a_duration` fails on an assertion over a clock
//! reading in default-run code: a wall-clock verdict belongs in a `mod bench`
//! twin `#[ignore = "make cancel-bench: …"]`. A call to a
//! `tests/common/` helper whose verdict is a duration (`CLOCK_VERDICTS`:
//! `assert_pin`, `assert_ramp`) counts as one too.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

const SUITE_PREFIX: &str = "suite_";
const ISOLATE_MARK: &str = "# isolate:";
const SERIAL_SUITE: &str = "suite_serial";
const SERIAL_LOCK: &str = "let _serial = serial();";

/// One `[[test]]` entry of `Cargo.toml`.
struct Target {
    name: String,
    path: Option<String>,
    /// The text after `# isolate:` on the `name` line.
    isolate: String,
}

impl Target {
    /// The root file: the entry's `path`, else Cargo's default.
    fn root(&self) -> PathBuf {
        match &self.path {
            Some(path) => manifest_dir().join(path),
            None => tests_dir().join(format!("{}.rs", self.name)),
        }
    }
}

fn manifest_dir() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn tests_dir() -> PathBuf {
    manifest_dir().join("tests")
}

/// The text between the first pair of double quotes.
fn quoted(text: &str) -> Option<&str> {
    let (_, rest) = text.split_once('"')?;
    let (value, _) = rest.split_once('"')?;
    Some(value)
}

fn test_targets() -> Vec<Target> {
    let manifest = fs::read_to_string(manifest_dir().join("Cargo.toml")).expect("Cargo.toml reads");
    let mut targets: Vec<Target> = Vec::new();
    let mut in_test = false;
    for line in manifest.lines().map(str::trim) {
        if line.starts_with('[') {
            in_test = line == "[[test]]";
            if in_test {
                targets.push(Target {
                    name: String::new(),
                    path: None,
                    isolate: String::new(),
                });
            }
            continue;
        }
        if !in_test {
            continue;
        }
        let (Some(target), Some((key, value))) = (targets.last_mut(), line.split_once('=')) else {
            continue;
        };
        match key.trim() {
            "name" => {
                target.name = quoted(value).expect("a quoted test name").to_string();
                if let Some((_, reason)) = value.split_once(ISOLATE_MARK) {
                    target.isolate = reason.trim().to_string();
                }
            }
            "path" => target.path = quoted(value).map(str::to_string),
            _ => {}
        }
    }
    targets
}

/// The stems one root file mounts, in file order.
fn mounted_stems(root: &Path) -> Vec<String> {
    let Ok(source) = fs::read_to_string(root) else {
        return Vec::new();
    };
    let lines: Vec<&str> = source.lines().map(str::trim).collect();
    let mut stems = Vec::new();
    for pair in lines.windows(2) {
        let Some(file) = pair[0]
            .strip_prefix("#[path = \"")
            .and_then(|rest| rest.strip_suffix("\"]"))
        else {
            continue;
        };
        let Some(stem) = file.strip_suffix(".rs") else {
            continue;
        };
        if !file.contains('/') && pair[1] == format!("mod {stem};") {
            stems.push(stem.to_string());
        }
    }
    stems
}

/// The stems of the `tests/*.rs` files that hold a `#[test]` line.
fn test_file_stems() -> BTreeSet<String> {
    let mut stems = BTreeSet::new();
    for entry in fs::read_dir(tests_dir()).expect("tests/ lists") {
        let path = entry.expect("a tests/ entry").path();
        if !path.is_file() || path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let source = fs::read_to_string(&path).expect("a test file reads");
        if source
            .lines()
            .any(|line| line.trim_start().starts_with("#[test]"))
        {
            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .expect("a UTF-8 file name");
            stems.insert(stem.to_string());
        }
    }
    stems
}

#[test]
fn every_test_file_is_built_exactly_once() {
    let targets = test_targets();
    let tests = tests_dir();
    // The stems that are `[[test]]` roots directly under `tests/`.
    let roots: BTreeSet<String> = targets
        .iter()
        .map(Target::root)
        .filter(|root| root.parent() == Some(tests.as_path()))
        .filter_map(|root| Some(root.file_stem()?.to_str()?.to_string()))
        .collect();
    let mut mounts: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for target in &targets {
        for stem in mounted_stems(&target.root()) {
            mounts.entry(stem).or_default().push(target.name.clone());
        }
    }

    let mut problems = Vec::new();
    for stem in test_file_stems() {
        if !roots.contains(&stem) && !mounts.contains_key(&stem) {
            problems.push(format!(
                "tests/{stem}.rs is not built: mount it in its family's tests/suite_*.rs, or give it a [[test]] entry with an `# isolate:` reason"
            ));
        }
    }
    for (stem, by) in &mounts {
        if by.len() > 1 {
            problems.push(format!(
                "tests/{stem}.rs is mounted {} times: {}",
                by.len(),
                by.join(", ")
            ));
        }
        if roots.contains(stem) {
            problems.push(format!(
                "tests/{stem}.rs is both a [[test]] root and mounted by {}",
                by.join(", ")
            ));
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn no_mounted_file_declares_a_shared_module() {
    let mut problems = Vec::new();
    for target in test_targets() {
        if !target.name.starts_with(SUITE_PREFIX) {
            continue;
        }
        let suite = &target.name;
        for stem in mounted_stems(&target.root()) {
            let source = fs::read_to_string(tests_dir().join(format!("{stem}.rs")))
                .expect("a mounted file reads");
            for line in source.lines().map(str::trim) {
                for module in ["common", "live_filter_support"] {
                    if line == format!("mod {module};") || line == format!("pub mod {module};") {
                        problems.push(format!(
                            "tests/{stem}.rs (mounted by {suite}) declares its own `mod {module};`: the suite root declares it once; use `use crate::{module};`"
                        ));
                    }
                }
                if line.starts_with("#[path = \"common/") {
                    let path = quoted(line).unwrap_or(line);
                    problems.push(format!(
                        "tests/{stem}.rs (mounted by {suite}) mounts {path} itself: import it from the suite root's mount (`use crate::…`; see core/tests/{suite}.rs for its name)"
                    ));
                }
            }
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn every_test_target_has_its_root_file() {
    let targets = test_targets();
    assert!(!targets.is_empty(), "Cargo.toml lists no [[test]] entry");
    let missing: Vec<String> = targets
        .iter()
        .filter(|target| !target.root().is_file())
        .map(|target| {
            format!(
                "[[test]] {} has no root file {}",
                target.name,
                target.root().display()
            )
        })
        .collect();
    assert!(missing.is_empty(), "{}", missing.join("\n"));
}

#[test]
fn every_standalone_target_states_its_isolation_reason() {
    let unexplained: Vec<String> = test_targets()
        .iter()
        .filter(|target| !target.name.starts_with(SUITE_PREFIX) && target.isolate.is_empty())
        .map(|target| {
            format!(
                "[[test]] {} is a standalone binary with no `# isolate: <reason>` comment on its name line",
                target.name
            )
        })
        .collect();
    assert!(unexplained.is_empty(), "{}", unexplained.join("\n"));
}

#[test]
fn every_serial_suite_test_takes_the_lock_first() {
    let targets = test_targets();
    let Some(target) = targets.iter().find(|target| target.name == SERIAL_SUITE) else {
        panic!("Cargo.toml lists no [[test]] {SERIAL_SUITE}");
    };
    let mut problems = Vec::new();
    for stem in mounted_stems(&target.root()) {
        let source = fs::read_to_string(tests_dir().join(format!("{stem}.rs")))
            .expect("a mounted file reads");
        let lines: Vec<&str> = source.lines().map(str::trim).collect();
        for (index, line) in lines.iter().enumerate() {
            if *line != "#[test]" {
                continue;
            }
            let first = lines[index + 1..]
                .iter()
                .position(|line| line.ends_with('{'))
                .and_then(|offset| lines.get(index + offset + 2));
            if first != Some(&SERIAL_LOCK) {
                problems.push(format!(
                    "tests/{stem}.rs:{} (mounted by {SERIAL_SUITE}): the test's first statement is not `{SERIAL_LOCK}`: every test in this suite takes the shared lock first, so it drops last",
                    index + 1
                ));
            }
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// Every `.rs` file under `dir`, recursively.
fn rust_sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("a source directory lists") {
        let path = entry.expect("a source entry").path();
        if path.is_dir() {
            rust_sources(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

#[test]
fn no_library_source_holds_a_unit_test() {
    let mut sources = Vec::new();
    rust_sources(&manifest_dir().join("src"), &mut sources);
    sources.sort();
    let mut found = Vec::new();
    for path in &sources {
        let source = fs::read_to_string(path).expect("a source file reads");
        for (index, line) in source.lines().enumerate() {
            let line = line.trim();
            if line.starts_with("#[cfg(test)]") || line.starts_with("#[test]") {
                let shown = path.strip_prefix(manifest_dir()).unwrap_or(path);
                found.push(format!("{}:{}", shown.display(), index + 1));
            }
        }
    }
    assert!(
        found.is_empty(),
        "`[lib] test = false`: a unit test in src/ never runs; move it to a black-box test under tests/ \
         (core/CLAUDE.md \"Tests\"):\n{}",
        found.join("\n")
    );
}

/// What makes a `let` binding a clock reading.
const CLOCK_READS: &[&str] = &[
    ".elapsed()",
    "duration_since(",
    "as_secs",
    "as_millis",
    "as_micros",
    "as_nanos",
];

/// What makes an assertion a duration assertion, besides a tainted name.
const CLOCK_WORDS: &[&str] = &[
    "Instant",
    ".elapsed()",
    "Duration",
    "as_secs",
    "as_millis",
    "as_micros",
    "as_nanos",
    "duration_since",
];

const ASSERT_MACROS: &[&str] = &[
    "assert!(",
    "assert_eq!(",
    "assert_ne!(",
    "debug_assert!(",
    "debug_assert_eq!(",
    "debug_assert_ne!(",
];

/// The `tests/common/` helpers whose verdict is a duration: a call to one in
/// default-run code counts as a duration assertion.
const CLOCK_VERDICTS: &[&str] = &["assert_pin", "assert_ramp"];

/// The hang guard a deadline loop may assert: not a verdict on a duration.
const HANG_GUARD: &str = "Instant::now() < deadline";

fn is_ident(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// `source` with every comment and every string or char literal's contents
/// blanked to spaces, so lines, columns and indentation stay where they were.
fn strip_comments_and_strings(source: &str) -> String {
    let chars: Vec<char> = source.chars().collect();
    let mut out = String::with_capacity(source.len());
    let blank = |c: char| if c == '\n' { '\n' } else { ' ' };
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        if c == '/' && next == Some('/') {
            while i < chars.len() && chars[i] != '\n' {
                out.push(' ');
                i += 1;
            }
        } else if c == '/' && next == Some('*') {
            let mut depth = 0;
            while i < chars.len() {
                if chars[i] == '/' && chars.get(i + 1) == Some(&'*') {
                    depth += 1;
                    out.push_str("  ");
                    i += 2;
                } else if chars[i] == '*' && chars.get(i + 1) == Some(&'/') {
                    depth -= 1;
                    out.push_str("  ");
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    out.push(blank(chars[i]));
                    i += 1;
                }
            }
        } else if (c == 'r' || (c == 'b' && next == Some('r')))
            && (i == 0 || !is_ident(chars[i - 1]))
            && {
                let start = if c == 'b' { i + 2 } else { i + 1 };
                let hashes = chars[start..].iter().take_while(|&&h| h == '#').count();
                chars.get(start + hashes) == Some(&'"')
            }
        {
            // A raw string: r"…", r#"…"#, br"…".
            let start = if c == 'b' { i + 2 } else { i + 1 };
            let hashes = chars[start..].iter().take_while(|&&h| h == '#').count();
            for &k in &chars[i..start + hashes + 1] {
                out.push(k);
            }
            i = start + hashes + 1;
            while i < chars.len() {
                if chars[i] == '"'
                    && chars[i + 1..]
                        .iter()
                        .take(hashes)
                        .filter(|&&h| h == '#')
                        .count()
                        == hashes
                {
                    out.push('"');
                    out.extend(std::iter::repeat_n('#', hashes));
                    i += 1 + hashes;
                    break;
                }
                out.push(blank(chars[i]));
                i += 1;
            }
        } else if c == '"' {
            out.push('"');
            i += 1;
            while i < chars.len() && chars[i] != '"' {
                if chars[i] == '\\' {
                    out.push(' ');
                    i += 1;
                }
                if i < chars.len() {
                    out.push(blank(chars[i]));
                    i += 1;
                }
            }
            out.push('"');
            i += 1;
        } else if c == '\'' && next == Some('\\') {
            // An escaped char literal: '\n', '\'', '\u{…}'.
            out.push('\'');
            i += 1;
            while i < chars.len() && !(chars[i] == '\'' && chars[i - 1] != '\\') {
                out.push(' ');
                i += 1;
            }
            out.push('\'');
            i += 1;
        } else if c == '\'' && chars.get(i + 2) == Some(&'\'') {
            // A one-char literal ('"', '{'); a lifetime has no closing quote.
            out.push_str("' '");
            i += 3;
        } else {
            out.push(c);
            i += 1;
        }
    }
    out
}

/// The text from `open` (an opening bracket) through its matching close.
fn balanced(text: &str, open: usize) -> &str {
    let mut depth = 0;
    for (i, c) in text[open..].char_indices() {
        match c {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => {
                depth -= 1;
                if depth == 0 {
                    return &text[open..open + i + 1];
                }
            }
            _ => {}
        }
    }
    &text[open..]
}

/// Whether `name` occurs in `text` as a whole identifier.
fn names(text: &str, name: &str) -> bool {
    text.match_indices(name).any(|(at, _)| {
        let before = text[..at].chars().next_back();
        let after = text[at + name.len()..].chars().next();
        !before.is_some_and(is_ident) && !after.is_some_and(is_ident)
    })
}

/// Whether `text` calls `name`: the identifier followed by `(`.
fn calls(text: &str, name: &str) -> bool {
    text.match_indices(name).any(|(at, _)| {
        let before = text[..at].chars().next_back();
        !before.is_some_and(is_ident) && text[at + name.len()..].starts_with('(')
    })
}

/// One `fn` item: its name, first and last line (0-based), and whether it is
/// exempt (`#[ignore` on it, or inside `mod bench`).
struct FnItem {
    name: String,
    first: usize,
    last: usize,
    exempt: bool,
    returns_duration: bool,
    indent: usize,
}

/// The `fn` items of a stripped source, bounded by rustfmt indentation: an
/// item opened at indent `i` closes at the next line that is exactly `i`
/// spaces plus `}`.
fn fn_items(lines: &[&str]) -> Vec<FnItem> {
    let close = |from: usize, indent: usize| {
        let end = format!("{}}}", " ".repeat(indent));
        (from..lines.len()).find(|&j| lines[j] == end)
    };
    let mut benches: Vec<(usize, usize)> = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let indent = line.len() - line.trim_start().len();
        if line.trim() == "mod bench {" {
            if let Some(end) = close(i + 1, indent) {
                benches.push((i, end));
            }
        }
    }
    let mut items = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();
        let Some(rest) = [
            "fn ",
            "pub fn ",
            "pub(crate) fn ",
            "unsafe fn ",
            "pub unsafe fn ",
        ]
        .iter()
        .find_map(|prefix| trimmed.strip_prefix(prefix)) else {
            continue;
        };
        let name: String = rest.chars().take_while(|&c| is_ident(c)).collect();
        // The header runs to the line holding the body's `{`; a `;` first
        // means a declaration with no body.
        let Some(open) =
            (i..lines.len()).find(|&j| lines[j].contains('{') || lines[j].ends_with(';'))
        else {
            continue;
        };
        if !lines[open].contains('{') {
            continue;
        }
        let header = lines[i..=open].join("\n");
        let header = &header[..header.find('{').unwrap_or(header.len())];
        let returns_duration = header
            .split_once("->")
            .is_some_and(|(_, ret)| ret.contains("Duration"));
        let last = if lines[open].trim_end().ends_with('}')
            && lines[open].matches('{').count() == lines[open].matches('}').count()
        {
            open
        } else {
            match close(open, indent) {
                Some(end) => end,
                None => continue,
            }
        };
        // The item's attributes: the `#[…]` lines above it, across the blank
        // lines its stripped doc comments left.
        let mut ignored = false;
        for above in lines[..i].iter().rev().map(|line| line.trim()) {
            if above.starts_with("#[") {
                ignored |= above.starts_with("#[ignore");
            } else if !above.is_empty() {
                break;
            }
        }
        let in_bench = benches.iter().any(|&(a, b)| a < i && i < b);
        items.push(FnItem {
            name,
            first: i,
            last,
            exempt: ignored || in_bench,
            returns_duration,
            indent,
        });
    }
    items
}

/// The names `let` binds in `pattern` (a plain or tuple pattern).
fn bound_names(pattern: &str) -> Vec<String> {
    let pattern = pattern.split(':').next().unwrap_or(pattern);
    pattern
        .split(|c: char| !is_ident(c))
        .filter(|word| !word.is_empty() && *word != "mut" && *word != "ref" && *word != "_")
        .filter(|word| !word.chars().next().is_some_and(|c| c.is_ascii_digit()))
        .map(str::to_string)
        .collect()
}

/// Each `let` in `body`: its bound names and its initializer, read up to the
/// terminating `;` at bracket depth 0.
fn lets(body: &str) -> Vec<(Vec<String>, String)> {
    let mut out = Vec::new();
    for (at, _) in body.match_indices("let ") {
        if body[..at].chars().next_back().is_some_and(is_ident) {
            continue;
        }
        let rest = &body[at + 4..];
        // The pattern ends at the first `=` that is not `==`, `<=`, `>=`, `!=`, `=>`.
        let bytes = rest.as_bytes();
        let mut depth = 0_i32;
        let mut eq = None;
        for (i, &b) in bytes.iter().enumerate() {
            match b {
                b'(' | b'[' | b'{' | b'<' => depth += 1,
                b')' | b']' | b'}' | b'>' => depth -= 1,
                b';' if depth <= 0 => break,
                b'=' if depth <= 0
                    && bytes.get(i + 1) != Some(&b'=')
                    && bytes.get(i + 1) != Some(&b'>')
                    && !matches!(
                        bytes.get(i.wrapping_sub(1)),
                        Some(b'=' | b'!' | b'<' | b'>')
                    ) =>
                {
                    eq = Some(i);
                    break;
                }
                _ => {}
            }
        }
        let Some(eq) = eq else { continue };
        let pattern = &rest[..eq];
        let init = &rest[eq + 1..];
        let mut depth = 0_i32;
        let mut end = init.len();
        for (i, c) in init.char_indices() {
            match c {
                '(' | '[' | '{' => depth += 1,
                ')' | ']' | '}' => depth -= 1,
                ';' if depth == 0 => {
                    end = i;
                    break;
                }
                _ => {}
            }
            if depth < 0 {
                end = i;
                break;
            }
        }
        out.push((bound_names(pattern), init[..end].to_string()));
    }
    out
}

/// The line numbers (1-based) of the duration assertions in one file's
/// default-run code.
fn duration_assertions(source: &str) -> Vec<usize> {
    let stripped = strip_comments_and_strings(source);
    let lines: Vec<&str> = stripped.lines().collect();
    let items = fn_items(&lines);
    let duration_fns: Vec<&str> = items
        .iter()
        .filter(|item| item.indent == 0 && item.returns_duration)
        .map(|item| item.name.as_str())
        .collect();
    // Each line's innermost enclosing fn.
    let innermost = |line: usize| {
        items
            .iter()
            .filter(|item| item.first <= line && line <= item.last)
            .max_by_key(|item| item.first)
    };
    let mut found = Vec::new();
    for item in &items {
        if item.exempt {
            continue;
        }
        let body = lines[item.first..=item.last].join("\n");
        let mut tainted: Vec<String> = Vec::new();
        let bindings = lets(&body);
        loop {
            let mut grew = false;
            for (bound, init) in &bindings {
                let reads = CLOCK_READS.iter().any(|read| init.contains(read))
                    || tainted.iter().any(|name| calls(init, name))
                    || duration_fns.iter().any(|name| calls(init, name));
                if !reads {
                    continue;
                }
                for name in bound {
                    if !tainted.contains(name) {
                        tainted.push(name.clone());
                        grew = true;
                    }
                }
            }
            if !grew {
                break;
            }
        }
        let starts: Vec<usize> = std::iter::once(0)
            .chain(body.match_indices('\n').map(|(i, _)| i + 1))
            .collect();
        // A call to a clock-verdict helper is a duration assertion.
        for (offset, text) in lines[item.first..=item.last].iter().enumerate() {
            let line = item.first + offset;
            if innermost(line).map(|f| f.first) == Some(item.first)
                && CLOCK_VERDICTS.iter().any(|v| calls(text, v))
                && !found.contains(&(line + 1))
            {
                found.push(line + 1);
            }
        }
        for macro_name in ASSERT_MACROS {
            for (at, _) in body.match_indices(macro_name) {
                if body[..at].chars().next_back().is_some_and(is_ident) {
                    continue;
                }
                let line = item.first + starts.iter().rposition(|&s| s <= at).unwrap_or(0);
                // A nested fn's assertion is judged by that fn.
                if innermost(line).map(|f| f.first) != Some(item.first) {
                    continue;
                }
                let open = at + macro_name.len() - 1;
                let text = balanced(&body, open);
                let inner = &text[1..text.len().saturating_sub(1)];
                let condition: String = {
                    let mut depth = 0;
                    let end = inner
                        .char_indices()
                        .find(|&(_, c)| {
                            match c {
                                '(' | '[' | '{' => depth += 1,
                                ')' | ']' | '}' => depth -= 1,
                                _ => {}
                            }
                            c == ',' && depth == 0
                        })
                        .map_or(inner.len(), |(i, _)| i);
                    inner[..end]
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" ")
                };
                if condition == HANG_GUARD {
                    continue;
                }
                // A park wait's timeout is a hang guard, not a verdict.
                let mut judged = String::new();
                let mut rest = text;
                while let Some(at) = rest.find("wait_parked(") {
                    let open = at + "wait_parked".len();
                    judged.push_str(&rest[..open]);
                    judged.push_str("()");
                    rest = &rest[open + balanced(rest, open).len()..];
                }
                judged.push_str(rest);
                let clock = CLOCK_WORDS.iter().any(|word| judged.contains(word))
                    || tainted.iter().any(|name| names(&judged, name));
                if clock && !found.contains(&(line + 1)) {
                    found.push(line + 1);
                }
            }
        }
    }
    found.sort_unstable();
    found
}

#[test]
fn no_default_test_asserts_a_duration() {
    let mut problems = Vec::new();
    let mut paths: Vec<PathBuf> = fs::read_dir(tests_dir())
        .expect("tests/ lists")
        .map(|entry| entry.expect("a tests/ entry").path())
        .filter(|path| path.is_file() && path.extension().and_then(|e| e.to_str()) == Some("rs"))
        .collect();
    paths.sort();
    for path in paths {
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .expect("a UTF-8 file name");
        if stem == "test_layout" {
            continue;
        }
        let source = fs::read_to_string(&path).expect("a test file reads");
        for line in duration_assertions(&source) {
            problems.push(format!(
                "tests/{stem}.rs:{line}: an assertion on a duration in default-run code: a wall-clock verdict depends on load, so move it into the file's `mod bench` twin under #[ignore = \"make cancel-bench: …\"] and run it with `make cancel-bench`"
            ));
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}
