"""Unit and integration tests for the Epicode memory skill client."""

import json
import sys
from pathlib import Path
from unittest import mock

import pytest

# Add the skill source directory to the import path.
SKILL_DIR = Path(__file__).resolve().parent.parent / "skills" / "epicode-memory"
sys.path.insert(0, str(SKILL_DIR))

from epicode_memory import EpicodeMemoryClient, EpicodeMemoryError, main


@pytest.fixture
def api_key():
    return "tm-test-key"


@pytest.fixture
def client(api_key):
    return EpicodeMemoryClient(api_key=api_key, base_url="https://example.com")


class TestClientInitialization:
    def test_requires_api_key(self):
        with mock.patch.dict("os.environ", {}, clear=True):
            with pytest.raises(EpicodeMemoryError, match="API key is required"):
                EpicodeMemoryClient()

    def test_accepts_api_key_from_env(self):
        with mock.patch.dict("os.environ", {"EPICODE_API_KEY": "tm-env-key"}):
            client = EpicodeMemoryClient()
            assert client._api_key == "tm-env-key"
            client.close()

    def test_accepts_api_key_argument(self, api_key):
        client = EpicodeMemoryClient(api_key=api_key)
        assert client._api_key == api_key
        client.close()

    def test_argument_overrides_env(self, api_key):
        with mock.patch.dict("os.environ", {"EPICODE_API_KEY": "tm-env-key"}):
            client = EpicodeMemoryClient(api_key=api_key)
            assert client._api_key == api_key
            client.close()

    def test_default_base_url(self, api_key):
        client = EpicodeMemoryClient(api_key=api_key)
        assert client._base_url == "https://epicode.cn"
        client.close()

    def test_env_base_url_override(self, api_key):
        with mock.patch.dict(
            "os.environ", {"EPICODE_BASE_URL": "https://custom.epicode.cn"}
        ):
            client = EpicodeMemoryClient(api_key=api_key)
            assert client._base_url == "https://custom.epicode.cn"
            client.close()

    def test_base_url_strips_trailing_slash(self, api_key):
        client = EpicodeMemoryClient(api_key=api_key, base_url="https://example.com/")
        assert client._base_url == "https://example.com"
        client.close()


class TestHealth:
    def test_health_does_not_send_api_key(self, client):
        with mock.patch.object(client._session, "request") as mock_request:
            mock_request.return_value = mock.Mock(
                ok=True,
                status_code=200,
                json=lambda: {"status": "ok"},
                text='{"status": "ok"}',
            )
            result = client.health()
            mock_request.assert_called_once()
            args, kwargs = mock_request.call_args
            assert args[0] == "GET"
            assert args[1] == "https://example.com/health"
            assert "X-API-Key" not in kwargs["headers"]
            assert result == {"status": "ok"}


class TestRemember:
    def test_remember_sends_correct_payload(self, client):
        with mock.patch.object(client._session, "request") as mock_request:
            mock_request.return_value = mock.Mock(
                ok=True,
                status_code=200,
                json=lambda: {"id": 1, "status": "created"},
                text='{"id": 1, "status": "created"}',
            )
            result = client.remember("hello world")
            args, kwargs = mock_request.call_args
            assert args[0] == "POST"
            assert args[1] == "https://example.com/api/v1/remember"
            assert kwargs["json"] == {"content": "hello world"}
            assert kwargs["headers"]["X-API-Key"] == "tm-test-key"
            assert result == {"id": 1, "status": "created"}


class TestSearch:
    def test_search_sends_query_and_limit(self, client):
        with mock.patch.object(client._session, "request") as mock_request:
            mock_request.return_value = mock.Mock(
                ok=True,
                status_code=200,
                json=lambda: {"results": []},
                text='{"results": []}',
            )
            client.search("test query", limit=5)
            args, kwargs = mock_request.call_args
            assert args[0] == "POST"
            assert args[1] == "https://example.com/api/v1/search"
            assert kwargs["json"] == {"query": "test query", "limit": 5}

    def test_search_omits_limit_when_none(self, client):
        with mock.patch.object(client._session, "request") as mock_request:
            mock_request.return_value = mock.Mock(
                ok=True,
                status_code=200,
                json=lambda: {"results": []},
                text='{"results": []}',
            )
            client.search("test query")
            call_kwargs = mock_request.call_args.kwargs
            assert call_kwargs["json"] == {"query": "test query"}


