"""Unit tests for the Epicode MCP server bridge."""

import sys
from pathlib import Path
from unittest import mock

import pytest

# Add the bridge source directory to the import path.
BRIDGE_DIR = Path(__file__).resolve().parent.parent / "mcp-bridge"
sys.path.insert(0, str(BRIDGE_DIR))

import epicode_mcp_server as server


@pytest.fixture
def api_key():
    return "tm-test-key"


@pytest.fixture(autouse=True)
def reset_client():
    """Reset the lazy client singleton between tests."""
    server._client = None
    yield
    server._client = None


class TestClientInitialization:
    def test_requires_api_key(self):
        with mock.patch.dict("os.environ", {}, clear=True):
            with pytest.raises(server.EpicodeClientError, match="API key is required"):
                server.EpicodeClient()

    def test_accepts_api_key_from_env(self):
        with mock.patch.dict("os.environ", {"EPICODE_API_KEY": "tm-env-key"}):
            client = server.EpicodeClient()
            assert client._api_key == "tm-env-key"

    def test_default_base_url(self, api_key):
        client = server.EpicodeClient(api_key=api_key)
        assert client._base_url == "https://epicode.cn"


class TestMCPHealth:
    def test_health_tool_calls_unauthenticated_endpoint(self, api_key):
        with mock.patch.dict("os.environ", {"EPICODE_API_KEY": api_key}):
            with mock.patch.object(
                server.EpicodeClient, "health", return_value={"status": "ok"}
            ) as mock_health:
                result = server.health()
                mock_health.assert_called_once()
                assert '"status": "ok"' in result


class TestMCPMemoryCreate:
    def test_memory_create_tool(self, api_key):
        with mock.patch.dict("os.environ", {"EPICODE_API_KEY": api_key}):
            with mock.patch.object(
                server.EpicodeClient,
                "remember",
                return_value={"id": 1, "status": "created"},
            ) as mock_remember:
                result = server.memory_create("hello")
                mock_remember.assert_called_once_with("hello")
                assert '"id": 1' in result


class TestMCPMemorySearch:
    def test_memory_search_tool_passes_limit(self, api_key):
        with mock.patch.dict("os.environ", {"EPICODE_API_KEY": api_key}):
            with mock.patch.object(
                server.EpicodeClient, "search", return_value={"results": []}
            ) as mock_search:
                server.memory_search("query", limit=5)
                mock_search.assert_called_once_with("query", limit=5)

    def test_memory_search_tool_default_limit(self, api_key):
        with mock.patch.dict("os.environ", {"EPICODE_API_KEY": api_key}):
            with mock.patch.object(
                server.EpicodeClient, "search", return_value={"results": []}
            ) as mock_search:
                server.memory_search("query")
                mock_search.assert_called_once_with("query", limit=None)


class TestMCPMemoryRecall:
    def test_memory_recall_tool_passes_depth(self, api_key):
        with mock.patch.dict("os.environ", {"EPICODE_API_KEY": api_key}):
            with mock.patch.object(
                server.EpicodeClient, "recall", return_value={"fragments": []}
            ) as mock_recall:
                server.memory_recall("query", depth=3)
                mock_recall.assert_called_once_with("query", depth=3)


class TestMCPMemoryAsk:
    def test_memory_ask_tool_passes_depth(self, api_key):
        with mock.patch.dict("os.environ", {"EPICODE_API_KEY": api_key}):
            with mock.patch.object(
                server.EpicodeClient, "ask", return_value={"answer": "yes"}
            ) as mock_ask:
                server.memory_ask("question", depth=2)
                mock_ask.assert_called_once_with("question", depth=2)


class TestMCPToolRegistration:
    @pytest.mark.asyncio
    async def test_at_least_five_tools_registered(self):
        tools = await server.mcp.list_tools()
        tool_names = {t.name for t in tools}
        required = {"health", "memory_create", "memory_search", "memory_recall", "memory_ask"}
        assert required.issubset(tool_names), f"Missing tools: {required - tool_names}"
