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
    id: str
    labels: list[str] = field(default_factory=list)


@dataclass(frozen=True)
class SearchResult:
    id: str
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
    memory_file: str = ""


@dataclass(frozen=True)
class AskResponse:
    success: bool
    question: str = ""
    answer: str = ""
    memory_count: int = 0
    memories: list[str] = field(default_factory=list)


@dataclass(frozen=True)
class CreateNodeResponse:
    success: bool
    id: str


@dataclass(frozen=True)
class NodeResponse:
    success: bool
    id: str
    content: str
    labels: list[str] = field(default_factory=list)


@dataclass(frozen=True)
class KnowledgeResponse:
    success: bool
    id: str
    relations: list[Any] = field(default_factory=list)
    details: dict[str, Any] = field(default_factory=dict)


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
class TieredMemoryResult:
    """A single tiered memory result with knowledge graph associations."""
    id: str
    content: str
    tier: int
    similarity: float = 0.0
    kg_associations: list[dict[str, Any]] = field(default_factory=list)
    emotional_valence: Emotion = field(default_factory=Emotion)
    spatial_coords: tuple[float, float, float] = (0.0, 0.0, 0.0)


@dataclass(frozen=True)
class RecallWithTiersResponse:
    """Tiered memory recall response via SMRP (Structured Memory Response Protocol)."""
    success: bool
    query: str = ""
    tiers: list[list[TieredMemoryResult]] = field(default_factory=list)
    total_results: int = 0
    knowledge_graph_edges: list[dict[str, Any]] = field(default_factory=list)


@dataclass(frozen=True)
class IdentityStepResponse:
    """Response from an identity ritual step."""
    success: bool
    step: int = 0
    agent_name: str = ""
    ritual_state: str = ""
    personality_signature: dict[str, Any] = field(default_factory=dict)


@dataclass(frozen=True)
class DreamCycleResponse:
    """Response from triggering a background memory consolidation (dream cycle)."""
    success: bool
    cycles_completed: int = 0
    memories_consolidated: int = 0
    new_associations: int = 0
    energy_delta: float = 0.0


@dataclass(frozen=True)
class KnowledgeGraphNode:
    """A node in the knowledge graph visualization."""
    id: str
    label: str
    content: str
    x: float = 0.0
    y: float = 0.0
    z: float = 0.0
    tier: int = 1


@dataclass(frozen=True)
class KnowledgeGraphEdge:
    """An edge in the knowledge graph visualization."""
    source: str
    target: str
    relation: str
    strength: float = 0.5


@dataclass(frozen=True)
class KnowledgeGraphResponse:
    """Knowledge graph visualization data."""
    success: bool
    node_id: str = ""
    nodes: list[KnowledgeGraphNode] = field(default_factory=list)
    edges: list[KnowledgeGraphEdge] = field(default_factory=list)
    clusters: list[dict[str, Any]] = field(default_factory=list)

