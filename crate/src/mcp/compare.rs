//! `compare_versions` — the tool an agent calls.
//!
//! It takes manifest *contents* directly and touches no filesystem. An
//! agent already has file-read tools; duplicating them here would add a
//! path-traversal surface for no capability. The tool that needs a
//! filesystem is `versions_le_check`.
//!
//! The `path` of each file is not decoration: it decides which grammar
//! the content is read with, and it is how every finding names itself.
//! A path this tool does not recognise as a manifest is refused by name
//! rather than guessed at from the content — a JSON document is not
//! necessarily a `package.json`.

use serde_json::{Value, json};

use crate::scan::{self, Document};

const DEFAULT_MAX_RESULTS: usize = 500;
const MAX_MAX_RESULTS: usize = 5000;

pub(crate) fn definition() -> Value {
    json!({
        "name": "compare_versions",
        "description": "Compare the version constraints in a set of manifests and report where \
                        the same dependency is constrained inconsistently — including where two \
                        constraints cannot both be satisfied. Takes file contents directly and \
                        reads no filesystem. Comparison never crosses an ecosystem: an npm \
                        `semver` and a Cargo `semver` are unrelated packages that share a word. \
                        A constraint in a grammar the tool does not model is reported in \
                        `refusals` and excluded from comparison rather than guessed at.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "files": {
                    "type": "array",
                    "minItems": 1,
                    "description": "The manifests to compare.",
                    "items": {
                        "type": "object",
                        "properties": {
                            "path": {
                                "type": "string",
                                "description": "Path or filename, e.g. \"crates/api/Cargo.toml\". \
                                                It decides which grammar the content is read \
                                                with, and labels every finding. Recognised: \
                                                package.json, Cargo.toml, pyproject.toml, go.mod, \
                                                and any .yml or .yaml under .github/workflows/.",
                            },
                            "content": { "type": "string", "description": "The file contents." },
                        },
                        "required": ["path", "content"],
                        "additionalProperties": false,
                    },
                },
                "maxResults": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": MAX_MAX_RESULTS,
                    "default": DEFAULT_MAX_RESULTS,
                    "description": format!(
                        "Cap on returned findings (default {DEFAULT_MAX_RESULTS}). \
                         meta.truncated reports whether any were dropped."
                    ),
                },
            },
            "required": ["files"],
            "additionalProperties": false,
        },
    })
}

pub(crate) fn run(arguments: &Value) -> Result<Value, String> {
    let documents = read_files(arguments)?;
    let max_results = read_max_results(arguments)?;
    let mut report = scan::report_for(&documents);

    // The `truncated` flag matters more than the cap: a silently
    // incomplete answer is wrong in the most expensive way, and this is
    // a tool whose whole job is telling you what disagrees.
    let truncated = report.findings.len() > max_results;
    report.findings.truncate(max_results);

    let diagnostics: Vec<Value> = report
        .diagnostics
        .iter()
        .map(|diagnostic| {
            json!({
                "severity": diagnostic.severity,
                "code": diagnostic.code,
                "message": format!("{}: {}", diagnostic.file, diagnostic.message),
            })
        })
        .collect();

    let count = report.findings.len();
    let data = json!({
        "schema": report.schema,
        "manifests": report.manifests,
        "findings": report.findings,
        "refusals": report.refusals,
        "summary": report.summary,
    });
    Ok(super::envelope(
        "compare_versions",
        &data,
        count,
        &diagnostics,
        truncated,
    ))
}

fn read_files(arguments: &Value) -> Result<Vec<Document>, String> {
    let invalid =
        "files is required and must be a non-empty array of { path, content }".to_string();
    let items = arguments
        .get("files")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid.clone())?;
    if items.is_empty() {
        return Err(invalid);
    }

    items
        .iter()
        .map(|item| {
            let path = item
                .get("path")
                .and_then(Value::as_str)
                .ok_or_else(|| invalid.clone())?;
            let content = item
                .get("content")
                .and_then(Value::as_str)
                .ok_or_else(|| invalid.clone())?;
            let kind = scan::manifest_of(std::path::Path::new(path)).ok_or_else(|| {
                format!(
                    "{path} is not a manifest this tool reads. It reads package.json, Cargo.toml, \
                     pyproject.toml, go.mod, and .yml or .yaml under .github/workflows/."
                )
            })?;
            Ok(Document {
                kind,
                label: path.to_string(),
                content: content.to_string(),
            })
        })
        .collect()
}

