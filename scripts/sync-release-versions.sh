#!/usr/bin/env bash
# scripts/sync-release-versions.sh — 发版配套(审计三轮固化):
# release-please 只更新 extra-files 里声明的文件, 不碰 Cargo.lock(--locked 必挂)
# 和部分 generic 漏网文件. 本脚本在 release 分支上运行, 把全部版本位对齐.
# 用法: 在 release-please--branches--main 分支上执行, 然后提交推送.
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

V=$(tr -d '[:space:]' < version.txt)
echo "syncing all version files to ${V}"

python3 - "$V" <<'PYEOF'
import json, re, sys
V = sys.argv[1]
OLD = None
# 推断旧版本: manifest 已是新版, 从 Cargo.lock 的 epicode 包读? 直接全按 V 写,
# 但 Cargo.lock 需要先读当前值 — 逐项按精确模式替换任意旧版本号
def bump(path, pairs, regex=False, flags=0):
    s = open(path, encoding="utf-8").read()
    changed = False
    for pat, rep in pairs:
        s2, n = re.subn(pat, rep, s, flags=flags)
        if n: changed = True
        s = s2
    open(path, "w", encoding="utf-8", newline="\n").write(s)
    print(("done " if changed else "skip ") + path)

VER = r"\d+\.\d+\.\d+"
bump("backend/sdk/python/pyproject.toml", [(r'version = "' + VER + '"', f'version = "{V}"')])
bump("backend/sdk/python/epicode/__init__.py", [(r'__version__ = "' + VER + '"', f'__version__ = "{V}"')])
bump("backend/docs/openapi.yaml", [(r"^(  version:) " + VER, r"\1 " + V)], flags=re.M)
bump("deploy/helm/epicode/Chart.yaml", [(r"^(version:) " + VER, r"\1 " + V), (r'^(appVersion:) "' + VER + '"', r'\1 "' + V + '"')], flags=re.M)
bump("deploy/helm/epicode/values.yaml", [(r'tag: "' + VER + '"', f'tag: "{V}"')])
bump("deploy/kubernetes/epicode.yaml", [(r"epicode-backend:" + VER, f"epicode-backend:{V}"), (r"epicode-frontend:" + VER, f"epicode-frontend:{V}")])
for lock in ["backend/Cargo.lock", "guard/Cargo.lock"]:
    try:
        s = open(lock, encoding="utf-8").read()
    except FileNotFoundError:
        continue
    for name in ["epicode", "epicode-cloud", "epicode-mcp", "epicode-bench", "epicode-migrate", "epicode-guard"]:
        s2 = re.sub(r'(name = "' + name + r'"\nversion = )' + VER, r'\g<1>' + V, s)
        s = s2
    open(lock, "w", encoding="utf-8", newline="\n").write(s)
    print("done " + lock)
for jp in ["frontend/package.json", "backend/sdk/typescript/package.json"]:
    pkg = json.load(open(jp, encoding="utf-8"))
    if pkg.get("version") != V:
        pkg["version"] = V
        open(jp, "w", encoding="utf-8", newline="\n").write(json.dumps(pkg, indent=2) + "\n")
        print("done " + jp)
for lp in ["frontend/package-lock.json", "backend/sdk/typescript/package-lock.json"]:
    try:
        lock = json.load(open(lp, encoding="utf-8"))
    except FileNotFoundError:
        continue
    if lock.get("version") != V:
        lock["version"] = V
        if lock.get("packages", {}).get(""):
            lock["packages"][""]["version"] = V
        open(lp, "w", encoding="utf-8", newline="\n").write(json.dumps(lock, indent=2) + "\n")
        print("done " + lp)
PYEOF

echo "verify:"
bash scripts/verify-version.sh
