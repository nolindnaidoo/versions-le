//! The manifest readers: text in, constraint entries out.
//!
//! One reader per manifest, and every one of them **skips what it does
//! not model rather than failing the file**. A `Cargo.toml` key this
//! tool has never heard of is not a parse error; it is a key that is not
//! a dependency constraint.
//!
//! Where a value *is* a constraint but in a grammar the tool does not
//! model — `workspace = true`, a git URL, PEP 440's `~=` — the reader
//! emits the entry **and** an `unknown_grammar` refusal. The entry stays
//! visible in the report; the comparison skips it. Dropping it silently
//! would read as "there was nothing there".

use serde::{Deserialize, Serialize};
use serde_json::Value as Json;
use toml::Value as Toml;

use super::grammar::{self, Constraint};
use super::heuristics::{Ecosystem, ManifestKind, looks_like_a_version};

/// Where a constraint sits in a manifest. `line` is carried only where
/// the reader honestly knows it — the two line-scanning readers, `go.mod`
/// and the workflow one, do; the structured readers address by key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Site {
    pub(crate) file: String,
    pub(crate) key: String,
    pub(crate) constraint: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) line: Option<u32>,
}

/// What a constraint is *for*. It decides which checks apply: a
/// prerelease is a finding in a runtime dependency and ordinary in a dev
/// one, and a minimum-toolchain declaration has its own check rather
/// than being compared as if it were a package.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Kind {
    Runtime,
    Dev,
    Build,
    Peer,
    Optional,
    Engine,
    Msrv,
    Tool,
    Action,
}

