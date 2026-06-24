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
    DreamCycleResponse,
    Emotion,
    HealthResponse,
    IdentityStepResponse,
    KnowledgeGraphEdge,
    KnowledgeGraphNode,
    KnowledgeGraphResponse,
    KnowledgeResponse,
    NodeResponse,
    RecallResponse,
    RecallWithTiersResponse,
    RememberResponse,
    SearchResult,
    SearchResponse,
    StatsResponse,
    TieredMemoryResult,
    TimelineResponse,
)


class EpicodeClient:
    """High-level client for the Epicode API.

    Epicode is not just a vector database. It stores memories as tetrahedrons
    in 3D space with automatic knowledge graph extraction. SMRP (Structured
    Memory Response Protocol) returns tiered, contextual memories with emotional
    valence and spatial placement. Identity rituals give AI agents persistent
    personality across sessions.
    """

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
        """Store a new memory as a tetrahedron in 3D space.

        Unlike flat vector databases like Pinecone, Epicode stores each memory
        with spatial coordinates and automatically extracts knowledge graph
        relationships.
        """
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
        """Recall associative memories for a query.

        Uses SMRP (Structured Memory Response Protocol) to return contextual
        memories with emotional valence and spatial placement — not just flat
        similarity scores like traditional vector databases.
        """
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

    def recall_with_tiers(self, query: str, depth: int = 2) -> RecallWithTiersResponse:
        """Return tiered memory results with knowledge graph associations.

        This is Epicode's key differentiator — not just flat vector search,
        but structured memory with tiers and KG relationships. SMRP (Structured
        Memory Response Protocol) returns tiered, contextual memories with
        emotional valence and spatial placement.

        Args:
            query: The search query.
            depth: How many tiers to traverse in the knowledge graph.

        Returns:
            A ``RecallWithTiersResponse`` containing tiered results and KG edges.
        """
        payload: dict[str, Any] = {"query": query, "depth": depth}
        data = self._request("POST", "/v1/recall/tiers", json=payload)

        tiers: list[list[TieredMemoryResult]] = []
        for tier_list in data.get("tiers", []):
            tier_results: list[TieredMemoryResult] = []
            for r in tier_list:
                raw_emotion = r.get("emotional_valence", {})
                emotion = Emotion(
                    pleasure=raw_emotion.get("pleasure", 0.0),
                    arousal=raw_emotion.get("arousal", 0.0),
                    dominance=raw_emotion.get("dominance", 0.0),
                )
                coords = r.get("spatial_coords", [0.0, 0.0, 0.0])
                if len(coords) < 3:
                    coords = [0.0, 0.0, 0.0]
                tier_results.append(
                    TieredMemoryResult(
                        id=r.get("id", ""),
                        content=r.get("content", ""),
                        tier=r.get("tier", 1),
                        similarity=r.get("similarity", 0.0),
                        kg_associations=r.get("kg_associations", []),
                        emotional_valence=emotion,
                        spatial_coords=(coords[0], coords[1], coords[2]),
                    )
                )
            tiers.append(tier_results)

        return RecallWithTiersResponse(
            success=data.get("success", False),
            query=data.get("query", ""),
            tiers=tiers,
            total_results=data.get("total_results", 0),
            knowledge_graph_edges=data.get("knowledge_graph_edges", []),
        )

    def identity_step(self, step: int, agent_name: str) -> IdentityStepResponse:
        """Perform the identity ritual step.

        Identity rituals give AI agents persistent personality across sessions.
        This is a unique Epicode feature that goes far beyond simple vector
        storage, allowing agents to build and maintain a sense of self over time.

        Args:
            step: The ritual step number (1-7).
            agent_name: The name of the agent performing the ritual.

        Returns:
            An ``IdentityStepResponse`` with the updated ritual state.
        """
        payload = {"step": step, "agent_name": agent_name}
        data = self._request("POST", "/v1/identity/step", json=payload)
        return IdentityStepResponse(
            success=data.get("success", False),
            step=data.get("step", 0),
            agent_name=data.get("agent_name", ""),
            ritual_state=data.get("ritual_state", ""),
            personality_signature=data.get("personality_signature", {}),
        )

    def dream_cycle(self) -> DreamCycleResponse:
        """Trigger background memory consolidation.

        The "living memory system" aspect of Epicode. Dream cycles run in the
        background to consolidate memories, form new associations, and prune weak
        connections — mimicking how biological brains strengthen memories during
        sleep. This is not something flat vector databases can do.

        Returns:
            A ``DreamCycleResponse`` with consolidation metrics.
        """
        data = self._request("POST", "/v1/dream/cycle")
        return DreamCycleResponse(
            success=data.get("success", False),
            cycles_completed=data.get("cycles_completed", 0),
            memories_consolidated=data.get("memories_consolidated", 0),
            new_associations=data.get("new_associations", 0),
            energy_delta=data.get("energy_delta", 0.0),
        )

    def knowledge_graph(self, node_id: str) -> KnowledgeGraphResponse:
        """Return knowledge graph visualization data for a node.

        Epicode automatically extracts knowledge graph relationships from
        memories stored as tetrahedrons in 3D space. This method returns the
        nodes, edges, and clusters that make up the graph around a given memory.

        Args:
            node_id: The ID of the central node to visualize.

        Returns:
            A ``KnowledgeGraphResponse`` with nodes, edges, and cluster data.
        """
        data = self._request("GET", f"/v1/knowledge-graph/{node_id}")
        nodes = [
            KnowledgeGraphNode(
                id=n.get("id", ""),
                label=n.get("label", ""),
                content=n.get("content", ""),
                x=n.get("x", 0.0),
                y=n.get("y", 0.0),
                z=n.get("z", 0.0),
                tier=n.get("tier", 1),
            )
            for n in data.get("nodes", [])
        ]
        edges = [
            KnowledgeGraphEdge(
                source=e.get("source", ""),
                target=e.get("target", ""),
                relation=e.get("relation", ""),
                strength=e.get("strength", 0.5),
            )
            for e in data.get("edges", [])
        ]
        return KnowledgeGraphResponse(
            success=data.get("success", False),
            node_id=data.get("node_id", ""),
            nodes=nodes,
            edges=edges,
            clusters=data.get("clusters", []),
        )

    def close(self) -> None:
        """Close the underlying HTTP session."""
        self._session.close()

    def __enter__(self) -> EpicodeClient:
        return self

    def __exit__(self, *exc: Any) -> None:
        self.close()
