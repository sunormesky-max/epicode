"""Epicode admin client."""

from __future__ import annotations

from typing import Any

import requests

from tetramem.exceptions import (
    AuthenticationError,
    NotFoundError,
    RateLimitError,
    ServerError,
    EpicodeError,
    ValidationError,
)
from tetramem.models import AdminStatsResponse, AdminUsersResponse, RegisterResponse


class EpicodeAdmin:
    """Admin client for the Epicode API."""

    DEFAULT_BASE_URL = "http://localhost:9111"
    DEFAULT_TIMEOUT = 30

    def __init__(
        self,
        admin_key: str,
        *,
        base_url: str | None = None,
        timeout: int | None = None,
        session: requests.Session | None = None,
    ) -> None:
        self._admin_key = admin_key
        self._base_url = (base_url or self.DEFAULT_BASE_URL).rstrip("/")
        self._timeout = timeout or self.DEFAULT_TIMEOUT
        self._session = session or requests.Session()
        self._session.headers.update({"X-Admin-Key": self._admin_key, "Content-Type": "application/json"})

    # ------------------------------------------------------------------
    # Internal helpers
    # ------------------------------------------------------------------

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
        if code >= 500:
            raise ServerError(message, status_code=code, response_body=body)

        raise EpicodeError(message, status_code=code, response_body=body)

    # ------------------------------------------------------------------
    # Public admin API
    # ------------------------------------------------------------------

    def register(self, user_id: str, *, plan: str = "free") -> RegisterResponse:
        """Register a new user and obtain an API key."""
        data = self._request("POST", "/register", json={"user_id": user_id, "plan": plan})
        return RegisterResponse(
            success=data.get("success", False),
            user_id=data.get("user_id", ""),
            api_key=data.get("api_key", ""),
            plan=data.get("plan", ""),
            max_memories=data.get("max_memories", 0),
        )

    def list_users(self) -> AdminUsersResponse:
        """List all registered users."""
        data = self._request("GET", "/admin/users")
        return AdminUsersResponse(
            success=data.get("success", False),
            total_users=data.get("total_users", 0),
            active_engines=data.get("active_engines", 0),
        )

    def get_stats(self) -> AdminStatsResponse:
        """Get global admin statistics."""
        data = self._request("GET", "/admin/stats")
        return AdminStatsResponse(
            success=data.get("success", False),
            total_users=data.get("total_users", 0),
            active_engines=data.get("active_engines", 0),
            max_users=data.get("max_users", 0),
        )

    def close(self) -> None:
        """Close the underlying HTTP session."""
        self._session.close()

    def __enter__(self) -> EpicodeAdmin:
        return self

    def __exit__(self, *exc: Any) -> None:
        self.close()