class TestRecall:
    def test_recall_sends_query_and_depth(self, client):
        with mock.patch.object(client._session, "request") as mock_request:
            mock_request.return_value = mock.Mock(
                ok=True,
                status_code=200,
                json=lambda: {"fragments": []},
                text='{"fragments": []}',
            )
            client.recall("test query", depth=3)
            call_kwargs = mock_request.call_args.kwargs
            assert call_kwargs["json"] == {"query": "test query", "depth": 3}


class TestAsk:
    def test_ask_sends_question_and_depth(self, client):
        with mock.patch.object(client._session, "request") as mock_request:
            mock_request.return_value = mock.Mock(
                ok=True,
                status_code=200,
                json=lambda: {"answer": "yes"},
                text='{"answer": "yes"}',
            )
            client.ask("what?", depth=2)
            call_kwargs = mock_request.call_args.kwargs
            assert call_kwargs["json"] == {"question": "what?", "depth": 2}


class TestErrorHandling:
    def test_network_error_is_wrapped(self, client):
        import requests

        with mock.patch.object(
            client._session, "request", side_effect=requests.ConnectionError("boom")
        ):
            with pytest.raises(EpicodeMemoryError, match="Network error"):
                client.health()

    def test_http_error_uses_response_body_message(self, client):
        with mock.patch.object(client._session, "request") as mock_request:
            mock_request.return_value = mock.Mock(
                ok=False,
                status_code=401,
                json=lambda: {"error": "invalid API key", "success": False},
                text='{"error": "invalid API key"}',
            )
            with pytest.raises(EpicodeMemoryError, match="invalid API key") as exc_info:
                client.remember("x")
            assert exc_info.value.status_code == 401
            assert exc_info.value.response_body == {
                "error": "invalid API key",
                "success": False,
            }

    def test_http_error_falls_back_to_status_text(self, client):
        with mock.patch.object(client._session, "request") as mock_request:
            mock_request.return_value = mock.Mock(
                ok=False,
                status_code=500,
                json=lambda: {},
                text="Internal Server Error",
            )
            with pytest.raises(EpicodeMemoryError, match="Internal Server Error"):
                client.remember("x")


class TestCLI:
    def test_health_subcommand_works_without_key(self, capsys):
        """CLI health should not require an API key."""
        with mock.patch(
            "epicode_memory._health_from_url",
            return_value={"status": "ok", "success": True, "version": "1.0.1"},
        ):
            with mock.patch.dict("os.environ", {}, clear=True):
                exit_code = main(["health"])
        captured = capsys.readouterr()
        assert exit_code == 0
        result = json.loads(captured.out)
        assert result["status"] == "ok"

    def test_health_subcommand_forwards_network_error_without_key(self, capsys):
        """CLI health should still report connectivity errors when no key is set."""
        with mock.patch(
            "epicode_memory._health_from_url",
            side_effect=EpicodeMemoryError("Network error: boom"),
        ):
            with mock.patch.dict("os.environ", {}, clear=True):
                exit_code = main(["health"])
        captured = capsys.readouterr()
        assert exit_code == 1
        assert "Network error" in captured.err

    def test_remember_subcommand_success(self, capsys, api_key):
        with mock.patch(
            "epicode_memory.EpicodeMemoryClient.remember",
            return_value={"id": 42, "status": "created"},
        ):
            with mock.patch.dict("os.environ", {"EPICODE_API_KEY": api_key}):
                exit_code = main(["remember", "hello"])
        captured = capsys.readouterr()
        assert exit_code == 0
        assert json.loads(captured.out)["id"] == 42

    def test_remember_subcommand_error(self, capsys, api_key):
        with mock.patch(
            "epicode_memory.EpicodeMemoryClient.remember",
            side_effect=EpicodeMemoryError("bad request", status_code=400),
        ):
            with mock.patch.dict("os.environ", {"EPICODE_API_KEY": api_key}):
                exit_code = main(["remember", "hello"])
        captured = capsys.readouterr()
        assert exit_code == 1
        assert "bad request" in captured.err
        assert "Status: 400" in captured.err


class TestContextManager:
    def test_client_closes_on_exit(self, api_key):
        client = EpicodeMemoryClient(api_key=api_key)
        with mock.patch.object(client, "close") as mock_close:
            with client:
                pass
            mock_close.assert_called_once()
