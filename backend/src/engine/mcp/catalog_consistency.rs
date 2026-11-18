use super::McpHandler;
use std::collections::BTreeSet;

fn repo_file(rel: &str) -> String {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join(rel);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

fn names(catalog: &serde_json::Value) -> Vec<String> {
    catalog["tools"]
        .as_array()
        .expect("tools array")
        .iter()
        .map(|t| t["name"].as_str().expect("tool name").to_string())
        .collect()
}

/// Quoted identifiers between `start` and the first `end` after it.
fn quoted_between(src: &str, start: &str, end: &str) -> BTreeSet<String> {
    let s = src
        .find(start)
        .unwrap_or_else(|| panic!("{start} not found"));
    let e = s + src[s..]
        .find(end)
        .unwrap_or_else(|| panic!("{end} not found"));
    src[s..e]
        .split('"')
        .skip(1)
        .step_by(2)
        .filter(|w| !w.is_empty() && w.chars().all(|c| c.is_ascii_lowercase() || c == '_'))
        .map(String::from)
        .collect()
}

#[test]
fn registry_dispatch_permission_annotation_agree() {
    // Refactor(modularize-engine): the registry now lives in sibling files;
    // dispatch arms in dispatch.rs, permission/annotation maps in annotations.rs.
    let src = include_str!("dispatch.rs");
    let full = names(&McpHandler::tool_catalog());
    let listed: BTreeSet<String> = full.iter().cloned().collect();
    assert_eq!(listed.len(), full.len(), "duplicate tool names in catalog");

    let dispatch: BTreeSet<String> = src
        .lines()
        .map(str::trim)
        .filter(|l| l.starts_with('"') && l.contains("=> self.tool_"))
        .filter_map(|l| l[1..].split('"').next().map(String::from))
        .collect();
    assert_eq!(listed, dispatch, "tools/list vs tools/call dispatch drift");

    for n in &listed {
        assert!(
            McpHandler::tool_permission(n).is_some(),
            "{n}: listed but no permission (subaccounts get 403)"
        );
        assert!(
            McpHandler::tool_annotation_class(n).is_some(),
            "{n}: listed but no explicit annotation class"
        );
    }
    let perm = quoted_between(
        include_str!("annotations.rs"),
        "fn tool_permission(",
        "_ => None",
    );
    let stale: Vec<_> = perm.difference(&listed).collect();
    assert!(
        stale.is_empty(),
        "permission map names non-existent MCP tools: {stale:?}"
    );
    let ann = quoted_between(
        include_str!("annotations.rs"),
        "fn tool_annotation_class(",
        "_ => return None",
    );
    let stale: Vec<_> = ann.difference(&listed).collect();
    assert!(
        stale.is_empty(),
        "annotation map names non-existent MCP tools: {stale:?}"
    );
}

#[test]
fn schemas_are_well_formed() {
    for t in McpHandler::tool_catalog()["tools"].as_array().unwrap() {
        let n = t["name"].as_str().unwrap();
        let schema = &t["inputSchema"];
        assert_eq!(schema["type"], "object", "{n}: inputSchema.type");
        let props = schema["properties"].as_object();
        for r in schema["required"].as_array().into_iter().flatten() {
            let r = r.as_str().unwrap();
            assert!(
                props.is_some_and(|p| p.contains_key(r)),
                "{n}: required `{r}` not in properties"
            );
        }
    }
}

#[test]
fn advertised_catalog_matches_registry() {
    let public = names(&McpHandler::tool_catalog());
    assert_eq!(McpHandler::public_tool_count(), public.len());

    let disc: serde_json::Value =
        serde_json::from_str(&repo_file(".well-known/mcp/discovery.json")).expect("discovery.json");
    let advertised: Vec<String> = names(&disc);
    assert_eq!(
        advertised, public,
        ".well-known/mcp/discovery.json drifted from tools/list"
    );
    // Full-object equality: descriptions, input schemas (params/defaults/enums)
    // and annotations must match too — names alone let schemas drift.
    let live = McpHandler::tool_catalog();
    for (d, l) in disc["tools"]
        .as_array()
        .unwrap()
        .iter()
        .zip(live["tools"].as_array().unwrap())
    {
        for key in ["description", "inputSchema", "annotations"] {
            assert_eq!(
                d[key], l[key],
                "discovery.json {key} of {} drifted from tools/list — run \
                     `cargo test --lib regenerate_discovery_manifest -- --ignored`",
                l["name"]
            );
        }
    }

    // Marketing / docs copy: any "<N> MCP tools" / "<N> 个 MCP 工具" claim must not
    // exceed the real public count, and an exact claim must equal it.
    let count = public.len();
    for rel in [
        "README.md",
        "README.zh.md",
        "frontend/index.html",
        "frontend/public/llms.txt",
        "frontend/src/i18n/translations.ts",
        "frontend/src/lib/page-meta.ts",
        "mcp-bridge/README.md",
    ] {
        let text = repo_file(rel);
        for (i, _) in text.match_indices("MCP") {
            let before = text[..i].trim_end().trim_end_matches('个').trim_end();
            let digits: String = before
                .trim_end_matches('+')
                .chars()
                .rev()
                .take_while(|c| c.is_ascii_digit())
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            let after = &text[i + 3..];
            let about_tools =
                after.trim_start().starts_with("tool") || after.trim_start().starts_with("工具");
            if digits.is_empty() || !about_tools {
                continue;
            }
            let n: usize = digits.parse().unwrap();
            if before.ends_with('+') {
                assert!(n <= count, "{rel}: claims {n}+ MCP tools, actual {count}");
            } else {
                assert_eq!(n, count, "{rel}: claims {n} MCP tools, actual {count}");
            }
        }
    }
}

/// Regenerate `.well-known/mcp/discovery.json` from the live registry:
/// `cargo test --lib regenerate_discovery_manifest -- --ignored`
#[test]
#[ignore]
fn regenerate_discovery_manifest() {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../.well-known/mcp/discovery.json");
    let mut disc: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    disc["tools"] = McpHandler::tool_catalog()["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| {
            serde_json::json!({
                "name": t["name"],
                "description": t["description"],
                "inputSchema": t["inputSchema"],
                "annotations": t["annotations"],
            })
        })
        .collect();
    std::fs::write(&path, serde_json::to_string_pretty(&disc).unwrap() + "\n").unwrap();
}

