import json
import os
import urllib.request


BASE_URL = os.environ.get("EPICODE_BASE_URL", "http://localhost:8080/api/v1")
API_KEY = os.environ.get("EPICODE_API_KEY")

if not API_KEY:
    raise SystemExit("Set EPICODE_API_KEY before running this example.")


def api(path: str, body: dict) -> dict:
    req = urllib.request.Request(
        f"{BASE_URL}{path}",
        data=json.dumps(body).encode("utf-8"),
        headers={
            "Content-Type": "application/json",
            "X-API-Key": API_KEY,
        },
        method="POST",
    )
    with urllib.request.urlopen(req) as resp:
        return json.loads(resp.read().decode("utf-8"))


remembered = api("/remember", {"content": "Python example stored an operational memory"})
print("remember:", remembered)

search = api("/search", {"query": "operational memory", "limit": 3})
print("search:", search)

answer = api("/ask", {"question": "What did the Python example store?", "depth": 2})
print("ask:", answer)
