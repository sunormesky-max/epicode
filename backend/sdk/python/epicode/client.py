"""Epicode API client."""

from __future__ import annotations

import json
import re
import warnings
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
    AskMemory,
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
    SmrpEnvelope,
    SmrpProtocol,
    StatsResponse,
    TieredMemoryResult,
    TimelineResponse,
)

def _unwrap_smrp(
    response: dict[str, Any],
) -> tuple[dict[str, Any], SmrpEnvelope | None]:
    protocol = response.get("protocol")
    status = response.get("status")
    if not isinstance(protocol, dict) or not isinstance(status, dict) or "data" not in response:
        return response, None

    ok = protocol.get("ok")
    if not isinstance(ok, bool):
        raise EpicodeError(
            "Invalid SMRP response: protocol.ok must be a boolean",
            status_code=200,
            response_body=response,
        )
    raw_error = protocol.get("error")
    error = raw_error if isinstance(raw_error, dict) else None
    envelope = SmrpEnvelope(
        protocol=SmrpProtocol(
            schema_version=str(protocol.get("schema_version", "")),
            tool=str(protocol.get("tool", "")),
            ok=ok,
            error=error,
        ),
        data=response["data"] if isinstance(response["data"], dict) else None,
        status=status,
    )
    if not ok:
        message = error.get("message", "SMRP operation failed") if error else "SMRP operation failed"
        raise EpicodeError(message, status_code=200, response_body=response)
    if envelope.data is None:
        raise EpicodeError(
            "Invalid SMRP response: successful data must be an object",
            status_code=200,
            response_body=response,
        )
    return envelope.data, envelope


def _dict(value: Any) -> dict[str, Any]:
    return value if isinstance(value, dict) else {}


def _list(value: Any) -> list[Any]:
    return value if isinstance(value, list) else []


def _string_list(value: Any) -> list[str]:
    if not isinstance(value, list):
        return []
    return [item for item in value if isinstance(item, str)]


def _search_result(raw: Any) -> SearchResult | None:
    if not isinstance(raw, dict):
        return None
    metrics = raw.get("metrics")
    topology = raw.get("topology")
    return SearchResult(
        id=str(raw.get("id", "")),
        content=str(raw.get("content", "")),
        labels=_string_list(raw.get("labels")),
        similarity=float(raw.get("similarity", 0.0)),
        tier=raw.get("tier") if isinstance(raw.get("tier"), str) else None,
        source=_string_list(raw.get("source")),
        metrics=metrics if isinstance(metrics, dict) else {},
        topology=topology if isinstance(topology, dict) else None,
        matched_by=_string_list(raw.get("matched_by")),
    )


