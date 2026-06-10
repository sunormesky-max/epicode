"""TetraMem SDK exceptions."""


class TetraMemError(Exception):
    """Base exception for all TetraMem SDK errors."""

    def __init__(self, message: str, status_code: int | None = None, response_body: dict | None = None):
        super().__init__(message)
        self.status_code = status_code
        self.response_body = response_body or {}


class AuthenticationError(TetraMemError):
    """Raised when the API key is missing or invalid."""


class NotFoundError(TetraMemError):
    """Raised when a requested resource is not found."""


class RateLimitError(TetraMemError):
    """Raised when the API rate limit is exceeded."""


class ServerError(TetraMemError):
    """Raised when the server returns a 5xx error."""


class ValidationError(TetraMemError):
    """Raised when the request body fails validation."""


class PlanLimitExceededError(TetraMemError):
    """Raised when the user's plan memory limit is exceeded."""
