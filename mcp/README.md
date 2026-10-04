# versions-le-mcp

<p align="center">
  <a href="https://marketplace.visualstudio.com/items?itemName=nolindnaidoo.versions-le">
    <img src="https://img.shields.io/badge/Install%20from-VS%20Code-blue?style=for-the-badge&logo=visualstudiocode" alt="Install from VS Code Marketplace" />
  </a>
  <a href="https://open-vsx.org/extension/nolindnaidoo/versions-le">
    <img src="https://img.shields.io/open-vsx/dt/nolindnaidoo/versions-le?style=for-the-badge&label=Open%20VSX&color=blue" alt="Open VSX downloads" />
  </a>
  <a href="https://www.npmjs.com/package/versions-le-mcp">
    <img src="https://img.shields.io/npm/v/versions-le-mcp?style=for-the-badge&label=MCP%20server&color=blue&logo=npm" alt="versions-le-mcp on npm" />
  </a>
  <a href="https://letools.dev/tools/versions-le">
    <img src="https://img.shields.io/badge/LE%20Tools-letools.dev-blue?style=for-the-badge" alt="LE Tools" />
  </a>
</p>

An [MCP](https://modelcontextprotocol.io) server that compares the version
constraints in a set of manifests — `package.json`, `Cargo.toml`,
`pyproject.toml`, `go.mod` and GitHub workflows — and reports where one
dependency is constrained differently, and where two constraints cannot both
be satisfied by any version. It is the engine behind the
[Versions-LE](https://letools.dev/tools/versions-le) editor extension, exposed
as a tool an agent can call.

**Comparison never crosses an ecosystem.** An npm `semver` and a Cargo
`semver` are unrelated packages that share a word, and a bare `"1.0.200"` means
exactly that version in npm and anything below 2 in Cargo.

**A constraint in a grammar it does not model is refused by name, never
approximated** — a git or path specifier, a workspace inheritance, a PEP 440
compatible-release clause. It is reported in `refusals` and left out of every
comparison.

No dependencies, no network calls, no filesystem access. Content goes in,
structured results come out.

## Use it

Point any MCP host at `npx versions-le-mcp`.

**Claude Code**

```bash
claude mcp add versions-le -- npx -y versions-le-mcp
```

**Anything with a JSON config** — Cursor, Windsurf, Claude Desktop:

```json
{
  "mcpServers": {
    "versions-le": {
      "command": "npx",
      "args": ["-y", "versions-le-mcp"]
    }
  }
}
```

**VS Code and Zed** need nothing here. Install the extension instead — it
carries this server and registers it for you:
[VS Code Marketplace](https://marketplace.visualstudio.com/items?itemName=nolindnaidoo.versions-le)
· [Open VSX](https://open-vsx.org/extension/nolindnaidoo/versions-le)
· [Zed](https://zed.dev/docs/ai/mcp) *(no listing yet — add it by hand)*

**No Node?** The same `compare_versions` tool ships in a static Rust binary:
`cargo install versions-le`, then `versions-le mcp`
([crates.io](https://crates.io/crates/versions-le)). The two servers answer
identically — one corpus runs against both, and a differential test feeds both
thousands of generated manifest sets, broken JSON and TOML included, and
compares every answer. The binary additionally offers `versions_le_check`,
which walks a tree; **this server reads no files**.

Prefer a global install to `npx` on every launch:

```bash
npm install -g versions-le-mcp
```

No environment variables, no API key, no configuration of its own. To check it
before wiring it into anything:

```bash
echo '{"jsonrpc":"2.0","id":1,"method":"tools/list"}' | npx -y versions-le-mcp
```

If that prints the tool name, the server works.

## The tool

### `compare_versions`

| argument | type | |
|---|---|---|
| `files` | `{ path, content }[]` | **required.** The manifests. The path decides the grammar and labels every finding: `package.json`, `Cargo.toml`, `pyproject.toml`, `go.mod`, or a `.yml`/`.yaml` under `.github/workflows/`. |
| `maxResults` | number | Default `500`, ceiling `5000`. |

Findings are `disjoint-constraint`, `malformed-constraint` (errors),
`constraint-conflict`, `msrv-mismatch`, `prerelease-in-production` (warnings)
and `floating-pin` (info), each with every site that produced it. Refusals are
`unknown_grammar`, `ambiguous_version_string`, `cross_ecosystem` and
`per_job_tool_version`. A manifest that does not parse is a diagnostic, and
`ok` is `false` while one is present:

```json
{
  "ok": true,
  "data": {
    "schema": 1,
    "manifests": [
      {
        "path": "api/Cargo.toml",
        "ecosystem": "cargo",
        "entries": 1
      },
      {
        "path": "web/Cargo.toml",
        "ecosystem": "cargo",
        "entries": 1
      }
    ],
    "findings": [
      {
        "code": "disjoint-constraint",
        "severity": "error",
        "ecosystem": "cargo",
        "name": "regex",
        "message": "\"1\" (api/Cargo.toml) and \"2\" (web/Cargo.toml) cannot both be satisfied by one version",
        "sites": [
          {
            "file": "api/Cargo.toml",
            "key": "dependencies.regex",
            "constraint": "1"
          },
          {
            "file": "web/Cargo.toml",
            "key": "dependencies.regex",
            "constraint": "2"
          }
        ]
      }
    ],
    "refusals": [],
    "summary": {
      "manifests": 2,
      "entries": 2,
      "findings": 1,
      "refusals": 0,
      "errors": 1,
      "warnings": 0,
      "infos": 0
    }
  },
  "diagnostics": [],
  "meta": {
    "tool": "compare_versions",
    "count": 1,
    "truncated": false
  }
}
```

## Also in the MCP registry

`io.github.nolindnaidoo/versions-le` —
[registry.modelcontextprotocol.io](https://registry.modelcontextprotocol.io)

## Fourteen more like it

One tool each, same shape: content in, structured data out, no network and no
filesystem. Every one is on npm as `<name>-mcp` and in the MCP registry as
`io.github.nolindnaidoo/<name>`.

| Package | Tool | Does |
|---|---|---|
| [`urls-le-mcp`](https://www.npmjs.com/package/urls-le-mcp) | `extract_urls` | URLs, with protocol and position |
| [`colors-le-mcp`](https://www.npmjs.com/package/colors-le-mcp) | `extract_colors` | colors from stylesheets and code |
| [`dates-le-mcp`](https://www.npmjs.com/package/dates-le-mcp) | `extract_dates` | dates and timestamps |
| [`numbers-le-mcp`](https://www.npmjs.com/package/numbers-le-mcp) | `extract_numbers` | numeric values |
| [`paths-le-mcp`](https://www.npmjs.com/package/paths-le-mcp) | `extract_paths` | file and directory paths |
| [`string-le-mcp`](https://www.npmjs.com/package/string-le-mcp) | `extract_strings` | string values |
| [`regex-le-mcp`](https://www.npmjs.com/package/regex-le-mcp) | `extract_patterns` | regexes, with a ReDoS verdict |
| [`secrets-le-mcp`](https://www.npmjs.com/package/secrets-le-mcp) | `detect_secrets` | credentials, masked — never the value |
| [`envsync-le-mcp`](https://www.npmjs.com/package/envsync-le-mcp) | `compare_env_files` | dotenv key drift, names only |
| [`scrape-le-mcp`](https://www.npmjs.com/package/scrape-le-mcp) | `analyze_robots_txt` | whether a path may be crawled |
| [`unicode-le-mcp`](https://www.npmjs.com/package/unicode-le-mcp) | `detect_unicode_risks` | Unicode that hides meaning, as codepoints |
| [`i18n-le-mcp`](https://www.npmjs.com/package/i18n-le-mcp) | `check_catalogues` | translation catalogues, keys only |
| [`ids-le-mcp`](https://www.npmjs.com/package/ids-le-mcp) | `extract_ids` | UUIDs, ULIDs and Snowflakes, with the time inside |
| [`ips-le-mcp`](https://www.npmjs.com/package/ips-le-mcp) | `extract_ips` | IP addresses, CIDR blocks and MACs, normalized |

Every tool in the family, one page: **[letools.dev](https://letools.dev)**

## Built by

**[Nolin Naidoo](https://nolindnaidoo.com)** — Chief Engineer, AI/ML & Platform
Architecture. [nolindnaidoo.com](https://nolindnaidoo.com) ·
[GitHub](https://github.com/nolindnaidoo) ·
[LinkedIn](https://www.linkedin.com/in/nolindnaidoo/)

### Also from the same workshop

Twelve Rust tools built the same way: small, single-purpose, and driven by a
machine rather than a person. pixelcoords and pixelactions make up one loop —
pixelcoords answers *where*, pixelactions *acts* there. The ten LE crates are
the terminal half of the extensions they sit in: the same detection, held to
the extension's own corpus, and an exit code instead of a results editor.

| | | |
|---|---|---|
| **[pixelcoords](https://github.com/nolindnaidoo/pixelcoords)** | Freeze your screen, mark regions, get pixel-exact coordinates and crops | [site](https://pixelcoords.dev) · [crates.io](https://crates.io/crates/pixelcoords) · [docs.rs](https://docs.rs/pixelcoords) |
| **[pixelactions](https://github.com/nolindnaidoo/pixelactions)** | Consume human-verified coordinates, perform the interaction, confirm it landed | [site](https://pixelactions.dev) · [crates.io](https://crates.io/crates/pixelactions) · [docs.rs](https://docs.rs/pixelactions) |
| **[paths-le](https://github.com/nolindnaidoo/paths-le/tree/main/crate)** | Find every path in a codebase and report whether it still points at anything | [crates.io](https://crates.io/crates/paths-le) |
| **[secrets-le](https://github.com/nolindnaidoo/secrets-le/tree/main/crate)** | Find hardcoded credentials, and never print one | [crates.io](https://crates.io/crates/secrets-le) |
| **[urls-le](https://github.com/nolindnaidoo/urls-le/tree/main/crate)** | Extract every URL from a codebase, with its protocol and exact position | [crates.io](https://crates.io/crates/urls-le) |
| **[regex-le](https://github.com/nolindnaidoo/regex-le/tree/main/crate)** | Find every regex in a codebase and report which can be driven into catastrophic backtracking | [crates.io](https://crates.io/crates/regex-le) |
| **[string-le](https://github.com/nolindnaidoo/string-le/tree/main/crate)** | Get every string in a codebase out where a person can read them | [crates.io](https://crates.io/crates/string-le) |
| **[numbers-le](https://github.com/nolindnaidoo/numbers-le/tree/main/crate)** | Find every hardcoded number in a codebase so a person can check them | [crates.io](https://crates.io/crates/numbers-le) |
| **[envsync-le](https://github.com/nolindnaidoo/envsync-le/tree/main/crate)** | Compare the dotenv files in a tree and say which keys are missing from which | [crates.io](https://crates.io/crates/envsync-le) |
| **[colors-le](https://github.com/nolindnaidoo/colors-le/tree/main/crate)** | Find every colour in a codebase, and say which are not in your palette | [crates.io](https://crates.io/crates/colors-le) |
| **[dates-le](https://github.com/nolindnaidoo/dates-le/tree/main/crate)** | Extract every date and timestamp, and the exact instant each one resolves to | [crates.io](https://crates.io/crates/dates-le) |
| **[scrape-le](https://github.com/nolindnaidoo/scrape-le/tree/main/crate)** | Check whether a page is scrapeable before the scraper is written | [crates.io](https://crates.io/crates/scrape-le) |

## Licence

MIT © [Nolin Naidoo](https://nolindnaidoo.com)
