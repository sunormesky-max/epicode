//! SDK contract: every HTTP route the Python/TypeScript SDKs call must be
//! registered in cloud.rs with that method, and every MCP tool they call must
//! exist with the argument names they send. Static scan of the SDK sources —
//! a renamed/removed route or tool turns CI red instead of shipping a 404.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

fn read(rel: &str) -> String {
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    std::fs::read_to_string(base.join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

/// path (params normalized to ":p") -> methods, from cloud.rs.
fn routes() -> BTreeMap<String, BTreeSet<String>> {
    let src = read("backend/src/bin/cloud.rs");
    let mut out: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for chunk in src.split(".route(").skip(1) {
        let Some(rest) = chunk.trim_start().strip_prefix('"') else {
            continue;
        };
        let Some(end) = rest.find('"') else { continue };
        let body = &rest[end..];
        let body = &body[..body.find(".layer(").unwrap_or(body.len())];
        let e = out.entry(norm(&rest[..end])).or_default();
        for m in ["get", "post", "put", "delete", "patch"] {
            let hit = body.match_indices(&format!("{m}(")).any(|(i, _)| {
                i == 0 || {
                    let b = body.as_bytes()[i - 1];
                    !b.is_ascii_alphanumeric() && b != b'_'
                }
            });
            if hit {
                e.insert(m.to_uppercase());
            }
        }
    }
    out
}

/// Normalize `:id`, `{node_id}` and `${...}` segments to `:p`.
fn norm(path: &str) -> String {
    path.split('/')
        .map(|seg| {
            if seg.starts_with(':') || seg.starts_with('{') || seg.starts_with("${") {
                ":p"
            } else {
                seg
            }
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// First string literal ("...", '...' or `...`) at the start of `s` (after whitespace).
fn literal(s: &str) -> Option<String> {
    let t = s.trim_start();
    let t = t.strip_prefix('f').unwrap_or(t);
    let q = t.chars().next()?;
    if !"\"'`".contains(q) {
        return None;
    }
    let inner = &t[1..];
    Some(inner[..inner.find(q)?].to_string())
}

/// (method, public path relative to the client base, root_relative) calls.
struct Call {
    file: &'static str,
    method: String,
    path: String,
    root: bool,
}

fn python_calls(file: &'static str, root: bool) -> Vec<Call> {
    let src = read(file);
    src.split("self._request(")
        .skip(1)
        .filter_map(|c| {
            let method = literal(c)?;
            let after = &c[c.find(',')? + 1..];
            Some(Call {
                file,
                method,
                path: literal(after)?,
                root,
            })
        })
        .collect()
}

fn typescript_calls(file: &'static str) -> Vec<Call> {
    let src = read(file);
    let mut out = Vec::new();
    for (needle, root) in [("this.baseUrl,", false), ("this.rootUrl,", true)] {
        for c in src.split(needle).skip(1) {
            let Some(path) = literal(c) else { continue };
            let after = &c[c.find(',').unwrap() + 1..];
            let Some(method) = literal(after) else {
                continue;
            };
            out.push(Call {
                file,
                method,
                path,
                root,
            });
        }
    }
    out
}

#[test]
fn sdk_http_calls_hit_registered_routes() {
    let routes = routes();
    let mut calls = python_calls("backend/sdk/python/epicode/client.py", false);
    calls.extend(python_calls("backend/sdk/python/epicode/admin.py", true));
    calls.extend(typescript_calls("backend/sdk/typescript/src/epicode.ts"));
    assert!(
        calls.len() >= 30,
        "SDK call extraction broke ({})",
        calls.len()
    );
    let mut bad = Vec::new();
    for c in &calls {
        // Public base is https://host/api/v1 -> backend /v1/...; root clients use /api.
        let backend = if c.root || c.path == "/health" {
            c.path.clone()
        } else {
            format!("/v1{}", c.path)
        };
        let ok = routes
            .get(&norm(&backend))
            .is_some_and(|ms| ms.contains(&c.method));
        if !ok {
            bad.push(format!(
                "{}: {} {} (backend {backend})",
                c.file, c.method, c.path
            ));
        }
    }
    assert!(
        bad.is_empty(),
        "SDK calls with no backend route:\n{}",
        bad.join("\n")
    );
}

#[test]
fn sdk_mcp_calls_use_live_tools_and_params() {
    let catalog = epicode::engine::mcp::McpHandler::tool_catalog();
    let tools: BTreeMap<String, BTreeSet<String>> = catalog["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| {
            let props = t["inputSchema"]["properties"]
                .as_object()
                .map(|o| o.keys().cloned().collect())
                .unwrap_or_default();
            (t["name"].as_str().unwrap().to_string(), props)
        })
        .collect();
    let mut checked = 0;
    let mut bad = Vec::new();
    for (file, needle) in [
        ("backend/sdk/python/epicode/client.py", "self._mcp_call("),
        ("backend/sdk/typescript/src/epicode.ts", "this.mcpCall("),
    ] {
        let src = read(file);
        for c in src.split(needle).skip(1) {
            let Some(tool) = literal(c) else { continue };
            checked += 1;
            let Some(props) = tools.get(&tool) else {
                bad.push(format!("{file}: MCP tool {tool} does not exist"));
                continue;
            };
            // argument object literal: keys before ':' at depth 1
            let open = c.find('{').unwrap();
            let close = open + c[open..].find('}').unwrap();
            for part in c[open + 1..close].split(',') {
                let Some(k) = part.split(':').next() else {
                    continue;
                };
                let k = k.trim().trim_matches(|ch| ch == '"' || ch == '\'');
                if !k.is_empty() && !props.contains(k) {
                    bad.push(format!("{file}: {tool}({k}) — no such parameter"));
                }
            }
        }
    }
    assert!(checked >= 2, "MCP call extraction broke");
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}
