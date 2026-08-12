//! The agent surface: the same analysis over the Model Context Protocol
//! on stdio, so a model can ask which constraints disagree rather than
//! be handed five manifests and asked to hold five grammars in its head.
//!
//! Two rules the family's MCP surfaces established:
//!
//! - **An empty answer is not an error.** A tree whose manifests agree
//!   comes back as an ordinary result carrying `ok: true` — the check
//!   ran. Only a malformed question is a protocol error.
//! - **Refusals speak the caller's vocabulary.** An MCP caller has no
//!   command line, so no message here mentions a flag.
//!
//! Read-only by construction: nothing on this surface writes.

pub(crate) mod compare;

use std::io::{BufRead, Write};
use std::process::ExitCode;

use serde_json::{Value, json};

use crate::detect::heuristics::Ecosystem;
use crate::discover::DiscoverOptions;
use crate::scan::{self, ScanOptions};

const PROTOCOL_VERSION: &str = "2025-06-18";

/// JSON-RPC error codes, from the spec.
const INVALID_PARAMS: i64 = -32602;
const METHOD_NOT_FOUND: i64 = -32601;

pub(crate) fn serve() -> ExitCode {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else {
            return ExitCode::from(2);
        };
        if line.trim().is_empty() {
            continue;
        }
        let Ok(request) = serde_json::from_str::<Value>(&line) else {
            // A frame that is not JSON has no id to answer against;
            // dropping it is the only honest option.
            continue;
        };
        let Some(response) = handle(&request) else {
            continue; // a notification: no reply
        };
        if writeln!(stdout, "{response}").is_err() || stdout.flush().is_err() {
            return ExitCode::from(2);
        }
    }
    ExitCode::SUCCESS
}

fn handle(request: &Value) -> Option<Value> {
    let id = request.get("id").cloned();
    let method = request.get("method")?.as_str()?;
    // Notifications carry no id and get no reply.
    id.as_ref()?;

    let result = match method {
        "initialize" => Ok(json!({
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "versions-le", "version": env!("CARGO_PKG_VERSION") },
        })),
        "tools/list" => Ok(json!({ "tools": tool_definitions() })),
        "tools/call" => call_tool(request.get("params")),
        "ping" => Ok(json!({})),
        other => Err((
            METHOD_NOT_FOUND,
            format!("this server does not implement {other}"),
        )),
    };

    Some(match result {
        Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
        Err((code, message)) => json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": { "code": code, "message": message },
        }),
    })
}

fn tool_definitions() -> Value {
    json!([
        compare::definition(),
        {
            "name": "versions_le_check",
            "description": "Find the manifests in a directory and report where the same \
                            dependency is constrained inconsistently. Reads the filesystem; never \
                            writes to it and never resolves a dependency or reaches the network. \
                            Comparison never crosses an ecosystem, and a constraint in a grammar \
                            the tool does not model is reported in `refusals` rather than guessed \
                            at.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "the directory to analyse" },
                    "ecosystems": {
                        "type": "array",
                        "items": {
                            "type": "string",
                            "enum": ["npm", "cargo", "python", "go", "ci"],
                        },
                        "description": "Limit the walk to these ecosystems. Empty means all.",
                    },
                    "exclude": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Glob patterns for manifests to skip.",
                    },
                    "hidden": {
                        "type": "boolean",
                        "default": false,
                        "description": "Descend hidden directories too. .github is always walked.",
                    },
                    "ignored": {
                        "type": "boolean",
                        "default": false,
                        "description": "Walk files excluded by .gitignore too.",
                    },
                },
                "required": ["path"],
            },
        },
    ])
}