impl Kind {
    /// Whether a prerelease here is ordinary. A test harness on a
    /// release candidate is a choice; a shipped dependency on one is a
    /// finding.
    pub(crate) fn is_development(self) -> bool {
        matches!(self, Self::Dev | Self::Build)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Entry {
    pub(crate) ecosystem: Ecosystem,
    pub(crate) name: String,
    pub(crate) kind: Kind,
    pub(crate) site: Site,
    pub(crate) constraint: Constraint,
    /// Why this pin floats, when it does.
    pub(crate) floats: Option<&'static str>,
}

/// Something the tool saw and deliberately did not analyse.
///
/// A refusal is not a finding: it is the tool naming the limit of what
/// it knows, so an answer that is narrower than the tree cannot be read
/// as a clean one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Refusal {
    pub(crate) reason: String,
    /// `null` for `cross_ecosystem`, which by definition spans several.
    pub(crate) ecosystem: Option<Ecosystem>,
    pub(crate) name: String,
    pub(crate) message: String,
    pub(crate) sites: Vec<Site>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ParseError {
    #[serde(rename = "type")]
    pub(crate) kind: String,
    pub(crate) message: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Parsed {
    pub(crate) entries: Vec<Entry>,
    pub(crate) refusals: Vec<Refusal>,
    pub(crate) errors: Vec<ParseError>,
}

impl Parsed {
    fn push(&mut self, entry: Entry) {
        if let Constraint::Unknown(reason) = entry.constraint {
            self.refusals.push(Refusal {
                reason: "unknown_grammar".to_string(),
                ecosystem: Some(entry.ecosystem),
                name: entry.name.clone(),
                message: format!("{reason}; excluded from comparison"),
                sites: vec![entry.site.clone()],
            });
        }
        self.entries.push(entry);
    }

    fn ambiguous(&mut self, ecosystem: Ecosystem, name: &str, site: Site, why: &str) {
        self.refusals.push(Refusal {
            reason: "ambiguous_version_string".to_string(),
            ecosystem: Some(ecosystem),
            name: name.to_string(),
            message: format!("{why}; excluded from comparison"),
            sites: vec![site],
        });
    }

    fn error(&mut self, message: &str) {
        self.errors.push(ParseError {
            kind: "parse-error".to_string(),
            message: message.to_string(),
        });
    }
}

pub(crate) fn parse(kind: ManifestKind, file: &str, content: &str) -> Parsed {
    let mut out = Parsed::default();
    match kind {
        ManifestKind::PackageJson => package_json(&mut out, file, content),
        ManifestKind::CargoToml => cargo_toml(&mut out, file, content),
        ManifestKind::PyprojectToml => pyproject_toml(&mut out, file, content),
        ManifestKind::GoMod => go_mod(&mut out, file, content),
        ManifestKind::Workflow => workflow(&mut out, file, content),
    }
    out
}

fn site(file: &str, key: String, constraint: &str, line: Option<u32>) -> Site {
    Site {
        file: file.to_string(),
        key,
        constraint: constraint.to_string(),
        line,
    }
}

// ---------------------------------------------------------------------
// package.json
// ---------------------------------------------------------------------

const NPM_SECTIONS: [(&str, Kind); 4] = [
    ("dependencies", Kind::Runtime),
    ("devDependencies", Kind::Dev),
    ("peerDependencies", Kind::Peer),
    ("optionalDependencies", Kind::Optional),
];

fn package_json(out: &mut Parsed, file: &str, content: &str) {
    let Ok(root) = serde_json::from_str::<Json>(content) else {
        out.error("not valid JSON");
        return;
    };

    for (section, kind) in NPM_SECTIONS {
        let Some(table) = root.get(section).and_then(Json::as_object) else {
            continue;
        };
        for (name, value) in table {
            let Some(raw) = value.as_str() else {
                out.error(&format!("{section}.{name} is not a version string"));
                continue;
            };
            out.push(Entry {
                ecosystem: Ecosystem::Npm,
                name: name.clone(),
                kind,
                site: site(file, format!("{section}.{name}"), raw, None),
                constraint: grammar::npm(raw),
                floats: npm_floats(raw),
            });
        }
    }

    if let Some(engines) = root.get("engines").and_then(Json::as_object) {
        for (name, value) in engines {
            let Some(raw) = value.as_str() else { continue };
            out.push(Entry {
                ecosystem: Ecosystem::Npm,
                name: name.clone(),
                kind: Kind::Engine,
                site: site(file, format!("engines.{name}"), raw, None),
                constraint: grammar::npm(raw),
                floats: npm_floats(raw),
            });
        }
    }

    package_manager(out, file, &root);
}

/// `"packageManager": "bun@1.1.0"` — Corepack's pin, and the one place a
/// `package.json` names a tool version rather than a dependency.
fn package_manager(out: &mut Parsed, file: &str, root: &Json) {
    let Some(raw) = root.get("packageManager").and_then(Json::as_str) else {
        return;
    };
    let Some((name, version)) = raw.split_once('@') else {
        out.error("packageManager is not <name>@<version>");
        return;
    };
    // Corepack appends a `+sha512.…` integrity hash; it is not part of
    // the version.
    let version = version.split('+').next().unwrap_or(version);
    out.push(Entry {
        ecosystem: Ecosystem::Npm,
        name: name.to_string(),
        kind: Kind::Tool,
        site: site(file, "packageManager".to_string(), version, None),
        constraint: grammar::npm(version),
        floats: npm_floats(version),
    });
}

fn npm_floats(raw: &str) -> Option<&'static str> {
    let text = raw.trim();
    if text.is_empty() || matches!(text, "*" | "x" | "X") {
        return Some("an unbounded range accepts any future major");
    }
    if matches!(text, "latest" | "next") {
        return Some("a dist tag resolves to whatever is newest at install time");
    }
    is_zero_caret(text)
}

/// A caret below 1.0 is a floating pin, not a tight one: `^0.4.1` admits
/// every later 0.4 patch, and a 0.x crate makes no stability promise
/// about those.
fn is_zero_caret(text: &str) -> Option<&'static str> {
    let rest = text.strip_prefix('^')?;
    let major = rest.split(['.', '-']).next()?;
    (major == "0").then_some("a caret on a 0.x version, which promises no stability")
}

// ---------------------------------------------------------------------
// Cargo.toml
// ---------------------------------------------------------------------

const CARGO_SECTIONS: [(&str, Kind); 3] = [
    ("dependencies", Kind::Runtime),
    ("dev-dependencies", Kind::Dev),
    ("build-dependencies", Kind::Build),
];

fn cargo_toml(out: &mut Parsed, file: &str, content: &str) {
    let Ok(root) = toml::from_str::<Toml>(content) else {
        out.error("not valid TOML");
        return;
    };

    for (section, kind) in CARGO_SECTIONS {
        cargo_section(out, file, root.get(section), section, kind);
        let workspace = root.get("workspace").and_then(|table| table.get(section));
        cargo_section(out, file, workspace, &format!("workspace.{section}"), kind);
    }
    cargo_targets(out, file, &root);

    for key in ["package.rust-version", "workspace.package.rust-version"] {
        let Some(raw) = dig(&root, key).and_then(Toml::as_str) else {
            continue;
        };
        out.push(Entry {
            ecosystem: Ecosystem::Cargo,
            name: "rust".to_string(),
            kind: Kind::Msrv,
            site: site(file, key.to_string(), raw, None),
            constraint: grammar::minimum(raw),
            floats: None,
        });
    }
}

/// `[target.'cfg(windows)'.dependencies]` and friends. A platform
/// dependency is still a dependency, and skipping it would make a
/// windows-only version conflict invisible on a mac.
fn cargo_targets(out: &mut Parsed, file: &str, root: &Toml) {
    let Some(targets) = root.get("target").and_then(Toml::as_table) else {
        return;
    };
    for (platform, table) in targets {
        for (section, kind) in CARGO_SECTIONS {
            let key = format!("target.{platform}.{section}");
            cargo_section(out, file, table.get(section), &key, kind);
        }
    }
}

fn cargo_section(out: &mut Parsed, file: &str, table: Option<&Toml>, key: &str, kind: Kind) {
    let Some(table) = table.and_then(Toml::as_table) else {
        return;
    };
    for (alias, value) in table {
        let (name, raw, unknown) = cargo_dependency(alias, value);
        let raw = raw.unwrap_or_default();
        out.push(Entry {
            ecosystem: Ecosystem::Cargo,
            name,
            kind,
            site: site(file, format!("{key}.{alias}"), &raw, None),
            constraint: match unknown {
                Some(reason) => Constraint::Unknown(reason),
                None => grammar::cargo(&raw),
            },
            floats: unknown.is_none().then(|| cargo_floats(&raw)).flatten(),
        });
    }
}

/// The identity and the constraint of one `[dependencies]` entry.
///
/// A table entry is renamed by `package`, and its constraint is the
/// `version` key. **A `path` or `git` source with a `version` keeps that
/// version**: it is a real registry requirement that Cargo enforces on
/// publish, and refusing it would hide exactly the exact-patch pins a
/// workspace uses to hold its own crates together. Only a table with no
/// `version` at all — `workspace = true`, a bare path, a bare git
/// dependency — has no constraint to compare.
fn cargo_dependency(alias: &str, value: &Toml) -> (String, Option<String>, Option<&'static str>) {
    if let Some(raw) = value.as_str() {
        return (alias.to_string(), Some(raw.to_string()), None);
    }
    let Some(table) = value.as_table() else {
        return (alias.to_string(), None, Some("not a version requirement"));
    };
    let name = table
        .get("package")
        .and_then(Toml::as_str)
        .unwrap_or(alias)
        .to_string();
    if let Some(raw) = table.get("version").and_then(Toml::as_str) {
        return (name, Some(raw.to_string()), None);
    }
    (name, None, Some(no_version_reason(table)))
}

/// Why a dependency table has nothing to compare — ranked, most specific
/// source first, so `{ workspace = true, path = "…" }` is named as the
/// inheritance it is.
fn no_version_reason(table: &toml::Table) -> &'static str {
    if table.contains_key("workspace") {
        return "an inherited workspace dependency carries its version elsewhere";
    }
    if table.contains_key("git") {
        return "a git dependency resolves outside the registry";
    }
    if table.contains_key("path") {
        return "a path dependency with no version has no registry requirement";
    }
    "a dependency table with no version"
}

