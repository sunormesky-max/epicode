"""Epicode SDK data models."""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any


@dataclass(frozen=True)
class HealthResponse:
    status: str
    version: str
    success: bool


@dataclass(frozen=True)
class RememberResponse:
    success: bool
    id: int
    labels: list[str] = field(default_factory=list)


@dataclass(frozen=True)
class SearchResult:
    id: int
    content: str
    labels: list[str] = field(default_factory=list)
    similarity: float = 0.0


@dataclass(frozen=True)
class SearchResponse:
    success: bool
    results: list[SearchResult] = field(default_factory=list)
    total: int = 0


@dataclass(frozen=True)
class Emotion:
    pleasure: float = 0.0
    arousal: float = 0.0
    dominance: float = 0.0


@dataclass(frozen=True)
class RecallResponse:
    success: bool
    query: str = ""
    seed_count: int = 0
    total_fragments: int = 0
    associated_count: int = 0
    emotion: Emotion = field(default_factory=Emotion)
    memory_file: dict[str, list[dict[str, Any]]] | None = None


@dataclass(frozen=True)
class AskResponse:
    success: bool
    question: str = ""
    answer: str = ""
    memory_count: int = 0
    memories: list[dict[str, Any]] = field(default_factory=list)


@dataclass(frozen=True)
class CreateNodeResponse:
    success: bool
    id: int


@dataclass(frozen=True)
class NodeResponse:
    success: bool
    id: int
    content: str
    labels: list[str] = field(default_factory=list)


@dataclass(frozen=True)
class KnowledgeResponse:
    success: bool
    id: int
    relations: int = 0
    details: list[dict[str, Any]] = field(default_factory=list)


@dataclass(frozen=True)
class StatsResponse:
    success: bool
    user_id: str = ""
    plan: str = ""
    memories_used: int = 0
    max_memories: int = 0
    tetra_count: int = 0
    energy: float = 0.0
    clusters: list[Any] = field(default_factory=list)


@dataclass(frozen=True)
class TimelineEvent:
    raw: dict[str, Any] = field(default_factory=dict)


@dataclass(frozen=True)
class TimelineResponse:
    success: bool
    events: list[dict[str, Any]] = field(default_factory=list)
    total: int = 0


@dataclass(frozen=True)
class RegisterResponse:
    success: bool
    user_id: str
    api_key: str
    plan: str
    max_memories: int


@dataclass(frozen=True)
class AdminUsersResponse:
    success: bool
    total_users: int = 0
    active_engines: int = 0


@dataclass(frozen=True)
class AdminStatsResponse:
    success: bool
    total_users: int = 0
    active_engines: int = 0
    max_users: int = 0


@dataclass(frozen=True)
class IdentityStepResponse:
    """Response from an identity ritual step."""

    success: bool
    step: int = 0
    progress: dict[str, int] = field(default_factory=dict)
    next_prompt: str = ""
    pending: dict[str, bool] = field(default_factory=dict)


@dataclass(frozen=True)
class IdentityFinalizeResponse:
    """Response from completing the identity ritual."""

    success: bool
    awakened: bool = False
    identity: dict[str, Any] = field(default_factory=dict)
    message: str = ""


@dataclass(frozen=True)
class McpToolResponse:
    """JSON-RPC response returned by an MCP tool call."""

    jsonrpc: str
    id: int | str | None
    result: Any = None
    error: dict[str, Any] | None = None
