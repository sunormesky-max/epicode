//! 一致性契约测试(2026-09-28迭代): 守住三个一致性面
//! 1. 前后端一致: 前端api.ts调用的每个端点必须存在于后端路由注册表
//! 2. MCP服务一致: tools_list声明的每个工具必须有dispatch分支(反之亦然)
//! 3. SMRP信封一致: MCP与REST共用信封构造, 关键字段不漂移
//!
//! 这些测试读源码做静态契约(而非运行时) — 破坏一致性时CI先红, 不等线上404。

use std::collections::BTreeSet;
use std::path::Path;

/// 字符串常量(避开测试源码里的引号转义地狱)
const COLON_PARAM: &str = "/:";
const PARAM_ID: &str = "/:id";

fn read(rel: &str) -> String {
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    std::fs::read_to_string(base.join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

/// 从 cloud.rs 提取全部已注册路由(含跨行格式) — 字节扫描, 无引号字面量
fn backend_routes() -> BTreeSet<String> {
    let src = read("backend/src/bin/cloud.rs");
    let b = src.as_bytes();
    let needle = b".route(";
    let mut out = BTreeSet::new();
    let mut i = 0;
    while i + needle.len() <= b.len() {
        if &b[i..i + needle.len()] == needle {
            let mut j = i + needle.len();
            while j < b.len() && (b[j] == 0x20 || b[j] == 0x0a || b[j] == 0x0d || b[j] == 0x09) {
                j += 1;
            }
            if j < b.len() && b[j] == b'"' {
                let start = j + 1;
                let mut k = start;
                while k < b.len() && b[k] != b'"' {
                    k += 1;
                }
                if k < b.len() {
                    out.insert(src[start..k].to_string());
                }
            }
            i = j;
        }
        i += 1;
    }
    out
}

fn extract_frontend_endpoints(api: &str) -> BTreeSet<String> {
    // 无字符字面量写法: 按 request 定位, 在其后120字符窗口里找 '/...' 单引号串
    let mut called = BTreeSet::new();
    let b = api.as_bytes();
    let needle = b"request";
    let mut i = 0;
    while i + 7 <= b.len() {
        if &b[i..i + 7] == needle {
            let win_end = (i + 120).min(b.len());
            let mut j = i + 7;
            while j < win_end {
                if b[j] == 0x27 {
                    // 找配对单引号
                    if let Some(rel_end) = api[j + 1..win_end]
                        .as_bytes()
                        .iter()
                        .position(|&c| c == 0x27)
                    {
                        let inner = &api[j + 1..j + 1 + rel_end];
                        if inner.starts_with('/') && !inner.contains(' ') {
                            called.insert(inner.to_string());
                        }
                    }
                    break;
                }
                j += 1;
            }
            i += 7;
        }
        i += 1;
    }
    called
}

#[test]
fn frontend_api_calls_have_backend_routes() {
    let api = read("frontend/src/lib/api.ts");
    let called = extract_frontend_endpoints(&api);
    assert!(!called.is_empty(), "前端端点提取失败(检查api.ts格式)");

    let routes = backend_routes();
    let mut missing = Vec::new();
    for c in &called {
        let hit = routes.iter().any(|r| {
            r == c
                || r.split(COLON_PARAM).next() == Some(c.as_str())
                || c.starts_with(&format!("{}/", r.trim_end_matches(PARAM_ID)))
        });
        if !hit {
            missing.push(c.clone());
        }
    }
    assert!(
        missing.is_empty(),
        "前端调用但后端无路由(前后端断链): {missing:?}
后端路由数: {}",
        routes.len()
    );
}

#[test]
fn mcp_tools_list_matches_dispatch() {
    let src = read("backend/src/engine/mcp.rs");
    // dispatch分支: "name" => self.tool_xxx(
    let dispatch: BTreeSet<String> = src
        .lines()
        .filter_map(|l| {
            let l = l.trim();
            l.strip_prefix('"')
                .and_then(|r| r.split('"').next())
                .filter(|name| l.contains("=> self.tool_") && !name.contains('/'))
                .map(String::from)
        })
        .collect();
    // tools_list声明: "name": "xxx"
    let tl_start = src.find("fn tools_list").expect("tools_list not found");
    let tl_body = &src[tl_start..tl_start + 60_000.min(src.len() - tl_start)];
    let declared: BTreeSet<String> = tl_body
        .match_indices("\"name\": \"")
        .map(|(i, _)| {
            let s = i + 8;
            let e = tl_body[s..].find('"').map(|e| s + e).unwrap_or(s);
            tl_body[s..e].to_string()
        })
        .collect();

    assert!(!dispatch.is_empty() && !declared.is_empty(), "提取失败");
    let undeclared: Vec<_> = dispatch.difference(&declared).collect();
    let unimplemented: Vec<_> = declared.difference(&dispatch).collect();
    assert!(
        undeclared.is_empty() && unimplemented.is_empty(),
        "MCP工具清单不一致 — dispatch有未声明: {undeclared:?}, 声明无dispatch: {unimplemented:?}"
    );
}

#[test]
fn smrp_envelope_shared_fields_present() {
    let smrp = read("backend/src/engine/smrp.rs");
    // 信封关键字段: 成功信封必须携带的结构
    for field in ["structure_version", "status", "ttl_ms"] {
        let snake = field;
        assert!(
            smrp.contains(snake) || smrp.contains(&field.replace("_ms", "Ms")),
            "SMRP信封缺关键字段: {field}"
        );
    }
    // REST与MCP共用层存在(§1.3 承诺)
    assert!(smrp.contains("envelope_ok"), "SMRP共用信封构造缺失");
}