fn cargo_floats(raw: &str) -> Option<&'static str> {
    let text = raw.trim();
    if text.is_empty() || matches!(text, "*" | "x" | "X") {
        return Some("an unbounded range accepts any future major");
    }
    if let Some(reason) = is_zero_caret(text) {
        return Some(reason);
    }
    // A bare Cargo requirement *is* a caret, so `"0.4"` floats for the
    // same reason `"^0.4"` does — the operator is just written down.
    let bare = text.starts_with(|c: char| c.is_ascii_digit());
    (bare && text.starts_with('0'))
        .then_some("a caret on a 0.x version, which promises no stability")
}

/// A dotted key path through a TOML document.
fn dig<'a>(root: &'a Toml, path: &str) -> Option<&'a Toml> {
    path.split('.')
        .try_fold(root, |value, segment| value.get(segment))
}

// ---------------------------------------------------------------------
// pyproject.toml
// ---------------------------------------------------------------------

fn pyproject_toml(out: &mut Parsed, file: &str, content: &str) {
    let Ok(root) = toml::from_str::<Toml>(content) else {
        out.error("not valid TOML");
        return;
    };

    if let Some(raw) = dig(&root, "project.requires-python").and_then(Toml::as_str) {
        out.push(Entry {
            ecosystem: Ecosystem::Python,
            name: "python".to_string(),
            kind: Kind::Engine,
            site: site(file, "project.requires-python".to_string(), raw, None),
            constraint: grammar::python(raw),
            floats: python_floats(raw),
        });
    }

    pep621_list(
        out,
        file,
        dig(&root, "project.dependencies"),
        "project.dependencies",
        Kind::Runtime,
    );

    let Some(groups) = dig(&root, "project.optional-dependencies").and_then(Toml::as_table) else {
        return;
    };
    for (group, list) in groups {
        let key = format!("project.optional-dependencies.{group}");
        pep621_list(out, file, Some(list), &key, Kind::Dev);
    }
}

