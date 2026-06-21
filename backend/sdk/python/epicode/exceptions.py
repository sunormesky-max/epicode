"""Epicode SDK exceptions."""


class EpicodeError(Exception):
    """Base exception for all Epicode SDK errors."""

    def __init__(self, message: str, status_code: int | None = None, response_body: dict | None = None):
        super().__init__(message)
        self.status_code = status_code
        self.response_body = response_body or {}


class AuthenticationError(EpicodeError):
    """Raised when the API key is missing or invalid."""


class NotFoundError(EpicodeError):
    """Raised when a requested resource is not found."""


class RateLimitError(EpicodeError):
    """Raised when the API rate limit is exceeded."""


class ServerError(EpicodeError):
    """Raised when the server returns a 5xx error."""


class ValidationError(EpicodeError):
    """Raised when the request body fails validation."""


class PlanLimitExceededError(EpicodeError):
    """Raised when the user's plan memory limit is exceeded."""
