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

/// The most bytes one frame may hold before it is refused.
///
/// Input with no newline was held whole: 64 MiB of it grew the process by
/// 64 MB and answered nothing. The same figure and message as the npm
/// server's, so a caller diffing the two finds nothing.
pub(crate) const MAX_FRAME_BYTES: usize = 16 * 1024 * 1024;

/// JSON-RPC error codes, from the spec.
const PARSE_ERROR: i64 = -32700;
const INVALID_PARAMS: i64 = -32602;
const METHOD_NOT_FOUND: i64 = -32601;

pub(crate) fn serve() -> ExitCode {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    serve_streams(stdin.lock(), stdout.lock())
}

/// The stdio loop over any pair of streams, so a test can drive it with
/// bytes.
///
/// Reads newline-delimited JSON. A line that is not UTF-8 is read with
/// replacement characters, as the npm server's decoder reads it. A line
/// that is not JSON, or not a request, is answered with a parse error in
/// the npm server's words rather than dropped, so a client that sent one
/// learns what happened; before, the line vanished without a word.
pub(crate) fn serve_streams(mut input: impl BufRead, mut output: impl Write) -> ExitCode {
    let mut frame: Vec<u8> = Vec::new();
    // Set once a frame has passed the limit: the rest of that line is
    // dropped as it arrives, so the oversized message is never held.
    let mut discarding = false;
    loop {
        let (consumed, complete) = {
            let Ok(chunk) = input.fill_buf() else {
                return ExitCode::from(2);
            };
            if chunk.is_empty() {
                break;
            }
            let newline = chunk.iter().position(|&byte| byte == b'\n');
            if !discarding {
                frame.extend_from_slice(&chunk[..newline.unwrap_or(chunk.len())]);
            }
            (newline.map_or(chunk.len(), |at| at + 1), newline.is_some())
        };
        input.consume(consumed);
        if discarding {
            discarding = !complete;
            continue;
        }
        if !complete {
            if frame.len() > MAX_FRAME_BYTES {
                frame = Vec::new();
                discarding = true;
                if emit(&mut output, &frame_refusal()).is_err() {
                    return ExitCode::from(2);
                }
            }
            continue;
        }
        let line = std::mem::take(&mut frame);
        if line.len() > MAX_FRAME_BYTES {
            // The newline arrived in the chunk that took the frame over the
            // limit, so the check above never saw it. The limit is on the
            // line, not on when its end happened to arrive.
            if emit(&mut output, &frame_refusal()).is_err() {
                return ExitCode::from(2);
            }
            continue;
        }
        let Some(response) = respond_to_line(&line) else {
            continue;
        };
        if emit(&mut output, &response).is_err() {
            return ExitCode::from(2);
        }
    }
    // A partial frame at close was never a request; the npm server drops
    // it the same way.
    ExitCode::SUCCESS
}

/// One line in, at most one reply out: the same refusals, in the same
/// words, as the npm server's loop.
fn respond_to_line(line: &[u8]) -> Option<Value> {
    let text = String::from_utf8_lossy(line);
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let Ok(request) = serde_json::from_str::<Value>(text) else {
        return Some(parse_error("Invalid JSON"));
    };
    if !is_request(&request) {
        return Some(parse_error("Not a JSON-RPC 2.0 request"));
    }
    handle(&request)
}

fn emit(output: &mut impl Write, message: &Value) -> std::io::Result<()> {
    writeln!(output, "{message}")?;
    output.flush()
}

fn is_request(value: &Value) -> bool {
    value.get("jsonrpc").and_then(Value::as_str) == Some("2.0")
        && value.get("method").is_some_and(Value::is_string)
}

fn frame_refusal() -> Value {
    parse_error(&format!(
        "Message exceeds the {MAX_FRAME_BYTES} byte frame limit"
    ))
}

fn parse_error(message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": null,
        "error": { "code": PARSE_ERROR, "message": message },
    })
}

