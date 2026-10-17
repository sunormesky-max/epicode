"""SDK methods that previously called non-existent routes or sent wrong fields.

Payloads mirror the real backend handlers (backend/src/bin/cloud/*.rs and the
MCP dream_cycle tool); backend/tests/sdk_contract.rs checks every SDK route
against the live route table so these cannot drift silently.
"""

from __future__ import annotations

import sys
import warnings
from pathlib import Path
from typing import Any

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from epicode import EpicodeClient, EpicodeError


def smrp(data: dict[str, Any], tool: str = "memory_recall") -> dict[str, Any]:
    return {
        "protocol": {"schema_version": "1.0", "tool": tool, "ok": True, "error": None},
        "data": data,
        "status": {"identity": {"system": "Epicode"}, "space": {"memories": 1}},
    }


def capture(monkeypatch: pytest.MonkeyPatch, client: EpicodeClient, response: dict[str, Any]):
    calls: list[tuple[str, str, dict[str, Any]]] = []

    def fake_request(method: str, path: str, **kwargs: Any) -> dict[str, Any]:
        calls.append((method, path, kwargs))
        return response

    monkeypatch.setattr(client, "_request", fake_request)
    return calls


def test_identity_step_sends_value(monkeypatch: pytest.MonkeyPatch) -> None:
    client = EpicodeClient("key")
    calls = capture(
        monkeypatch,
        client,
        smrp(
            {
                "step": 1,
                "progress": {"completed": 1, "total": 5, "current_step": 2},
                "next_prompt": "Why was I created?",
                "pending": {"has_name": True},
            },
            "identity_step",
        ),
    )
    r = client.identity_step(1, "Ada")
    assert calls == [("POST", "/identity/step", {"json": {"step": 1, "value": "Ada"}})]
    assert r.success and r.step == 1 and r.progress["current_step"] == 2
    assert r.next_prompt == "Why was I created?"

    with warnings.catch_warnings(record=True) as w:
        warnings.simplefilter("always")
        client.identity_step(1, agent_name="Ada")
    assert calls[-1][2] == {"json": {"step": 1, "value": "Ada"}}
    assert any(issubclass(x.category, DeprecationWarning) for x in w)


def test_recall_with_tiers_uses_recall_route_and_real_tiers(monkeypatch: pytest.MonkeyPatch) -> None:
    client = EpicodeClient("key")
    item = {"id": 3, "content": "uses parking_lot", "labels": ["rust"], "tier": "primary",
            "source": ["vector"], "similarity": 0.91, "metrics": {"importance": 0.5}}
    calls = capture(
        monkeypatch,
        client,
        smrp(
            {
                "query": "rust locks",
                "depth": 2,
                "tiers": {"primary": [item], "contextual": [], "experiential": [], "hub": [dict(item, id=4, tier="hub")]},
                "emotion": {"pleasure": 0.2, "arousal": 0.1, "dominance": 0.0},
            }
        ),
    )
    r = client.recall_with_tiers("rust locks")
    assert calls == [("POST", "/recall", {"json": {"query": "rust locks", "depth": 2}})]
    assert [len(t) for t in r.tiers] == [1, 0, 0, 1]
    assert r.tiers[0][0].id == "3" and r.tiers[0][0].tier == 1 and r.tiers[0][0].tier_name == "primary"
    assert r.tiers[3][0].tier == 4 and r.tiers[3][0].labels == ["rust"]
    assert r.tiers[0][0].emotional_valence.pleasure == 0.2
    assert r.total_results == 2 and r.smrp is not None