/// Protocol failures (no tool named, an unknown tool) are JSON-RPC
/// errors; a tool that fails on its arguments returns a result carrying
/// `isError`, so a model reads the reason and reacts rather than
/// concluding the server is broken.
fn call_tool(params: Option<&Value>) -> Result<Value, (i64, String)> {
    let params = params.ok_or((INVALID_PARAMS, "no tool call was supplied".to_string()))?;
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or((INVALID_PARAMS, "the tool call named no tool".to_string()))?;
    let arguments = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));

    // Dispatch first, then the one place a tool outcome becomes a result:
    // a second copy of that mapping is how one tool ends up reporting a
    // failure differently from the other.
    let outcome = match name {
        "compare_versions" => compare::run(&arguments),
        "versions_le_check" => check_tool(&arguments),
        other => {
            return Err((
                INVALID_PARAMS,
                format!("this server offers no tool named {other}"),
            ));
        }
    };
    Ok(match outcome {
        Ok(result) => tool_result(&result),
        Err(message) => tool_failure(&message),
    })
}

fn check_tool(arguments: &Value) -> Result<Value, String> {
    let path = arguments
        .get("path")
        .and_then(Value::as_str)
        .ok_or_else(|| "no directory was supplied to analyse".to_string())?;
    let flag = |name: &str| {
        arguments
            .get(name)
            .and_then(Value::as_bool)
            .unwrap_or(false)
    };
    let list = |name: &str| -> Vec<String> {
        arguments
            .get(name)
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    };

    // An ecosystem name the tool does not know is a refusal, never a
    // silently wider walk: an answer over a scope nobody asked for reads
    // as clean when it is simply different.
    let mut ecosystems = Vec::new();
    for name in list("ecosystems") {
        let ecosystem = Ecosystem::parse(&name)
            .ok_or_else(|| format!("{name} is not one of npm, cargo, python, go or ci"))?;
        ecosystems.push(ecosystem);
    }

    let options = ScanOptions {
        discover: DiscoverOptions {
            hidden: flag("hidden"),
            respect_ignore: !flag("ignored"),
            exclude: list("exclude"),
            ecosystems,
        },
    };

    let report = scan::scan(&[std::path::PathBuf::from(path)], &options)?;
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
    let data = serde_json::to_value(&report).expect("a report serializes");

    Ok(envelope(
        "versions_le_check",
        &data,
        count,
        &diagnostics,
        false,
    ))
}

/// The one result shape every tool returns: `{ ok, data, diagnostics,
/// meta }`.
///
/// **`ok` reports whether the check ran, not whether the answer is
/// yes.** A tree full of conflicting pins is the answer, not a failure
/// to produce one — conflating the two would have a model report a
/// broken tool when what it actually learned is that the manifests
/// disagree.
pub(crate) fn envelope(
    tool: &str,
    data: &Value,
    count: usize,
    diagnostics: &[Value],
    truncated: bool,
) -> Value {
    let ok = !diagnostics
        .iter()
        .any(|diagnostic| diagnostic["severity"].as_str() == Some("error"));
    json!({
        "ok": ok,
        "data": data,
        "diagnostics": diagnostics,
        "meta": { "tool": tool, "count": count, "truncated": truncated },
    })
}

/// An MCP tool result: the envelope as text (what a model reads) and the
/// same envelope structured.
fn tool_result(envelope: &Value) -> Value {
    let text = serde_json::to_string_pretty(envelope).expect("an envelope serializes");
    json!({
        "content": [{ "type": "text", "text": text }],
        "structuredContent": envelope,
        "isError": false,
    })
}