fn handle(request: &Value) -> Option<Value> {
    let method = request.get("method")?.as_str()?;
    // A notification carries no id, or a null one, and takes no reply; the
    // npm server reads both the same way, and this answered the null one.
    let id = request.get("id").filter(|id| !id.is_null()).cloned()?;

    let result = match method {
        "initialize" => Ok(json!({
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "versions-le", "version": env!("CARGO_PKG_VERSION") },
        })),
        "tools/list" => Ok(json!({ "tools": tool_definitions() })),
        "tools/call" => call_tool(request.get("params")),
        "ping" => Ok(json!({})),
        other => Err((METHOD_NOT_FOUND, format!("Unknown method: {other}"))),
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
                "additionalProperties": false,
            },
        },
    ])
}

/// Protocol failures (no tool named, an unknown tool) are JSON-RPC
/// errors; a tool that fails on its arguments returns a result carrying
/// `isError`, so a model reads the reason and reacts rather than
/// concluding the server is broken.
fn call_tool(params: Option<&Value>) -> Result<Value, (i64, String)> {
    let no_name = || {
        (
            INVALID_PARAMS,
            "tools/call requires a string `name`".to_string(),
        )
    };
    let params = params.ok_or_else(no_name)?;
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(no_name)?;
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
        other => return Err((INVALID_PARAMS, format!("Unknown tool: {other}"))),
    };
    Ok(match outcome {
        Ok(result) => tool_result(&result),
        Err(message) => tool_failure(&message),
    })
}

const CHECK_ARGUMENTS: [&str; 5] = ["path", "ecosystems", "exclude", "hidden", "ignored"];