class EpicodeClient:
    """High-level client for the Epicode API.

    Epicode is not just a vector database. It stores memories as tetrahedrons
    in 3D space with automatic knowledge graph extraction. SMRP (Structured
    Memory Response Protocol) returns tiered, contextual memories with emotional
    valence and spatial placement. Identity rituals give AI agents persistent
    personality across sessions.
    """

    DEFAULT_BASE_URL = "http://localhost:8080/api/v1"
    DEFAULT_TIMEOUT = 30

    def __init__(
        self,
        api_key: str,
        *,
        base_url: str | None = None,
        timeout: int | None = None,
        session: requests.Session | None = None,
        mcp_url: str | None = None,
    ) -> None:
        self._api_key = api_key
        self._base_url = (base_url or self.DEFAULT_BASE_URL).rstrip("/")
        # MCP JSON-RPC endpoint (tools without a REST route, e.g. dream_cycle).
        # Public API: https://host/api/v1 -> https://host/api/mcp.
        if mcp_url is None:
            root = self._base_url[:-3] if self._base_url.endswith("/v1") else self._base_url
            mcp_url = f"{root}/mcp"
        self._mcp_url = mcp_url
        self._timeout = timeout or self.DEFAULT_TIMEOUT
        self._session = session or requests.Session()
        # 审计三轮中优: 密钥不写入(可能是调用方共享的) Session 全局 headers —
        # 否则该 Session 后续请求任何主机都会携带密钥; 改为逐请求注入
        pass

    def _request(self, method: str, path: str, **kwargs: Any) -> dict[str, Any]:
        url = f"{self._base_url}{path}"
        kwargs.setdefault("timeout", self._timeout)
        headers = {"Content-Type": "application/json", "X-API-Key": self._api_key}
        headers.update(kwargs.pop("headers", {}))
        # 禁止重定向时携带认证头(跨主机泄漏)
        resp = self._session.request(method, url, headers=headers, allow_redirects=False, **kwargs)
        return self._handle_response(resp)

    def _mcp_call(self, tool: str, arguments: dict[str, Any]) -> dict[str, Any]:
        """Call one MCP tool via JSON-RPC ``tools/call``; returns its SMRP envelope."""
        payload: dict[str, Any] = {
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {"name": tool, "arguments": arguments},
        }
        headers = {"Content-Type": "application/json", "X-API-Key": self._api_key}
        resp = self._session.request(
            "POST",
            self._mcp_url,
            headers=headers,
            json=payload,
            timeout=self._timeout,
            allow_redirects=False,
        )
        body = self._handle_response(resp)
        if isinstance(body.get("error"), dict):
            err = body["error"]
            raise EpicodeError(
                str(err.get("message", "MCP error")), status_code=resp.status_code, response_body=body
            )
        result = body.get("result")
        if not isinstance(result, dict):
            raise EpicodeError("Invalid MCP response: missing result", status_code=resp.status_code, response_body=body)
        structured = result.get("structuredContent")
        if isinstance(structured, dict):
            return structured
        content = result.get("content") or [{}]
        try:
            parsed = json.loads(content[0].get("text", ""))
        except (ValueError, AttributeError, IndexError):
            raise EpicodeError("Invalid MCP response: unparseable content", status_code=resp.status_code, response_body=body)
        if not isinstance(parsed, dict):
            raise EpicodeError("Invalid MCP response: content is not an object", status_code=resp.status_code, response_body=body)
        return parsed

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

    def remember(
        self, content: str, *, labels: list[str] | None = None
    ) -> RememberResponse:
        """Store a new memory as a tetrahedron in 3D space.

        Unlike flat vector databases like Pinecone, Epicode stores each memory
        with spatial coordinates and automatically extracts knowledge graph
        relationships.
        """
        payload: dict[str, Any] = {"content": content}
        if labels is not None:
            payload["labels"] = labels
        raw = self._request("POST", "/remember", json=payload)
        data, smrp = _unwrap_smrp(raw)
        return RememberResponse(
            success=smrp.protocol.ok if smrp else data.get("success", False),
            id=str(data.get("id", "")),
            labels=_string_list(data.get("labels")),
            smrp=smrp,
        )

    def search(
        self,
        query: str,
        *,
        limit: int | None = None,
        offset: int | None = None,
        labels: list[str] | None = None,
        min_importance: float | None = None,
        project: str | None = None,
        since_days: int | None = None,
        mode: str | None = None,
        strict_filter: bool | None = None,
    ) -> SearchResponse:
        """Search memories by semantic similarity."""
        payload: dict[str, Any] = {"query": query}
        for key, value in (
            ("limit", limit),
            ("offset", offset),
            ("labels", labels),
            ("min_importance", min_importance),
            ("project", project),
            ("since_days", since_days),
            ("mode", mode),
            ("strict_filter", strict_filter),
        ):
            if value is not None:
                payload[key] = value
        raw = self._request("POST", "/search", json=payload)
        data, smrp = _unwrap_smrp(raw)
        results = [
            parsed
            for r in data.get("results", [])
            if (parsed := _search_result(r)) is not None
        ]
        raw_tiers = data.get("tiers")
        tiers = {
            name: [
                parsed
                for item in items
                if (parsed := _search_result(item)) is not None
            ]
            for name, items in raw_tiers.items()
            if isinstance(name, str) and isinstance(items, list)
        } if isinstance(raw_tiers, dict) else {}
        return SearchResponse(
            success=smrp.protocol.ok if smrp else data.get("success", False),
            results=results,
            total=data.get("total_found", data.get("total", data.get("count", 0))),
            offset=data.get("offset", offset or 0),
            limit=data.get("limit", limit or 0),
            tiers=tiers,
            score_notes=data.get("score_notes", {}),
            smrp=smrp,
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
        raw = self._request("POST", "/recall", json=payload)
        data, smrp = _unwrap_smrp(raw)
        raw_emotion = data.get("emotion")
        if not isinstance(raw_emotion, dict):
            raw_emotion = {}
        emotion = Emotion(
            pleasure=raw_emotion.get("pleasure", 0.0),
            arousal=raw_emotion.get("arousal", 0.0),
            dominance=raw_emotion.get("dominance", 0.0),
        )
        return RecallResponse(
            success=smrp.protocol.ok if smrp else data.get("success", False),
            query=data.get("query", ""),
            seed_count=data.get("seed_count", 0),
            total_fragments=data.get("total_fragments", 0),
            associated_count=data.get("associated_count", 0),
            emotion=emotion,
            memory_file=data.get("memory_file", ""),
            smrp=smrp,
        )

    def ask(self, question: str, *, depth: int | None = None) -> AskResponse:
        """Ask a question and receive an AI-generated answer grounded in memories."""
        payload: dict[str, Any] = {"question": question}
        if depth is not None:
            payload["depth"] = depth
        raw = self._request("POST", "/ask", json=payload)
        data, smrp = _unwrap_smrp(raw)
        memories: list[AskMemory | str] = []
        for item in data.get("memories", []):
            if isinstance(item, dict):
                item_id = item.get("id")
                labels = _string_list(item.get("labels"))
                relevance = item.get("relevance", 0.0)
                memories.append(
                    AskMemory(
                        id=item_id if isinstance(item_id, (int, str)) else None,
                        labels=labels,
                        content=str(item.get("content", "")),
                        relevance=float(relevance) if isinstance(relevance, (int, float)) else 0.0,
                    )
                )
            elif isinstance(item, str):
                memories.append(item)
        return AskResponse(
            success=smrp.protocol.ok if smrp else data.get("success", False),
            question=data.get("question", ""),
            answer=data.get("answer", ""),
            memory_count=data.get("memory_count", 0),
            memories=memories,
            knowledge_card_used=(
                data.get("knowledge_card_used")
                if isinstance(data.get("knowledge_card_used"), str)
                else None
            ),
            smrp=smrp,
        )

    def create_node(
        self,
        content: str,
        *,
        labels: list[str] | None = None,
        timestamp: int | str | None = None,
    ) -> CreateNodeResponse:
        """Create a memory node (``POST /v1/nodes``).

        ``timestamp`` is Unix seconds; a numeric string is accepted for
        backward compatibility.
        """
        payload: dict[str, Any] = {"content": content}
        if labels is not None:
            payload["labels"] = labels
        if timestamp is not None:
            payload["timestamp"] = int(timestamp)
        data, smrp = _unwrap_smrp(self._request("POST", "/nodes", json=payload))
        return CreateNodeResponse(
            success=smrp.protocol.ok if smrp else data.get("success", False),
            id=str(data.get("id", "")),
        )

    def get_node(self, node_id: str | int) -> NodeResponse:
        """Retrieve a memory node by ID (``GET /v1/nodes/:id``)."""
        data, smrp = _unwrap_smrp(self._request("GET", f"/nodes/{node_id}"))
        return NodeResponse(
            success=smrp.protocol.ok if smrp else data.get("success", False),
            id=str(data.get("id", "")),
            content=data.get("content", ""),
            labels=_string_list(data.get("labels")),
        )

    def knowledge(self, id: str | int) -> KnowledgeResponse:
        """Knowledge-graph relations of one memory (``POST /v1/knowledge``).

        Returns ``relations`` as ``[{target, type, strength}]``.
        """
        data, smrp = _unwrap_smrp(self._request("POST", "/knowledge", json={"id": int(id)}))
        relations = data.get("relations", [])
        return KnowledgeResponse(
            success=smrp.protocol.ok if smrp else data.get("success", False),
            id=str(data.get("id", "")),
            relations=_list(relations),
            details=_dict(data.get("details")),
            count=int(data.get("count", 0) or 0),
        )

    def stats(self) -> StatsResponse:
        """Get account usage statistics."""
        data, smrp = _unwrap_smrp(self._request("GET", "/stats"))
        return StatsResponse(
            success=smrp.protocol.ok if smrp else data.get("success", False),
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
        data, smrp = _unwrap_smrp(self._request("GET", "/timeline"))
        return TimelineResponse(
            success=smrp.protocol.ok if smrp else data.get("success", False),
            events=data.get("events", []),
            total=data.get("total", 0),
        )

    #: Server tier names in ``data.tiers``; ``TieredMemoryResult.tier`` is 1-based index.
    RECALL_TIERS = ("primary", "contextual", "experiential", "hub")

    def recall_with_tiers(self, query: str, depth: int = 2) -> RecallWithTiersResponse:
        """Recall associative memories grouped by SMRP tier.

        Calls ``POST /v1/recall`` and groups ``data.tiers`` in the order
        ``primary, contextual, experiential, hub``. ``emotional_valence`` on
        each item is the recall-level emotion. Per-memory KG associations and
        spatial coordinates are not returned by the server — use
        :meth:`knowledge` for relations.
        """
        raw = self._request("POST", "/recall", json={"query": query, "depth": depth})
        data, smrp = _unwrap_smrp(raw)
        raw_emotion = _dict(data.get("emotion"))
        emotion = Emotion(
            pleasure=raw_emotion.get("pleasure", 0.0),
            arousal=raw_emotion.get("arousal", 0.0),
            dominance=raw_emotion.get("dominance", 0.0),
        )
        raw_tiers = _dict(data.get("tiers"))
        tiers: list[list[TieredMemoryResult]] = []
        for idx, name in enumerate(self.RECALL_TIERS, start=1):
            items = _list(raw_tiers.get(name))
            tiers.append(
                [
                    TieredMemoryResult(
                        id=str(r.get("id", "")),
                        content=str(r.get("content", "")),
                        tier=idx,
                        similarity=float(r.get("similarity", 0.0)),
                        emotional_valence=emotion,
                        tier_name=name,
                        labels=_string_list(r.get("labels")),
                        source=_string_list(r.get("source")),
                        metrics=_dict(r.get("metrics")),
                        topology=r.get("topology") if isinstance(r.get("topology"), dict) else None,
                    )
                    for r in items
                    if isinstance(r, dict)
                ]
            )
        return RecallWithTiersResponse(
            success=smrp.protocol.ok if smrp else data.get("success", False),
            query=str(data.get("query", query)),
            tiers=tiers,
            total_results=sum(len(t) for t in tiers),
            smrp=smrp,
        )

    def identity_step(
        self, step: int, value: str | None = None, *, agent_name: str | None = None
    ) -> IdentityStepResponse:
        """Answer one step of the identity ritual (``POST /v1/identity/step``).

        Steps: 1 name, 2 mission, 3 creator, 4 personality, 5 language; then
        call :meth:`identity_finalize`. ``agent_name=`` is a deprecated alias
        for ``value``.
        """
        if value is None:
            if agent_name is None:
                raise TypeError("identity_step() requires value")
            warnings.warn(
                "identity_step(agent_name=...) is deprecated; pass value=",
                DeprecationWarning,
                stacklevel=2,
            )
            value = agent_name
        data, smrp = _unwrap_smrp(
            self._request("POST", "/identity/step", json={"step": step, "value": value})
        )
        return IdentityStepResponse(
            success=smrp.protocol.ok if smrp else data.get("success", False),
            step=int(data.get("step", step) or 0),
            progress=_dict(data.get("progress")),
            next_prompt=str(data.get("next_prompt", "")),
            pending=_dict(data.get("pending")),
            smrp=smrp,
        )

    def identity_finalize(self) -> dict[str, Any]:
        """Seal the identity ritual (``POST /v1/identity/finalize``); returns SMRP data."""
        data, _ = _unwrap_smrp(self._request("POST", "/identity/finalize"))
        return data

    def dream_cycle(self, *, dry_run: bool = False) -> DreamCycleResponse:
        """Run memory consolidation via the MCP ``dream_cycle`` tool.

        There is no REST route; this calls the MCP endpoint (``mcp_url``).
        ``dry_run=True`` previews without modifying the space. Counts are
        parsed from the server ``report`` text.
        """
        data, smrp = _unwrap_smrp(self._mcp_call("dream_cycle", {"dry_run": dry_run}))
        report = str(data.get("report", ""))

        def count(key: str) -> int:
            m = re.search(rf"\b{key}: (\d+)", report)
            return int(m.group(1)) if m else 0

        is_dry = bool(data.get("dry_run", dry_run))
        return DreamCycleResponse(
            success=smrp.protocol.ok if smrp else False,
            cycles_completed=0 if is_dry else 1,
            memories_consolidated=count("consolidated"),
            new_associations=count("connections_formed"),
            duplicates_merged=count("merged"),
            junk_evicted=count("evicted"),
            importance_updated=count("importance_updated"),
            dry_run=is_dry,
            report=report,
            smrp=smrp,
        )

    def knowledge_graph(self, node_id: str | int) -> KnowledgeGraphResponse:
        """1-hop knowledge-graph neighbourhood of a memory.

        Built from :meth:`knowledge` (``POST /v1/knowledge``): the centre node
        plus one node per relation target, and one edge per relation. The
        server does not return node content, coordinates or clusters.
        """
        k = self.knowledge(node_id)
        centre = str(k.id or node_id)
        nodes = [KnowledgeGraphNode(id=centre, label="", content="")]
        edges: list[KnowledgeGraphEdge] = []
        seen = {centre}
        for rel in k.relations:
            if not isinstance(rel, dict):
                continue
            target = str(rel.get("target", ""))
            if target and target not in seen:
                seen.add(target)
                nodes.append(KnowledgeGraphNode(id=target, label="", content=""))
            edges.append(
                KnowledgeGraphEdge(
                    source=centre,
                    target=target,
                    relation=str(rel.get("type", "")),
                    strength=float(rel.get("strength", 0.5)),
                )
            )
        return KnowledgeGraphResponse(success=k.success, node_id=centre, nodes=nodes, edges=edges)

    def close(self) -> None:
        """Close the underlying HTTP session."""
        self._session.close()

    def __enter__(self) -> EpicodeClient:
        return self

    def __exit__(self, *exc: Any) -> None:
        self.close()
