#!/usr/bin/env python3
"""MCP server bridge for the Epicode spatial memory service.

This server exposes Epicode's cloud REST API as MCP tools so that MCP hosts such
as Claude Desktop and Cursor can create, search, recall and ask memories stored
at https://epicode.cn.

Tools
-----
- memory_create  -> POST /api/v1/remember
- memory_search  -> POST /api/v1/search
- memory_recall  -> POST /api/v1/recall
- memory_ask     -> POST /api/v1/ask
- health         -> GET  /health

Environment variables
---------------------
EPICODE_API_KEY : str
    Required for authenticated endpoints.
EPICODE_BASE_URL : str, optional
    Defaults to ``https://epicode.cn``.
"""

from __future__ import annotations

import os
from enum import Enum
from typing import Any

import requests
from dotenv import load_dotenv
from mcp.server.fastmcp import FastMCP

# Load environment variables from a local .env file if present.
load_dotenv()


class EpicodeClientError(Exception):
    """Raised when an Epicode API request fails."""

    def __init__(
        self,
        message: str,
        status_code: int | None = None,
        response_body: Any = None,
    ) -> None:
        super().__init__(message)
        self.status_code = status_code
        self.response_body = response_body


class SearchModeOption(str, Enum):
    HYBRID = "hybrid"
    EXACT = "exact"
    SEMANTIC = "semantic"
    GRAPH = "graph"
    AUTO = "auto"
    FUSION = "fusion"


class EpicodeClient:
    """Lightweight HTTP client for the Epicode cloud API."""

    DEFAULT_BASE_URL: str = "https://epicode.cn"
    DEFAULT_TIMEOUT: int = 60

    def __init__(
        self,
        api_key: str | None = None,
        *,
        base_url: str | None = None,
        timeout: int | None = None,
    ) -> None:
        resolved_key = api_key or os.getenv("EPICODE_API_KEY")
        if not resolved_key:
            raise EpicodeClientError(
                "API key is required. Set EPICODE_API_KEY environment variable."
            )
        self._api_key: str = resolved_key
        self._base_url: str = (
            base_url or os.getenv("EPICODE_BASE_URL") or self.DEFAULT_BASE_URL
        ).rstrip("/")
        self._timeout: int = timeout or self.DEFAULT_TIMEOUT
        self._session: requests.Session = requests.Session()
        self._session.headers.update(
            {
                "X-API-Key": self._api_key,
                "Content-Type": "application/json",
                "Accept": "application/json",
            }
        )

    def _request(
        self,
        method: str,
        path: str,
        *,
        json_payload: dict[str, Any] | None = None,
        authenticated: bool = True,
    ) -> dict[str, Any]:
        """Execute an HTTP request and return the decoded JSON response."""
        url = f"{self._base_url}{path}"
        headers: dict[str, str] = {"Accept": "application/json"}
        if authenticated:
            headers["X-API-Key"] = self._api_key
        if json_payload is not None:
            headers["Content-Type"] = "application/json"

        try:
            response = self._session.request(
                method,
                url,
                headers=headers,
                json=json_payload,
                timeout=self._timeout,
            )
        except requests.RequestException as exc:
            raise EpicodeClientError(f"Network error: {exc}") from exc

        try:
            body = response.json()
        except ValueError:
            body = {}

        if not response.ok:
            message = (
                body.get("error")
                or body.get("message")
                or response.text
                or f"HTTP {response.status_code}"
            )
            raise EpicodeClientError(
                message,
                status_code=response.status_code,
                response_body=body,
            )

        return body

    def health(self) -> dict[str, Any]:
        """Check Epicode cloud health."""
        return self._request("GET", "/health", authenticated=False)

    def remember(
        self, content: str, labels: list[str] | None = None
    ) -> dict[str, Any]:
        """Store a new memory."""
        payload: dict[str, Any] = {"content": content}
        if labels is not None:
            payload["labels"] = labels
        return self._request("POST", "/api/v1/remember", json_payload=payload)

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
    ) -> dict[str, Any]:
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
        return self._request("POST", "/api/v1/search", json_payload=payload)

    def recall(self, query: str, depth: int | None = None) -> dict[str, Any]:
        """Recall associative memories."""
        payload: dict[str, Any] = {"query": query}
        if depth is not None:
            payload["depth"] = depth
        return self._request("POST", "/api/v1/recall", json_payload=payload)

    def ask(self, question: str, depth: int | None = None) -> dict[str, Any]:
        """Ask a question grounded in memories."""
        payload: dict[str, Any] = {"question": question}
        if depth is not None:
            payload["depth"] = depth
        return self._request("POST", "/api/v1/ask", json_payload=payload)


# ---------------------------------------------------------------------------
# MCP server
# ---------------------------------------------------------------------------

mcp = FastMCP("epicode-memory")

# Shared client instance. Created lazily so missing config surfaces as a tool
# error rather than at import time.
_client: EpicodeClient | None = None


def _get_client() -> EpicodeClient:
    """Return the shared Epicode client, creating it on first use."""
    global _client  # noqa: PLW0603
    if _client is None:
        _client = EpicodeClient()
    return _client


@mcp.tool()
def health() -> dict[str, Any]:
    """Check Epicode cloud connectivity and service health."""
    return _get_client().health()


@mcp.tool()
def memory_create(content: str, labels: list[str] | None = None) -> dict[str, Any]:
    """Create a new memory in the Epicode spatial memory system.

    Args:
        content: The text content of the memory to store.
        labels: Optional category tags to attach to the memory.
    """
    return _get_client().remember(content, labels=labels)


@mcp.tool()
def memory_search(
    query: str,
    limit: int | None = None,
    offset: int | None = None,
    labels: list[str] | None = None,
    min_importance: float | None = None,
    project: str | None = None,
    since_days: int | None = None,
    mode: SearchModeOption | None = None,
    strict_filter: bool | None = None,
) -> dict[str, Any]:
    """Search memories and inspect ranked evidence with tier and provenance.

    Args:
        query: The search query.
        limit: Maximum number of results (REST default 20, maximum 200).
        offset: Number of ranked results to skip (default 0).
        labels: Optional OR filter for memory labels.
        min_importance: Optional minimum importance filter.
        project: Optional project filter.
        since_days: Optional age filter in days.
        mode: hybrid, exact, semantic, graph, auto, or fusion.
        strict_filter: Disable semantic backfill when applying filters.
    """
    return _get_client().search(
        query,
        limit=limit,
        offset=offset,
        labels=labels,
        min_importance=min_importance,
        project=project,
        since_days=since_days,
        mode=mode.value if isinstance(mode, SearchModeOption) else mode,
        strict_filter=strict_filter,
    )


@mcp.tool()
def memory_recall(query: str, depth: int | None = None) -> dict[str, Any]:
    """Recall associative memories for a query using SMRP.

    Args:
        query: The recall query.
        depth: Associative recall depth (optional).
    """
    return _get_client().recall(query, depth=depth)


@mcp.tool()
def memory_ask(question: str, depth: int | None = None) -> dict[str, Any]:
    """Synthesize an answer from memories and return structured source evidence.

    Args:
        question: The question to ask.
        depth: Recall depth used to ground the answer (optional).

    Use memory_search or memory_recall when the caller needs to inspect evidence
    without synthesis. A zero memory_count means no relevant memory evidence was found.
    """
    return _get_client().ask(question, depth=depth)


if __name__ == "__main__":
    mcp.run(transport="stdio")