fn check_tool(arguments: &Value) -> Result<Value, String> {
    only(arguments, &CHECK_ARGUMENTS, "this tool")?;
    let path = arguments
        .get("path")
        .and_then(Value::as_str)
        .ok_or_else(|| "no directory was supplied to analyse".to_string())?;

    // An ecosystem name the tool does not know is a refusal, never a
    // silently wider walk: an answer over a scope nobody asked for reads
    // as clean when it is simply different.
    let mut ecosystems = Vec::new();
    for name in list(arguments, "ecosystems")? {
        let ecosystem = Ecosystem::parse(&name)
            .ok_or_else(|| format!("{name} is not one of npm, cargo, python, go or ci"))?;
        ecosystems.push(ecosystem);
    }

    let options = ScanOptions {
        discover: DiscoverOptions {
            hidden: flag(arguments, "hidden")?,
            respect_ignore: !flag(arguments, "ignored")?,
            exclude: list(arguments, "exclude")?,
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

// -----------------------------------------------------------------------
// Arguments
//
// **This surface is no laxer than the terminal one.** The CLI refuses
// `--stict` rather than ignoring it, because a flag that silently does
// nothing reports a clean audit of a check that never ran. `"hidden":
// "true"` is the same mistake wearing JSON: it walked past every hidden
// directory and said nothing. Absent is still absent — a default the
// schema declares — and so is an explicit `null`, which is how a client
// spells "not supplied". A value of the wrong shape is a refusal.
// -----------------------------------------------------------------------

/// Absent or null, or exactly the named arguments. A misspelled one is
/// refused rather than ignored, and the schemas say
/// `additionalProperties: false` — a schema that says so and a reader
/// that shrugs is a claim with nothing behind it.
fn only(arguments: &Value, allowed: &[&str], what: &str) -> Result<(), String> {
    let Some(object) = arguments.as_object() else {
        return Err(format!("{what} takes an object of arguments"));
    };
    let Some(unknown) = object.keys().find(|key| !allowed.contains(&key.as_str())) else {
        return Ok(());
    };
    Err(format!(
        "{unknown} is not an argument {what} takes. It takes {}.",
        allowed.join(", ")
    ))
}

fn flag(arguments: &Value, name: &str) -> Result<bool, String> {
    match supplied(arguments, name) {
        None => Ok(false),
        Some(value) => value
            .as_bool()
            .ok_or_else(|| format!("{name} takes true or false")),
    }
}

fn list(arguments: &Value, name: &str) -> Result<Vec<String>, String> {
    let Some(value) = supplied(arguments, name) else {
        return Ok(Vec::new());
    };
    let wrong = || format!("{name} takes a list of strings");
    value
        .as_array()
        .ok_or_else(wrong)?
        .iter()
        .map(|item| item.as_str().map(str::to_string).ok_or_else(wrong))
        .collect()
}

/// An explicit `null` is a client spelling "not supplied", not a value
/// of the wrong type. Everything else present is judged on its shape.
fn supplied<'a>(arguments: &'a Value, name: &str) -> Option<&'a Value> {
    arguments.get(name).filter(|value| !value.is_null())
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
        let null_id = json!({ "jsonrpc": "2.0", "id": null, "method": "ping" });
        assert!(handle(&null_id).is_none(), "a null id is a notification");
    }

    /// The loop, driven with bytes in the 8 KiB chunks stdin arrives in,
    /// with what it wrote and how it ended.
    fn drive(input: &[u8]) -> (String, Vec<Value>) {
        let mut written = Vec::new();
        let reader = std::io::BufReader::with_capacity(8192, std::io::Cursor::new(input.to_vec()));
        let code = serve_streams(reader, &mut written);
        let replies = String::from_utf8(written)
            .expect("replies are UTF-8")
            .lines()
            .map(|line| serde_json::from_str(line).expect("a reply is JSON"))
            .collect();
        (format!("{code:?}"), replies)
    }

    const PING: &[u8] = b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}\n";

    /// A line of invalid bytes was dropped without a word; it is a parse
    /// error now, and the ping after it is still answered.
    #[test]
    fn an_invalid_utf8_line_is_a_parse_error_and_the_server_reads_on() {
        let (code, replies) = drive(&[b"\xff\xfe\n".as_slice(), PING].concat());
        assert_eq!(code, format!("{:?}", ExitCode::SUCCESS));
        assert_eq!(replies[0], parse_error("Invalid JSON"));
        assert_eq!(replies[1]["id"], 1);
    }

    /// Inside a string the npm server's decoder replaces the byte and the
    /// request succeeds; this reads it the same way, where it dropped it.
    #[test]
    fn an_invalid_byte_inside_a_string_is_replaced_as_the_npm_server_reads_it() {
        let request = [
            br#"{"jsonrpc":"2.0","id":2,"method":"ping","x":"caf"#.as_slice(),
            b"\xe9",
            br#""}"#,
            b"\n",
        ]
        .concat();
        let (_, replies) = drive(&request);
        assert_eq!(replies[0]["id"], 2);
        assert_eq!(replies[0]["result"], json!({}));
    }

    /// A frame that is not JSON used to be dropped without a word.
    #[test]
    fn a_line_that_is_not_json_is_answered_with_a_parse_error() {
        let (_, replies) = drive(&[b"not json at all\n".as_slice(), PING].concat());
        assert_eq!(replies[0], parse_error("Invalid JSON"));
        assert_eq!(replies[1]["id"], 1);
    }

    /// A request without a method was dropped, and one without `jsonrpc`
    /// or with another version was answered; the npm server refuses all
    /// three the same way.
    #[test]
    fn a_line_that_is_not_a_request_is_answered_with_a_parse_error() {
        for line in [
            br#"{"id":1}"#.as_slice(),
            br#"{"id":1,"method":"ping"}"#,
            br#"{"jsonrpc":"1.0","id":1,"method":"ping"}"#,
            br#"{"jsonrpc":"2.0","id":1,"method":7}"#,
            b"[1,2,3]",
            b"5",
        ] {
            let (_, replies) = drive(&[line, b"\n"].concat());
            assert_eq!(
                replies[0],
                parse_error("Not a JSON-RPC 2.0 request"),
                "{line:?}"
            );
        }
    }

    /// Input with no newline was held whole. Past the limit the frame is
    /// refused in the npm server's words, the rest of the line is dropped
    /// as it arrives, and the next line is served.
    #[test]
    fn a_frame_over_the_limit_is_refused_and_the_next_line_served() {
        // One byte over, with the newline in the chunk that takes the frame
        // over the limit; and far over, with the newline many chunks later.
        for over in [1, 100_000] {
            let mut input = vec![b'x'; MAX_FRAME_BYTES + over];
            input.extend_from_slice(b"\n");
            input.extend_from_slice(PING);
            let (code, replies) = drive(&input);
            assert_eq!(code, format!("{:?}", ExitCode::SUCCESS));
            assert_eq!(replies[0], frame_refusal(), "{over} over");
            assert_eq!(replies[1]["id"], 1, "{over} over");
            assert_eq!(replies.len(), 2, "{over} over");
        }
    }

    /// Exactly at the limit is served; the limit is a ceiling, not a size.
    #[test]
    fn a_frame_at_the_limit_is_served() {
        let mut input = br#"{"jsonrpc":"2.0","id":1,"method":"ping","params":{"pad":""#.to_vec();
        let padding = MAX_FRAME_BYTES - input.len() - 3;
        input.extend(std::iter::repeat_n(b'x', padding));
        input.extend_from_slice(b"\"}}\n");
        assert_eq!(
            input.len(),
            MAX_FRAME_BYTES + 1,
            "the line plus its newline"
        );
        let (_, replies) = drive(&input);
        assert_eq!(replies[0]["id"], 1);
    }

    #[test]
    fn blank_lines_and_a_partial_frame_at_close_are_ignored() {
        let (code, replies) = drive(
            &[
                b"\n\r\n \t\n".as_slice(),
                PING,
                b"{\"jsonrpc\":\"2.0\",\"id\":9",
            ]
            .concat(),
        );
        assert_eq!(code, format!("{:?}", ExitCode::SUCCESS));
        assert_eq!(replies.len(), 1);
        assert_eq!(replies[0]["id"], 1);
    }

    /// The refusals a client can provoke read the same from both servers.
    #[test]
    fn protocol_refusals_use_the_npm_servers_words() {
        let response = handle(&request("does/not/exist", &json!({}))).expect("a reply");
        assert_eq!(
            response["error"]["message"],
            "Unknown method: does/not/exist"
        );
        let response = call("versions_le_translate", &json!({}));
        assert_eq!(
            response["error"]["message"],
            "Unknown tool: versions_le_translate"
        );
        for params in [json!({}), json!([1]), json!({ "name": 3 })] {
            let response = handle(&request("tools/call", &params)).expect("a reply");
            assert_eq!(
                response["error"]["message"], "tools/call requires a string `name`",
                "{params}"
            );
        }
        let response =
            handle(&json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call" })).expect("a reply");
        assert_eq!(
            response["error"]["message"],
            "tools/call requires a string `name`"
        );
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

    /// **The terminal surface refuses `--stict` rather than ignoring
    /// it**, because a flag that silently does nothing reports a clean
    /// audit of a check that never ran. A wrongly-typed argument is the
    /// same mistake wearing JSON — `"hidden": "true"` walked past every
    /// hidden directory and said nothing — and this surface must not be
    /// the lax one.
    #[test]
    fn a_wrongly_typed_argument_is_refused_rather_than_coerced() {
        let tree = TempTree::new("mcp-types");
        tree.write("Cargo.toml", "[dependencies]\nserde = \"1\"\n");
        let path = tree.path().to_string_lossy().to_string();

        for arguments in [
            json!({ "path": path, "hidden": "true" }),
            json!({ "path": path, "ignored": 1 }),
            json!({ "path": path, "ecosystems": "cargo" }),
            json!({ "path": path, "ecosystems": [7, "cargo"] }),
            json!({ "path": path, "exclude": [42] }),
        ] {
            let response = call("versions_le_check", &arguments);
            assert_eq!(
                response["result"]["isError"], true,
                "{arguments} was accepted"
            );
        }

        // Absent stays absent, and an explicit null is how a client
        // spells "not supplied" — neither is a wrong value.
        for arguments in [
            json!({ "path": path }),
            json!({ "path": path, "hidden": null, "ecosystems": null }),
        ] {
            assert_eq!(
                call("versions_le_check", &arguments)["result"]["isError"],
                false,
                "{arguments} was refused"
            );
        }
    }

    /// A misspelled argument must not silently widen or narrow the walk.
    /// `--hiden` is refused on the command line; `"hiden"` is refused
    /// here.
    #[test]
    fn an_unknown_argument_is_refused_by_name() {
        let tree = TempTree::new("mcp-unknown");
        tree.write("Cargo.toml", "[dependencies]\nserde = \"1\"\n");
        let response = call(
            "versions_le_check",
            &json!({ "path": tree.path().to_string_lossy(), "hiden": true }),
        );
        assert_eq!(response["result"]["isError"], true, "{response}");
        assert!(
            response["result"]["content"][0]["text"]
                .as_str()
                .expect("a message")
                .contains("hiden"),
            "{response}"
        );
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
