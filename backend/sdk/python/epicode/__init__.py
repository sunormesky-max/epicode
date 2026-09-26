"""Epicode SDK – Python client library."""

from epicode.client import EpicodeClient
from epicode.admin import EpicodeAdmin
from epicode.exceptions import (
    AuthenticationError,
    EpicodeError,
    NotFoundError,
    PlanLimitExceededError,
    RateLimitError,
    ServerError,
    ValidationError,
)
from epicode.models import (
    AdminStatsResponse,
    AdminUsersResponse,
    AskResponse,
    CreateNodeResponse,
    DreamCycleResponse,
    Emotion,
    HealthResponse,
    IdentityStepResponse,
    KnowledgeResponse,
    RecallResponse,
    RecallWithTiersResponse,
    RegisterResponse,
    RememberResponse,
    SearchResult,
    SearchResponse,
    StatsResponse,
    TimelineEvent,
    TimelineResponse,
    TieredMemoryResult,
)

__version__ = "1.0.3"  # x-release-please-version
__all__ = [
    "EpicodeClient",
    "EpicodeAdmin",
    "EpicodeError",
    "AuthenticationError",
    "NotFoundError",
    "PlanLimitExceededError",
    "RateLimitError",
    "ServerError",
    "ValidationError",
    "AdminStatsResponse",
    "AdminUsersResponse",
    "AskResponse",
    "CreateNodeResponse",
    "Emotion",
    "HealthResponse",
    "KnowledgeResponse",
    "RecallResponse",
    "RecallWithTiersResponse",
    "RegisterResponse",
    "RememberResponse",
    "SearchResult",
    "SearchResponse",
    "StatsResponse",
    "TimelineEvent",
    "TimelineResponse",
]