#[test]
fn advertised_resources_are_readable() {
    let h = McpHandler::new(std::sync::Arc::new(super::tests::isolated_engine()));
    let call = |method: &str, params: serde_json::Value| {
        h.handle(super::McpRequest {
            jsonrpc: "2.0".into(),
            id: Some(serde_json::json!(1)),
            method: method.into(),
            params: Some(params),
        })
    };
    let init = call("initialize", serde_json::json!({}));
    let caps = &init.result.expect("initialize")["capabilities"];
    for cap in caps.as_object().unwrap().keys() {
        let probe = match cap.as_str() {
            "tools" => "tools/list",
            "resources" => "resources/list",
            "prompts" => "prompts/list",
            other => panic!("capability {other} has no probe; add one"),
        };
        let r = call(probe, serde_json::json!({}));
        assert!(
            r.error.is_none(),
            "capability {cap} advertised but {probe} fails"
        );
    }
    let list = call("resources/list", serde_json::json!({}));
    let listed: Vec<String> = list.result.unwrap()["resources"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["uri"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(listed, McpHandler::RESOURCE_URIS);
    for uri in &listed {
        let r = call("resources/read", serde_json::json!({ "uri": uri }));
        assert!(r.error.is_none(), "{uri}: {:?}", r.error.map(|e| e.message));
        let c = &r.result.unwrap()["contents"][0];
        assert_eq!(c["uri"], uri.as_str());
        let parsed: serde_json::Value = serde_json::from_str(c["text"].as_str().unwrap()).unwrap();
        assert_eq!(parsed["protocol"]["ok"], true, "{uri}: {parsed}");
    }
    let missing = call(
        "resources/read",
        serde_json::json!({ "uri": "epicode://nope" }),
    );
    assert_eq!(missing.error.unwrap().code, -32002);
    let disc: serde_json::Value =
        serde_json::from_str(&repo_file(".well-known/mcp/discovery.json")).unwrap();
    let disc_uris: Vec<&str> = disc["resources"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["uri"].as_str().unwrap())
        .collect();
    assert_eq!(disc_uris, McpHandler::RESOURCE_URIS);
}
