"""Source-level contract checks shared by the published SDKs and Cloud API."""

from __future__ import annotations

import json
from pathlib import Path
import sys
import unittest


ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "backend" / "sdk" / "python"))

from epicode import EpicodeAdmin, EpicodeClient  # noqa: E402


CLOUD_SOURCE = (ROOT / "backend" / "src" / "bin" / "cloud.rs").read_text(
    encoding="utf-8"
)
OPENAPI = (ROOT / "backend" / "docs" / "openapi.yaml").read_text(encoding="utf-8")
PYTHON_CLIENT = (
    ROOT / "backend" / "sdk" / "python" / "epicode" / "client.py"
).read_text(encoding="utf-8")
PYTHON_ADMIN = (ROOT / "backend" / "sdk" / "python" / "epicode" / "admin.py").read_text(
    encoding="utf-8"
)
TYPESCRIPT_CLIENT = (
    ROOT / "backend" / "sdk" / "typescript" / "src" / "epicode.ts"
).read_text(encoding="utf-8")


class RecordingResponse:
    def __init__(self, body: dict[str, object]) -> None:
        self.status_code = 200
        self.text = ""
        self._body = body

    def json(self) -> dict[str, object]:
        return self._body


class RecordingSession:
    def __init__(self, responses: list[dict[str, object]]) -> None:
        self.headers: dict[str, str] = {}
        self.calls: list[tuple[str, str, dict[str, object]]] = []
        self._responses = responses

    def request(self, method: str, url: str, **kwargs: object) -> RecordingResponse:
        self.calls.append((method, url, kwargs))
        return RecordingResponse(self._responses.pop(0))


class SdkCloudContractTests(unittest.TestCase):
    def test_documented_sdk_paths_have_cloud_v1_routes(self) -> None:
        paths = (
            "/health",
            "/register",
            "/remember",
            "/search",
            "/recall",
            "/ask",
            "/nodes",
            "/knowledge",
            "/stats",
            "/timeline",
            "/identity/step",
            "/identity/finalize",
            "/mcp",
            "/admin/users",
            "/admin/stats",
        )

        for path in paths:
            with self.subTest(path=path):
                self.assertIn(f'.route("/v1{path}"', CLOUD_SOURCE)
                self.assertIn(f"\n  {path}:\n", OPENAPI)

    def test_sdk_uses_the_supported_identity_and_mcp_contracts(self) -> None:
        for source in (PYTHON_CLIENT, TYPESCRIPT_CLIENT):
            for unsupported_path in (
                "/recall/tiers",
                "/dream/cycle",
                "/knowledge-graph/",
            ):
                with self.subTest(source=source[:20], path=unsupported_path):
                    self.assertNotIn(unsupported_path, source)

        self.assertIn('payload = {"step": step, "value": value}', PYTHON_CLIENT)
        self.assertIn("{ step, value }", TYPESCRIPT_CLIENT)
        self.assertIn('"/mcp"', PYTHON_CLIENT)
        self.assertIn('"/mcp"', TYPESCRIPT_CLIENT)

    def test_admin_registration_includes_required_password(self) -> None:
        self.assertIn('"password": password', PYTHON_ADMIN)
        self.assertIn("{ user_id: userId, password, plan }", TYPESCRIPT_CLIENT)
        self.assertIn("required: [user_id, password]", OPENAPI)

    def test_python_sdk_sends_cloud_request_bodies(self) -> None:
        session = RecordingSession(
            [
                {
                    "success": True,
                    "step": 1,
                    "progress": {"completed": 1, "total": 5, "current_step": 2},
                    "next_prompt": "Mission",
                    "pending": {},
                },
                {"jsonrpc": "2.0", "id": 1, "result": {"status": "complete"}},
                {
                    "success": True,
                    "query": "project context",
                    "memory_file": {},
                    "seed_count": 0,
                    "associated_count": 0,
                    "total_fragments": 0,
                    "emotion": {},
                },
            ]
        )
        client = EpicodeClient("api-key", session=session)

        client.identity_step(1, "Aurora")
        client.dream_cycle()
        client.recall("project context", depth=2)

        self.assertEqual(
            (
                "POST",
                "http://localhost:8080/api/v1/identity/step",
                {"json": {"step": 1, "value": "Aurora"}, "timeout": 30},
            ),
            session.calls[0],
        )
        self.assertEqual("http://localhost:8080/api/v1/mcp", session.calls[1][1])
        self.assertEqual(
            {
                "jsonrpc": "2.0",
                "id": 1,
                "method": "tools/call",
                "params": {"name": "dream_cycle", "arguments": {}},
            },
            session.calls[1][2]["json"],
        )
        self.assertEqual(
            {"query": "project context", "depth": 2}, session.calls[2][2]["json"]
        )

    def test_python_admin_registration_sends_password(self) -> None:
        session = RecordingSession(
            [
                {
                    "success": True,
                    "user_id": "alice",
                    "api_key": "tm-test",
                    "plan": "pro",
                    "max_memories": 1000,
                }
            ]
        )
        admin = EpicodeAdmin("admin-key", session=session)

        admin.register("alice", "a-secure-password", plan="pro")

        self.assertEqual(
            (
                "POST",
                "http://localhost:8080/api/v1/register",
                {
                    "json": {
                        "user_id": "alice",
                        "password": "a-secure-password",
                        "plan": "pro",
                    },
                    "timeout": 30,
                },
            ),
            session.calls[0],
        )

    def test_sdk_versions_match(self) -> None:
        python_project = (
            ROOT / "backend" / "sdk" / "python" / "pyproject.toml"
        ).read_text(encoding="utf-8")
        python_module = (
            ROOT / "backend" / "sdk" / "python" / "epicode" / "__init__.py"
        ).read_text(encoding="utf-8")
        typescript_package = json.loads(
            (ROOT / "backend" / "sdk" / "typescript" / "package.json").read_text(
                encoding="utf-8"
            )
        )
        python_readme = (ROOT / "backend" / "sdk" / "python" / "README.md").read_text(
            encoding="utf-8"
        )
        typescript_readme = (
            ROOT / "backend" / "sdk" / "typescript" / "README.md"
        ).read_text(encoding="utf-8")

        self.assertIn('version = "1.0.2"', python_project)
        self.assertIn('__version__ = "1.0.2"', python_module)
        self.assertEqual("1.0.2", typescript_package["version"])
        self.assertIn("v1.0.2", python_readme)
        self.assertIn("v1.0.2", typescript_readme)
        self.assertIn("identity_finalize()", python_readme)
        self.assertIn("identityFinalize()", typescript_readme)


if __name__ == "__main__":
    unittest.main()