/// The tool could not run on the arguments given. `isError` so a model
/// reads the message and corrects itself.
fn tool_failure(message: &str) -> Value {
    json!({
        "content": [{ "type": "text", "text": message }],
        "isError": true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::TempTree;

    fn request(method: &str, params: &Value) -> Value {
        json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params })
    }

    fn call(name: &str, arguments: &Value) -> Value {
        handle(&request(
            "tools/call",
            &json!({ "name": name, "arguments": arguments }),
        ))
        .expect("a reply")
    }

    #[test]
    fn initialize_answers_with_the_protocol_version() {
        let response = handle(&request("initialize", &json!({}))).expect("a reply");
        assert_eq!(response["result"]["protocolVersion"], PROTOCOL_VERSION);
        assert_eq!(response["result"]["serverInfo"]["name"], "versions-le");
    }

    #[test]
    fn tools_list_offers_both_tools() {
        let response = handle(&request("tools/list", &json!({}))).expect("a reply");
        let tools = response["result"]["tools"].as_array().expect("tools");
        let names: Vec<&str> = tools.iter().filter_map(|t| t["name"].as_str()).collect();
        assert_eq!(names, ["compare_versions", "versions_le_check"]);
    }

    #[test]
    fn a_notification_gets_no_reply() {
        assert!(handle(&json!({ "jsonrpc": "2.0", "method": "initialized" })).is_none());
    }

    #[test]
    fn an_unknown_method_is_a_protocol_error() {
        let response = handle(&request("does/not/exist", &json!({}))).expect("a reply");
        assert_eq!(response["error"]["code"], METHOD_NOT_FOUND);
    }

    #[test]
    fn an_unknown_tool_is_a_protocol_error() {
        assert_eq!(
            call("versions_le_translate", &json!({}))["error"]["code"],
            INVALID_PARAMS
        );
    }

    /// A bad argument is the tool failing on what it was given, not the
    /// server breaking.
    #[test]
    fn a_missing_argument_is_a_tool_failure_not_a_protocol_error() {
        let response = call("versions_le_check", &json!({}));
        assert!(response.get("error").is_none(), "{response}");
        assert_eq!(response["result"]["isError"], true);
        assert!(
            response["result"]["content"][0]["text"]
                .as_str()
                .expect("a message")
                .contains("no directory")
        );
    }

    #[test]
    fn the_shared_tool_finds_a_disjoint_pair() {
        let response = call(
            "compare_versions",
            &json!({ "files": [
                { "path": "a/Cargo.toml", "content": "[dependencies]\nserde = \"1\"\n" },
                { "path": "b/Cargo.toml", "content": "[dependencies]\nserde = \"2\"\n" },
            ]}),
        );
        let envelope = &response["result"]["structuredContent"];
        assert_eq!(envelope["meta"]["tool"], "compare_versions");
        assert_eq!(
            envelope["data"]["findings"][0]["code"],
            "disjoint-constraint"
        );
        assert_eq!(envelope["data"]["findings"][0]["severity"], "error");
        assert_eq!(response["result"]["isError"], false);
    }

    /// The shared tool reaches no filesystem — the property that lets an
    /// agent call it anywhere, and it must not regress.
    #[test]
    fn the_shared_tool_needs_no_filesystem() {
        let response = call(
            "compare_versions",
            &json!({ "files": [{
                "path": "/definitely/not/here/Cargo.toml",
                "content": "[dependencies]\nserde = \"1\"\n"
            }]}),
        );
        assert_eq!(response["result"]["isError"], false);
        assert_eq!(
            response["result"]["structuredContent"]["data"]["summary"]["manifests"],
            1
        );
    }

    /// Manifests that agree are an ordinary result, not an empty one.
    #[test]
    fn agreeing_manifests_are_an_ordinary_result() {
        let response = call(
            "compare_versions",
            &json!({ "files": [
                { "path": "a/Cargo.toml", "content": "[dependencies]\nserde = \"1\"\n" },
                { "path": "b/Cargo.toml", "content": "[dependencies]\nserde = \"1\"\n" },
            ]}),
        );
        let envelope = &response["result"]["structuredContent"];
        assert_eq!(envelope["ok"], true);
        assert_eq!(envelope["data"]["summary"]["findings"], 0);
    }

    /// The refusal spine, on the surface a model actually calls.
    #[test]
    fn a_grammar_the_tool_does_not_model_comes_back_as_a_refusal() {
        let response = call(
            "compare_versions",
            &json!({ "files": [
                { "path": "a/Cargo.toml", "content": "[dependencies]\nserde = \"1\"\n" },
                { "path": "b/Cargo.toml", "content": "[dependencies]\nserde = { workspace = true }\n" },
            ]}),
        );
        let data = &response["result"]["structuredContent"]["data"];
        assert_eq!(data["summary"]["findings"], 0);
        assert_eq!(data["refusals"][0]["reason"], "unknown_grammar");
    }

    #[test]
    fn the_check_tool_reports_what_it_found() {
        let tree = TempTree::new("mcp-check");
        tree.write("a/Cargo.toml", "[dependencies]\nserde = \"1.0.200\"\n");
        tree.write("b/Cargo.toml", "[dependencies]\nserde = \"1\"\n");
        let response = call(
            "versions_le_check",
            &json!({ "path": tree.path().to_string_lossy() }),
        );
        let envelope = &response["result"]["structuredContent"];
        assert_eq!(response["result"]["isError"], false);
        assert_eq!(
            envelope["data"]["findings"][0]["code"],
            "constraint-conflict"
        );
        assert_eq!(envelope["data"]["summary"]["manifests"], 2);
    }

    #[test]
    fn the_check_tool_honours_an_ecosystem_filter_and_refuses_an_unknown_one() {
        let tree = TempTree::new("mcp-filter");
        tree.write("Cargo.toml", "[dependencies]\nserde = \"1\"\n");
        tree.write("package.json", r#"{ "dependencies": { "a": "1" } }"#);
        let path = tree.path().to_string_lossy().to_string();

        let filtered = call(
            "versions_le_check",
            &json!({ "path": path, "ecosystems": ["cargo"] }),
        );
        assert_eq!(
            filtered["result"]["structuredContent"]["data"]["summary"]["manifests"],
            1
        );

        let refused = call(
            "versions_le_check",
            &json!({ "path": path, "ecosystems": ["maven"] }),
        );
        assert_eq!(refused["result"]["isError"], true);
    }

    /// Refusals speak the caller's vocabulary: an MCP caller has no
    /// command line, so no message may name a flag.
    #[test]
    fn no_message_mentions_a_command_line_flag() {
        let definitions = serde_json::to_string(&tool_definitions()).expect("serializes");
        assert!(!definitions.contains("--"), "{definitions}");

        let tree = TempTree::new("mcp-vocabulary");
        tree.write("Cargo.toml", "[dependencies]\nserde = \"1\"\n");
        for arguments in [
            json!({}),
            json!({ "paths": [] }),
            json!({ "path": "/no/such/place-xyz" }),
            json!({ "path": tree.path().to_string_lossy(), "ecosystems": ["maven"] }),
        ] {
            let rendered =
                serde_json::to_string(&call("versions_le_check", &arguments)).expect("serializes");
            assert!(!rendered.contains("--"), "{rendered}");
        }
    }

    /// Every tool returns the same envelope, so a caller writes one
    /// reader for all of them.
    #[test]
    fn every_tool_returns_the_same_envelope_shape() {
        let tree = TempTree::new("mcp-envelope");
        tree.write("Cargo.toml", "[dependencies]\nserde = \"1\"\n");
        let results = [
            call(
                "compare_versions",
                &json!({ "files": [{ "path": "Cargo.toml", "content": "" }] }),
            ),
            call(
                "versions_le_check",
                &json!({ "path": tree.path().to_string_lossy() }),
            ),
        ];
        for result in results {
            let envelope = &result["result"]["structuredContent"];
            assert!(envelope["ok"].is_boolean(), "{envelope}");
            assert!(!envelope["data"].is_null(), "{envelope}");
            assert!(envelope["diagnostics"].is_array(), "{envelope}");
            assert!(envelope["meta"]["tool"].is_string(), "{envelope}");
            assert!(envelope["meta"]["count"].is_number(), "{envelope}");
            assert!(envelope["meta"]["truncated"].is_boolean(), "{envelope}");
        }
    }
}