fn pep621_list(out: &mut Parsed, file: &str, list: Option<&Toml>, key: &str, kind: Kind) {
    let Some(items) = list.and_then(Toml::as_array) else {
        return;
    };
    for item in items {
        let Some(text) = item.as_str() else {
            out.error(&format!("{key} holds something that is not a requirement"));
            continue;
        };
        let (name, specifier, unknown) = pep508(text);
        if name.is_empty() {
            out.error(&format!("{key}: \"{text}\" names no distribution"));
            continue;
        }
        out.push(Entry {
            ecosystem: Ecosystem::Python,
            name,
            kind,
            site: site(file, format!("{key}.{text}"), &specifier, None),
            constraint: match unknown {
                Some(reason) => Constraint::Unknown(reason),
                None => grammar::python(&specifier),
            },
            floats: unknown
                .is_none()
                .then(|| python_floats(&specifier))
                .flatten(),
        });
    }
}

/// Split a PEP 508 requirement into a distribution name and a version
/// specifier.
///
/// Environment markers and direct URL references are refused rather than
/// stripped: a requirement that only applies below Python 3.11 is not
/// in conflict with one that does not, and pretending otherwise would
/// manufacture a finding.
fn pep508(text: &str) -> (String, String, Option<&'static str>) {
    let trimmed = text.trim();
    let name: String = trimmed
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
        .collect();
    let rest = trimmed[name.len()..].trim();

    if trimmed.contains(';') {
        return (name, String::new(), Some("an environment marker"));
    }
    if rest.starts_with('@') {
        return (name, String::new(), Some("a direct URL reference"));
    }
    // Extras change what is installed, never which versions are allowed.
    let rest = match rest.strip_prefix('[') {
        Some(tail) => tail.split_once(']').map_or("", |(_, after)| after).trim(),
        None => rest,
    };
    let specifier = rest.trim_start_matches('(').trim_end_matches(')').trim();
    (name, specifier.to_string(), None)
}

fn python_floats(raw: &str) -> Option<&'static str> {
    raw.trim()
        .is_empty()
        .then_some("no version specifier at all")
}

// ---------------------------------------------------------------------
// go.mod
// ---------------------------------------------------------------------

fn go_mod(out: &mut Parsed, file: &str, content: &str) {
    let mut in_require = false;
    for (index, raw_line) in content.lines().enumerate() {
        let line = strip_comment(raw_line).trim();
        // `// indirect` marks the resolver's output, not this module's
        // stated requirement, so it is not one of this file's claims.
        if line.is_empty() || raw_line.contains("// indirect") {
            continue;
        }
        let number = u32::try_from(index + 1).unwrap_or(u32::MAX);

        if line == ")" {
            in_require = false;
            continue;
        }
        if let Some(rest) = line.strip_prefix("go ") {
            out.push(go_language(file, rest.trim(), number));
            continue;
        }
        if line.starts_with("require (") {
            in_require = true;
            continue;
        }
        let requirement = match line.strip_prefix("require ") {
            Some(rest) => rest.trim(),
            None if in_require => line,
            None => continue,
        };
        let Some((module, version)) = requirement.split_once(char::is_whitespace) else {
            continue;
        };
        out.push(go_module(file, module, version.trim(), number));
    }
}

