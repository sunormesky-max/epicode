from __future__ import annotations

import sys
from pathlib import Path
from typing import Any

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from epicode import EpicodeClient, EpicodeError


def smrp(data: dict[str, Any], *, ok: bool = True) -> dict[str, Any]:
    return {
        "protocol": {
            "schema_version": "1.0",
            "tool": "memory_search",
            "ok": ok,
            "error": None if ok else {"code": 400, "message": "query rejected"},
        },
        "data": data if ok else None,
        "status": {"identity": {"system": "Epicode"}, "space": {"memories": 1}},
    }


def test_search_unwraps_smrp_and_preserves_retrieval_context(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    client = EpicodeClient("key")
    response = smrp(
        {
            "query": "deployment",
            "results": [
                {
                    "id": 7,
                    "content": "Deployed v2",
                    "labels": ["ops"],
                    "similarity": 0.82,
                    "tier": "experiential",
                    "source": ["vector", "bm25"],
                    "metrics": {"importance": 0.7},
                }
            ],
            "tiers": {"experiential": []},
            "total_found": 1,
            "offset": 5,
            "limit": 10,
            "score_notes": {"base": "hybrid"},
        }
    )
    calls: list[tuple[str, str, dict[str, Any]]] = []

    def fake_request(method: str, path: str, **kwargs: Any) -> dict[str, Any]:
        calls.append((method, path, kwargs))
        return response

    monkeypatch.setattr(client, "_request", fake_request)
    result = client.search(
        "deployment",
        limit=10,
        offset=5,
        labels=["ops"],
        mode="fusion",
    )

    assert calls == [
        (
            "POST",
            "/search",
            {
                "json": {
                    "query": "deployment",
                    "limit": 10,
                    "offset": 5,
                    "labels": ["ops"],
                    "mode": "fusion",
                }
            },
        )
    ]
    assert result.success
    assert result.total == 1
    assert result.results[0].tier == "experiential"
    assert result.results[0].source == ["vector", "bm25"]
    assert result.smrp is not None
    assert result.smrp.status["identity"]["system"] == "Epicode"


def test_remember_unwraps_smrp_and_forwards_labels(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    client = EpicodeClient("key")
    response = smrp({"id": 12, "status": "created", "labels": ["ops"]})
    calls: list[tuple[str, str, dict[str, Any]]] = []

    def fake_request(method: str, path: str, **kwargs: Any) -> dict[str, Any]:
        calls.append((method, path, kwargs))
        return response

    monkeypatch.setattr(client, "_request", fake_request)
    result = client.remember("Deployed v2", labels=["ops"])

    assert calls == [
        ("POST", "/remember", {"json": {"content": "Deployed v2", "labels": ["ops"]}})
    ]
    assert result.success
    assert result.id == "12"
    assert result.labels == ["ops"]
    assert result.smrp is not None


def test_recall_unwraps_smrp_tiers_and_emotion(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    client = EpicodeClient("key")
    response = smrp(
        {
            "query": "deployment",
            "depth": 2,
            "seed_count": 1,
            "associated_count": 1,
            "total_fragments": 2,
            "emotion": {"pleasure": 0.2, "arousal": 0.3, "dominance": 0.4},
            "tiers": {"primary": [], "contextual": []},
        }
    )
    monkeypatch.setattr(
        client, "_request", lambda *_args, **_kwargs: response
    )

    result = client.recall("deployment")

    assert result.success
    assert result.seed_count == 1
    assert result.associated_count == 1
    assert result.emotion.arousal == 0.3
    assert result.smrp is not None


def test_ask_exposes_structured_memories_and_legacy_responses(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    client = EpicodeClient("key")
    response = smrp(
        {
            "question": "What happened?",
            "answer": "A grounded answer.",
            "memory_count": 1,
            "memories": [
                {"id": 12, "labels": ["ops"], "content": "Deployed v2", "relevance": 0.9}
            ],
            "knowledge_card_used": "deployment",
        }
    )
    monkeypatch.setattr(
        client, "_request", lambda *_args, **_kwargs: response
    )

    result = client.ask("What happened?")

    assert result.answer == "A grounded answer."
    assert result.memories[0].id == 12
    assert result.memories[0].content == "Deployed v2"
    assert result.knowledge_card_used == "deployment"
    assert result.smrp is not None


def test_smrp_errors_raise_instead_of_becoming_false_success(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    client = EpicodeClient("key")
    response = {
        "protocol": {
            "schema_version": "1.0",
            "tool": "memory_search",
            "ok": False,
            "error": {"code": 400, "message": "query rejected"},
        },
        "data": None,
        "status": {"identity": {"system": "Epicode"}, "space": {"memories": 1}},
    }
    monkeypatch.setattr(
        client, "_request", lambda *_args, **_kwargs: response
    )

    with pytest.raises(EpicodeError, match="query rejected") as error:
        client.search("invalid")

    assert error.value.status_code == 200
    assert error.value.response_body == response


def test_legacy_flat_response_remains_supported(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    client = EpicodeClient("key")
    monkeypatch.setattr(
        client,
        "_request",
        lambda *_args, **_kwargs: {
            "success": True,
            "results": [{"id": "old-id", "content": "legacy", "labels": ["old"]}],
            "total": 1,
        },
    )

    result = client.search("legacy")

    assert result.success
    assert result.results[0].id == "old-id"
    assert result.smrp is None