def test_knowledge_sends_integer_id_and_unwraps(monkeypatch: pytest.MonkeyPatch) -> None:
    client = EpicodeClient("key")
    calls = capture(
        monkeypatch,
        client,
        smrp({"id": 7, "relations": [{"target": 9, "type": "Semantic", "strength": 0.8}], "count": 1},
             "knowledge_relations"),
    )
    k = client.knowledge("7")
    assert calls == [("POST", "/knowledge", {"json": {"id": 7}})]
    assert k.success and k.count == 1 and k.relations[0]["target"] == 9

    g = client.knowledge_graph(7)
    assert calls[-1] == ("POST", "/knowledge", {"json": {"id": 7}})
    assert [n.id for n in g.nodes] == ["7", "9"]
    assert g.edges[0].source == "7" and g.edges[0].target == "9"
    assert g.edges[0].relation == "Semantic" and g.edges[0].strength == 0.8


def test_timeline_get_node_create_node_unwrap_smrp(monkeypatch: pytest.MonkeyPatch) -> None:
    client = EpicodeClient("key")
    capture(monkeypatch, client, smrp({"events": [{"id": 1}], "total": 1}, "timeline"))
    assert client.timeline().total == 1
    capture(monkeypatch, client, smrp({"id": 5, "content": "x", "labels": ["a"]}, "memory_get"))
    n = client.get_node(5)
    assert n.success and n.id == "5" and n.labels == ["a"]
    calls = capture(monkeypatch, client, smrp({"id": 6}, "node_create"))
    c = client.create_node("x", timestamp="1792030000")
    assert calls[0][2] == {"json": {"content": "x", "timestamp": 1792030000}}
    assert c.success and c.id == "6"


class FakeResp:
    def __init__(self, body: dict[str, Any], status: int = 200) -> None:
        self.status_code = status
        self._body = body
        self.text = ""

    def json(self) -> dict[str, Any]:
        return self._body


class FakeSession:
    def __init__(self, body: dict[str, Any]) -> None:
        self.body = body
        self.calls: list[tuple[str, str, dict[str, Any]]] = []

    def request(self, method: str, url: str, **kwargs: Any) -> FakeResp:
        self.calls.append((method, url, kwargs))
        return FakeResp(self.body)


def test_dream_cycle_calls_mcp_tool() -> None:
    report = "consolidated: 4, connections_formed: 7, merged: 1, evicted: 2, importance_updated: 30, insights: []"
    envelope = smrp({"report": report, "dry_run": True}, "dream_cycle")
    session = FakeSession({"jsonrpc": "2.0", "id": 1, "result": {"structuredContent": envelope, "content": []}})
    client = EpicodeClient("key", base_url="https://epicode.cn/api/v1", session=session)  # type: ignore[arg-type]
    r = client.dream_cycle(dry_run=True)
    method, url, kwargs = session.calls[0]
    assert (method, url) == ("POST", "https://epicode.cn/api/mcp")
    assert kwargs["json"]["method"] == "tools/call"
    assert kwargs["json"]["params"] == {"name": "dream_cycle", "arguments": {"dry_run": True}}
    assert kwargs["headers"]["X-API-Key"] == "key"
    assert (r.memories_consolidated, r.new_associations, r.duplicates_merged, r.junk_evicted) == (4, 7, 1, 2)
    assert r.importance_updated == 30 and r.dry_run and r.cycles_completed == 0 and r.report == report


def test_dream_cycle_jsonrpc_error_raises() -> None:
    session = FakeSession({"jsonrpc": "2.0", "id": 1, "error": {"code": -32602, "message": "bad"}})
    client = EpicodeClient("key", session=session)  # type: ignore[arg-type]
    with pytest.raises(EpicodeError):
        client.dream_cycle()
    assert session.calls[0][1] == "http://localhost:8080/api/mcp"


def test_admin_routes_live_at_api_root() -> None:
    from epicode import EpicodeAdmin

    session = FakeSession({"success": True, "total_users": 2, "active_engines": 1})
    admin = EpicodeAdmin("ak", base_url="https://epicode.cn/api/v1", session=session)  # type: ignore[arg-type]
    admin.list_users()
    assert session.calls[0][1] == "https://epicode.cn/api/admin/users"
    assert session.calls[0][2]["headers"]["X-Admin-Key"] == "ak"