/// The `go` directive: a floor, not a version.
fn go_language(file: &str, raw: &str, line: u32) -> Entry {
    go_entry(
        file,
        "go",
        Kind::Engine,
        "go",
        grammar::minimum(raw),
        raw,
        line,
    )
}

/// A `require` version. Go module versions are exact.
fn go_module(file: &str, name: &str, raw: &str, line: u32) -> Entry {
    go_entry(
        file,
        name,
        Kind::Runtime,
        "require",
        grammar::go(raw),
        raw,
        line,
    )
}

/// Both go.mod entries, without a `match` on `Kind` deciding which
/// grammar to use: the two callers above are the only two shapes the
/// file has, and a wildcard arm would quietly hand a future kind to the
/// wrong one.
fn go_entry(
    file: &str,
    name: &str,
    kind: Kind,
    key: &str,
    constraint: Constraint,
    raw: &str,
    line: u32,
) -> Entry {
    Entry {
        ecosystem: Ecosystem::Go,
        name: name.to_string(),
        kind,
        site: site(file, key.to_string(), raw, Some(line)),
        constraint,
        floats: None,
    }
}

fn strip_comment(line: &str) -> &str {
    line.split_once("//").map_or(line, |(before, _)| before)
}

// ---------------------------------------------------------------------
// .github/workflows/*.yml
// ---------------------------------------------------------------------

/// The workflow reader scans **lines, not YAML**.
///
/// It wants two things — the `@ref` of a `uses:` and the value of a
/// `<tool>-version:` input — and both are single-line in every workflow
/// anyone writes. A YAML parser would buy structure this then discards,
/// at the cost of a dependency. What keeps the shortcut honest is the
/// evidence gate: a value that is not evidently a version becomes an
/// `ambiguous_version_string` refusal instead of an invented entry.
fn workflow(out: &mut Parsed, file: &str, content: &str) {
    for (index, raw_line) in content.lines().enumerate() {
        let line = raw_line.trim().trim_start_matches("- ").trim();
        if line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim();
        let value = unquote(value);
        let number = u32::try_from(index + 1).unwrap_or(u32::MAX);

        if key == "uses" {
            workflow_uses(out, file, value, number);
            continue;
        }
        let Some(tool) = workflow_tool(key) else {
            continue;
        };
        let at = site(file, key.to_string(), value, Some(number));
        if !looks_like_a_version(value) {
            out.ambiguous(Ecosystem::Ci, tool, at, "not evidently a version");
            continue;
        }
        out.push(Entry {
            ecosystem: Ecosystem::Ci,
            name: tool.to_string(),
            kind: if tool == "rust" {
                Kind::Msrv
            } else {
                Kind::Tool
            },
            site: at,
            constraint: grammar::ci_tool(value),
            floats: ci_floats(value),
        });
    }
}

/// `node-version`, `bun-version`, `python-version` … and `toolchain`,
/// which is what the Rust setup actions call theirs.
fn workflow_tool(key: &str) -> Option<&str> {
    if key == "toolchain" {
        return Some("rust");
    }
    let tool = key.strip_suffix("-version")?;
    (!tool.is_empty() && tool.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
        .then_some(tool)
}

fn workflow_uses(out: &mut Parsed, file: &str, value: &str, line: u32) {
    let Some((action, reference)) = value.rsplit_once('@') else {
        // A local or docker action has no version to read.
        return;
    };
    // `dtolnay/rust-toolchain@1.88` puts the toolchain in the ref, so
    // that ref is an MSRV claim rather than an action pin. Reading it as
    // an action version would leave the repository's real Rust pin
    // invisible to the MSRV check.
    //
    // **Unless the ref is a commit SHA.** `rust-toolchain@<sha>` is the
    // hardened spelling: the SHA pins the action and the toolchain comes
    // from a `toolchain:` input below it. Reading the SHA as a toolchain
    // makes a `malformed-constraint` out of the most careful thing a
    // workflow can do, which is the exact opposite of the point.
    let rust_toolchain = action.ends_with("rust-toolchain") && !grammar::is_commit_sha(reference);
    let key = if rust_toolchain { "toolchain" } else { "uses" };
    let at = site(file, key.to_string(), reference, Some(line));
    if rust_toolchain {
        out.push(Entry {
            ecosystem: Ecosystem::Ci,
            name: "rust".to_string(),
            kind: Kind::Msrv,
            site: at,
            constraint: grammar::ci_tool(reference),
            floats: ci_floats(reference),
        });
        return;
    }
    out.push(Entry {
        ecosystem: Ecosystem::Ci,
        name: action.to_string(),
        kind: Kind::Action,
        site: at,
        constraint: grammar::action_ref(reference),
        floats: action_floats(reference),
    });
}

fn ci_floats(raw: &str) -> Option<&'static str> {
    let text = raw.trim();
    if matches!(text, "*" | "x" | "X") {
        return Some("an unbounded range accepts any future major");
    }
    matches!(
        text,
        "latest" | "stable" | "nightly" | "beta" | "current" | "node" | "lts"
    )
    .then_some("an unpinned tool version installs whatever is newest that day")
    .or_else(|| {
        text.starts_with("lts/")
            .then_some("an unpinned tool version installs whatever is newest that day")
    })
}

