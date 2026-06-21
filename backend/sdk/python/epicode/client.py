"""Epicode API client."""

from __future__ import annotations

from typing import Any

import requests

from epicode.exceptions import (
    AuthenticationError,
    NotFoundError,
    PlanLimitExceededError,
    RateLimitError,
    ServerError,
    EpicodeError,
    ValidationError,
)
from epicode.models import (
    AskResponse,
    CreateNodeResponse,
    Emotion,
    HealthResponse,
    KnowledgeResponse,
    NodeResponse,
    RecallResponse,
    RememberResponse,
    SearchResult,
    SearchResponse,
    StatsResponse,
    TimelineResponse,
)


class EpicodeClient:
    """High-level client for the Epicode API."""

    DEFAULT_BASE_URL = "http://localhost:9111"
    DEFAULT_TIMEOUT = 30

    def __init__(
        self,
        api_key: str,
        *,
        base_url: str | None = None,
        timeout: int | None = None,
        session: requests.Session | None = None,
    ) -> None:
        self._api_key = api_key
        self._base_url = (base_url or self.DEFAULT_BASE_URL).rstrip("/")
        self._timeout = timeout or self.DEFAULT_TIMEOUT
        self._session = session or requests.Session()
        self._session.headers.update({"X-API-Key": self._api_key, "Content-Type": "application/json"})

    def _request(self, method: str, path: str, **kwargs: Any) -> dict[str, Any]:
        url = f"{self._base_url}{path}"
        kwargs.setdefault("timeout", self._timeout)
        resp = self._session.request(method, url, **kwargs)
        return self._handle_response(resp)

    @staticmethod
    def _handle_response(resp: requests.Response) -> dict[str, Any]:
        code = resp.status_code
        try:
            body = resp.json()
        except ValueError:
            body = {}

        if 200 <= code < 300:
            return body

        message = body.get("error") or body.get("message") or resp.text or f"HTTP {code}"

        if code in (401, 403):
            raise AuthenticationError(message, status_code=code, response_body=body)
        if code == 404:
            raise NotFoundError(message, status_code=code, response_body=body)
        if code == 422:
            raise ValidationError(message, status_code=code, response_body=body)
        if code == 429:
            raise RateLimitError(message, status_code=code, response_body=body)
        if code == 507:
            raise PlanLimitExceededError(message, status_code=code, response_body=body)
        if code >= 500:
            raise ServerError(message, status_code=code, response_body=body)

        raise EpicodeError(message, status_code=code, response_body=body)

    def health(self) -> HealthResponse:
        """Check API health (no authentication required)."""
        data = self._request("GET", "/health")
        return HealthResponse(
            status=data.get("status", ""),
            version=data.get("version", ""),
            success=data.get("success", False),
        )

    def remember(self, content: str) -> RememberResponse:
        """Store a new memory."""
        data = self._request("POST", "/v1/remember", json={"content": content})
        return RememberResponse(
            success=data.get("success", False),
            id=data.get("id", ""),
            labels=data.get("labels", []),
        )

    def search(self, query: str, *, limit: int | None = None) -> SearchResponse:
        """Search memories by semantic similarity."""
        payload: dict[str, Any] = {"query": query}
        if limit is not None:
            payload["limit"] = limit
        data = self._request("POST", "/v1/search", json=payload)
        results = [
            SearchResult(
                id=r.get("id", ""),
                content=r.get("content", ""),
                labels=r.get("labels", []),
                similarity=r.get("similarity", 0.0),
            )
            for r in data.get("results", [])
        ]
        return SearchResponse(
            success=data.get("success", False),
            results=results,
            total=data.get("total", 0),
        )

    def recall(self, query: str, *, depth: int | None = None) -> RecallResponse:
        """Recall associative memories for a query."""
        payload: dict[str, Any] = {"query": query}
        if depth is not None:
            payload["depth"] = depth
        data = self._request("POST", "/v1/recall", json=payload)
        raw_emotion = data.get("emotion", {})
        emotion = Emotion(
            pleasure=raw_emotion.get("pleasure", 0.0),
            arousal=raw_emotion.get("arousal", 0.0),
            dominance=raw_emotion.get("dominance", 0.0),
        )
        return RecallResponse(
            success=data.get("success", False),
            query=data.get("query", ""),
            seed_count=data.get("seed_count", 0),
            total_fragments=data.get("total_fragments", 0),
            associated_count=data.get("associated_count", 0),
            emotion=emotion,
            memory_file=data.get("memory_file", ""),
        )

    def ask(self, question: str, *, depth: int | None = None) -> AskResponse:
        """Ask a question and receive an AI-generated answer grounded in memories."""
        payload: dict[str, Any] = {"question": question}
        if depth is not None:
            payload["depth"] = depth
        data = self._request("POST", "/v1/ask", json=payload)
        return AskResponse(
            success=data.get("success", False),
            question=data.get("question", ""),
            answer=data.get("answer", ""),
            memory_count=data.get("memory_count", 0),
            memories=data.get("memories", []),
        )

    def create_node(
        self,
        content: str,
        *,
        labels: list[str] | None = None,
        timestamp: str | None = None,
    ) -> CreateNodeResponse:
        """Create a knowledge graph node."""
        payload: dict[str, Any] = {"content": content}
        if labels is not None:
            payload["labels"] = labels
        if timestamp is not None:
            payload["timestamp"] = timestamp
        data = self._request("POST", "/v1/nodes", json=payload)
        return CreateNodeResponse(
            success=data.get("success", False),
            id=data.get("id", ""),
        )

    def get_node(self, node_id: str) -> NodeResponse:
        """Retrieve a knowledge graph node by ID."""
        data = self._request("GET", f"/v1/nodes/{node_id}")
        return NodeResponse(
            success=data.get("success", False),
            id=data.get("id", ""),
            content=data.get("content", ""),
            labels=data.get("labels", []),
        )

    def knowledge(self, id: str) -> KnowledgeResponse:
        """Expand a memory node into related knowledge."""
        data = self._request("POST", "/v1/knowledge", json={"id": id})
        return KnowledgeResponse(
            success=data.get("success", False),
            id=data.get("id", ""),
            relations=data.get("relations", []),
            details=data.get("details", {}),
        )

    def stats(self) -> StatsResponse:
        """Get account usage statistics."""
        data = self._request("GET", "/v1/stats")
        return StatsResponse(
            success=data.get("success", False),
            user_id=data.get("user_id", ""),
            plan=data.get("plan", ""),
            memories_used=data.get("memories_used", 0),
            max_memories=data.get("max_memories", 0),
            tetra_count=data.get("tetra_count", 0),
            energy=data.get("energy", 0.0),
            clusters=data.get("clusters", []),
        )

    def timeline(self) -> TimelineResponse:
        """Get the memory timeline."""
        data = self._request("GET", "/v1/timeline")
        return TimelineResponse(
            success=data.get("success", False),
            events=data.get("events", []),
            total=data.get("total", 0),
        )

    def close(self) -> None:
        """Close the underlying HTTP session."""
        self._session.close()

    def __enter__(self) -> EpicodeClient:
        return self

    def __exit__(self, *exc: Any) -> None:
        self.close()