/// Clamp quietly, reject loudly.
fn read_max_results(arguments: &Value) -> Result<usize, String> {
    let Some(raw) = arguments.get("maxResults") else {
        return Ok(DEFAULT_MAX_RESULTS);
    };
    let invalid = "maxResults must be a positive integer".to_string();
    let value = raw.as_u64().ok_or_else(|| invalid.clone())?;
    if value < 1 {
        return Err(invalid);
    }
    Ok(usize::try_from(value)
        .unwrap_or(MAX_MAX_RESULTS)
        .min(MAX_MAX_RESULTS))
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;

    use super::*;
    use crate::detect::corpus::document;

    const CASES: &str = include_str!("../../fixtures/mcp-compare-versions.json");

    #[derive(Debug, Deserialize)]
    struct Case {
        name: String,
        files: Option<Vec<String>>,
        arguments: Value,
        expected: Option<Value>,
        #[serde(rename = "expectedError")]
        expected_error: Option<String>,
    }

    #[test]
    fn every_corpus_case_answers_identically() {
        let cases: Vec<Case> = serde_json::from_str(CASES).expect("the corpus is valid JSON");
        assert!(!cases.is_empty(), "the corpus is empty");

        for case in cases {
            let mut arguments = case.arguments.clone();
            if let Some(names) = &case.files {
                arguments["files"] = Value::Array(
                    names
                        .iter()
                        .map(|name| json!({ "path": name, "content": document(name) }))
                        .collect(),
                );
            }

            match (case.expected, case.expected_error) {
                (_, Some(expected)) => assert_eq!(
                    run(&arguments).expect_err(&case.name),
                    expected,
                    "{}",
                    case.name
                ),
                (Some(expected), None) => {
                    assert_eq!(
                        run(&arguments).expect(&case.name),
                        expected,
                        "{}",
                        case.name
                    );
                }
                (None, None) => panic!("{} pins neither a result nor an error", case.name),
            }
        }
    }

    #[test]
    fn the_tool_name_is_pinned() {
        assert_eq!(definition()["name"], "compare_versions");
    }

    /// A JSON document is not necessarily a `package.json`, and reading
    /// one as if it were would invent entries out of nothing.
    #[test]
    fn a_path_that_is_not_a_manifest_is_refused_by_name() {
        let error = run(&json!({
            "files": [{ "path": "config/settings.json", "content": "{}" }]
        }))
        .expect_err("a refusal");
        assert!(error.contains("settings.json"), "{error}");
        assert!(error.contains("package.json"), "{error}");
    }

    #[test]
    fn a_missing_files_argument_is_refused() {
        let error = run(&json!({})).expect_err("a refusal");
        assert!(error.contains("files is required"), "{error}");
        assert!(run(&json!({ "files": [] })).is_err());
    }

    #[test]
    fn a_fractional_cap_is_refused() {
        let error = run(&json!({
            "files": [{ "path": "Cargo.toml", "content": "" }],
            "maxResults": 1.5
        }))
        .expect_err("a refusal");
        assert_eq!(error, "maxResults must be a positive integer");
    }

    /// The schema may not offer a way to ask the tool to change a file.
    #[test]
    fn the_schema_offers_nothing_that_writes() {
        let definition = definition();
        let properties = definition["inputSchema"]["properties"]
            .as_object()
            .expect("properties");
        for absent in ["fix", "write", "update", "pin"] {
            assert!(!properties.contains_key(absent), "{absent} is offered");
        }
    }
}