fn action_floats(reference: &str) -> Option<&'static str> {
    if matches!(grammar::action_ref(reference), Constraint::Unknown(_))
        && !reference.chars().all(|c| c.is_ascii_hexdigit())
    {
        return Some("a branch or tag reference moves under the workflow");
    }
    None
}

fn unquote(value: &str) -> &str {
    let value = value.trim();
    // A trailing `# …` is a comment unless the value itself is quoted.
    let value = if value.starts_with(['"', '\'']) {
        value
    } else {
        value.split_once(" #").map_or(value, |(before, _)| before)
    };
    value
        .trim()
        .trim_matches(|c| matches!(c, '"' | '\''))
        .trim()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(kind: ManifestKind, content: &str) -> Parsed {
        parse(kind, "manifest", content)
    }

    fn constraint_of(parsed: &Parsed, key: &str) -> String {
        parsed
            .entries
            .iter()
            .find(|entry| entry.site.key == key)
            .map_or_else(
                || panic!("no entry at {key}"),
                |entry| entry.site.constraint.clone(),
            )
    }

    #[test]
    fn every_package_json_section_is_read() {
        let parsed = read(
            ManifestKind::PackageJson,
            r#"{
              "dependencies": { "react": "^18.2.0" },
              "devDependencies": { "vitest": "1.0.0" },
              "peerDependencies": { "react": ">=18" },
              "optionalDependencies": { "fsevents": "~2.3.0" },
              "engines": { "node": ">=20" },
              "packageManager": "bun@1.1.0"
            }"#,
        );
        assert_eq!(parsed.entries.len(), 6);
        assert_eq!(constraint_of(&parsed, "engines.node"), ">=20");
        assert_eq!(constraint_of(&parsed, "packageManager"), "1.1.0");
        assert!(parsed.errors.is_empty(), "{:?}", parsed.errors);
    }

    #[test]
    fn a_corepack_integrity_hash_is_not_part_of_the_version() {
        let parsed = read(
            ManifestKind::PackageJson,
            r#"{ "packageManager": "pnpm@9.0.0+sha512.abc" }"#,
        );
        assert_eq!(constraint_of(&parsed, "packageManager"), "9.0.0");
    }

    #[test]
    fn an_npm_specifier_outside_the_registry_is_refused_by_name() {
        let parsed = read(
            ManifestKind::PackageJson,
            r#"{ "dependencies": { "shared": "workspace:*", "ok": "^1.0.0" } }"#,
        );
        assert_eq!(parsed.refusals.len(), 1);
        assert_eq!(parsed.refusals[0].reason, "unknown_grammar");
        assert_eq!(parsed.refusals[0].name, "shared");
        // The entry stays visible; only the comparison skips it.
        assert_eq!(parsed.entries.len(), 2);
    }

    #[test]
    fn every_cargo_section_is_read_including_workspace_and_target() {
        let parsed = read(
            ManifestKind::CargoToml,
            r#"
            [package]
            rust-version = "1.88"
            [dependencies]
            serde = "1.0.200"
            [dev-dependencies]
            tempfile = "3"
            [build-dependencies]
            cc = "1"
            [workspace.dependencies]
            regex = "1"
            [target.'cfg(windows)'.dependencies]
            winapi = "0.3"
            "#,
        );
        assert_eq!(constraint_of(&parsed, "dependencies.serde"), "1.0.200");
        assert_eq!(constraint_of(&parsed, "workspace.dependencies.regex"), "1");
        assert_eq!(
            constraint_of(&parsed, "target.cfg(windows).dependencies.winapi"),
            "0.3"
        );
        assert_eq!(constraint_of(&parsed, "package.rust-version"), "1.88");
    }

    /// The exact-patch pin a workspace uses to hold its own crates
    /// together is a real registry requirement, and losing it would be
    /// losing the thing most worth checking.
    #[test]
    fn a_path_dependency_that_carries_a_version_keeps_it() {
        let parsed = read(
            ManifestKind::CargoToml,
            r#"
            [dependencies]
            core = { path = "../core", version = "=0.3.1" }
            "#,
        );
        assert_eq!(constraint_of(&parsed, "dependencies.core"), "=0.3.1");
        assert!(parsed.refusals.is_empty(), "{:?}", parsed.refusals);
    }

    /// **The real-world finding**, from running the binary against a
    /// workspace whose documentation said its binaries pinned their core
    /// "at an exact patch". They did not: `version = "0.7.7"` beside a
    /// `path` is a *caret* requirement in Cargo — `[0.7.7, 0.8.0)` — and
    /// only `=0.7.7` is the pin that was meant. One character apart, and
    /// the two must never collapse into each other.
    #[test]
    fn a_bare_version_in_a_path_dependency_floats_and_an_equals_pin_does_not() {
        let floating = read(
            ManifestKind::CargoToml,
            "[dependencies]\ncore = { path = \"../core\", version = \"0.7.7\" }\n",
        );
        assert_eq!(
            floating.entries[0].floats,
            Some("a caret on a 0.x version, which promises no stability"),
            "a bare 0.x version beside a path was read as a pin"
        );

        let pinned = read(
            ManifestKind::CargoToml,
            "[dependencies]\ncore = { path = \"../core\", version = \"=0.7.7\" }\n",
        );
        assert_eq!(pinned.entries[0].floats, None, "an = pin was read as float");
    }

    #[test]
    fn a_dependency_table_with_no_version_is_refused() {
        let parsed = read(
            ManifestKind::CargoToml,
            r#"
            [dependencies]
            inherited = { workspace = true }
            local = { path = "../local" }
            remote = { git = "https://example.com/a.git" }
            "#,
        );
        assert_eq!(parsed.refusals.len(), 3);
        for refusal in &parsed.refusals {
            assert_eq!(refusal.reason, "unknown_grammar");
        }
    }

    #[test]
    fn a_renamed_cargo_dependency_reports_its_real_name() {
        let parsed = read(
            ManifestKind::CargoToml,
            r#"
            [dependencies]
            rand_old = { package = "rand", version = "0.8" }
            "#,
        );
        assert_eq!(parsed.entries[0].name, "rand");
    }

    #[test]
    fn pep621_dependencies_and_requires_python_are_read() {
        let parsed = read(
            ManifestKind::PyprojectToml,
            r#"
            [project]
            requires-python = ">=3.11"
            dependencies = ["requests>=2.31", "pydantic[email]>=2,<3"]
            [project.optional-dependencies]
            dev = ["pytest>=8"]
            "#,
        );
        assert_eq!(parsed.entries.len(), 4);
        let names: Vec<&str> = parsed.entries.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"python"));
        assert!(names.contains(&"pydantic"));
        let pytest = parsed
            .entries
            .iter()
            .find(|e| e.name == "pytest")
            .expect("pytest");
        assert_eq!(pytest.kind, Kind::Dev);
    }

    /// A requirement that only applies below Python 3.11 is not in
    /// conflict with one that does not.
    #[test]
    fn an_environment_marker_is_refused_rather_than_stripped() {
        let parsed = read(
            ManifestKind::PyprojectToml,
            r#"
            [project]
            dependencies = ["tomli>=2 ; python_version < '3.11'"]
            "#,
        );
        assert_eq!(parsed.refusals.len(), 1);
        assert!(parsed.refusals[0].message.contains("marker"));
    }

    #[test]
    fn go_mod_reads_the_require_block_and_the_language_version() {
        let parsed = read(
            ManifestKind::GoMod,
            "module example.com/a\n\ngo 1.22\n\nrequire (\n\tgithub.com/x/y v1.4.0\n\tgithub.com/z/w v0.9.1 // indirect\n)\n\nrequire github.com/single/dep v2.0.0\n",
        );
        let names: Vec<&str> = parsed.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["go", "github.com/x/y", "github.com/single/dep"]);
        assert_eq!(parsed.entries[1].site.line, Some(6));
    }

    #[test]
    fn a_workflow_yields_action_pins_and_tool_versions() {
        let parsed = read(
            ManifestKind::Workflow,
            "jobs:\n  test:\n    steps:\n      - uses: actions/checkout@v4\n      - uses: oven-sh/setup-bun@v2\n        with:\n          bun-version: 1.1.0\n      - uses: dtolnay/rust-toolchain@1.88\n",
        );
        let names: Vec<&str> = parsed.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(
            names,
            ["actions/checkout", "oven-sh/setup-bun", "bun", "rust"]
        );
        assert_eq!(parsed.entries[3].kind, Kind::Msrv);
        assert_eq!(parsed.entries[2].site.line, Some(7));
    }

    /// Regression: `rust-toolchain@<sha>` is the hardened spelling, and
    /// reading the SHA as a toolchain made a `malformed-constraint` out
    /// of the most careful thing a workflow can do.
    #[test]
    fn a_sha_pinned_rust_toolchain_is_an_action_pin_not_a_toolchain() {
        let parsed = read(
            ManifestKind::Workflow,
            "      - uses: dtolnay/rust-toolchain@11bd71901bbe5b1630ceea73d27597364c9af683\n        with:\n          toolchain: 1.88\n",
        );
        let names: Vec<&str> = parsed.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["dtolnay/rust-toolchain", "rust"]);
        assert_eq!(parsed.entries[0].kind, Kind::Action);
        assert_eq!(parsed.entries[1].kind, Kind::Msrv);
        assert!(
            !matches!(parsed.entries[0].constraint, Constraint::Malformed(_)),
            "{:?}",
            parsed.entries[0].constraint
        );
    }

    /// The gate that keeps a matrix expression out of the comparison.
    #[test]
    fn a_workflow_value_that_is_not_evidently_a_version_is_refused() {
        let parsed = read(
            ManifestKind::Workflow,
            "        node-version: ${{ matrix.node }}\n        python-version: [3.11, 3.12]\n",
        );
        assert!(parsed.entries.is_empty(), "{:?}", parsed.entries);
        assert_eq!(parsed.refusals.len(), 2);
        for refusal in &parsed.refusals {
            assert_eq!(refusal.reason, "ambiguous_version_string");
        }
    }

    #[test]
    fn a_quoted_workflow_value_loses_its_quotes_and_its_comment() {
        let parsed = read(
            ManifestKind::Workflow,
            "        node-version: '20'\n        bun-version: 1.1.0 # pinned\n",
        );
        assert_eq!(constraint_of(&parsed, "node-version"), "20");
        assert_eq!(constraint_of(&parsed, "bun-version"), "1.1.0");
    }

    #[test]
    fn a_floating_pin_is_named_where_it_floats() {
        let npm = read(
            ManifestKind::PackageJson,
            r#"{ "dependencies": { "a": "latest", "b": "*", "c": "^0.4.1", "d": "^1.0.0" } }"#,
        );
        let floating: Vec<&str> = npm
            .entries
            .iter()
            .filter(|e| e.floats.is_some())
            .map(|e| e.name.as_str())
            .collect();
        assert_eq!(floating, ["a", "b", "c"]);

        let cargo = read(
            ManifestKind::CargoToml,
            "[dependencies]\nignore = \"0.4\"\nserde = \"1\"\n",
        );
        let floating: Vec<&str> = cargo
            .entries
            .iter()
            .filter(|e| e.floats.is_some())
            .map(|e| e.name.as_str())
            .collect();
        assert_eq!(floating, ["ignore"]);
    }

    #[test]
    fn a_manifest_that_does_not_parse_is_one_error_and_no_entries() {
        let parsed = read(ManifestKind::PackageJson, "{ not json");
        assert!(parsed.entries.is_empty());
        assert_eq!(parsed.errors.len(), 1);
    }

    /// A key this tool has never heard of is not a dependency
    /// constraint, and must never be a parse failure for the file.
    #[test]
    fn unmodelled_keys_are_skipped_not_failed() {
        let parsed = read(
            ManifestKind::CargoToml,
            "[package]\nname = \"a\"\n[features]\ndefault = []\n[profile.release]\nlto = true\n",
        );
        assert!(parsed.entries.is_empty());
        assert!(parsed.errors.is_empty());
    }
}
