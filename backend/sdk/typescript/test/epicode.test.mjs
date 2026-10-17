import assert from "node:assert/strict";
import { test } from "node:test";

import { EpicodeClient, EpicodeError } from "../dist/epicode.js";

const envelope = (data, ok = true, tool = "memory_search") => ({
  protocol: {
    schema_version: "1.0",
    tool,
    ok,
    error: ok ? null : { code: 400, message: "query rejected" },
  },
  data: ok ? data : null,
  status: { identity: { system: "Epicode" }, space: { memories: 1 } },
});

async function withFetch(mockFetch, run) {
  const previousFetch = globalThis.fetch;
  globalThis.fetch = mockFetch;
  try {
    await run();
  } finally {
    globalThis.fetch = previousFetch;
  }
}

test("search unwraps SMRP and preserves retrieval context", async () => {
  const body = envelope({
    query: "deployment",
    results: [
      {
        id: 7,
        content: "Deployed v2",
        labels: ["ops"],
        similarity: 0.82,
        tier: "experiential",
        source: ["vector", "bm25"],
      },
    ],
    tiers: { experiential: [] },
    total_found: 1,
    offset: 5,
    limit: 10,
  });

  await withFetch(async (url, init) => {
    assert.equal(url, "https://example.test/api/v1/search");
    assert.equal(init.headers["X-API-Key"], "key");
    assert.deepEqual(JSON.parse(init.body), {
      query: "deployment",
      limit: 10,
      offset: 5,
      mode: "fusion",
    });
    return new Response(JSON.stringify(body), {
      status: 200,
      headers: { "Content-Type": "application/json" },
    });
  }, async () => {
    const client = new EpicodeClient("key", "https://example.test/api/v1");
    const result = await client.search("deployment", {
      limit: 10,
      offset: 5,
      mode: "fusion",
    });
    assert.equal(result.success, true);
    assert.equal(result.total, 1);
    assert.equal(result.results[0].id, "7");
    assert.equal(result.results[0].tier, "experiential");
    assert.deepEqual(result.results[0].source, ["vector", "bm25"]);
    assert.equal(result.smrp.data.query, "deployment");
  });
});

test("remember and recall unwrap current SMRP payloads", async () => {
  const responses = [
    envelope({ id: 12, status: "created", labels: ["ops"] }, true, "memory_create"),
    envelope({
      query: "deployment",
      depth: 2,
      seed_count: 1,
      associated_count: 1,
      total_fragments: 2,
      emotion: { pleasure: 0.2, arousal: 0.3, dominance: 0.4 },
      tiers: { primary: [], contextual: [] },
    }, true, "memory_recall"),
  ];
  const requestBodies = [];

  await withFetch(async (_url, init) => {
    requestBodies.push(JSON.parse(init.body));
    return new Response(JSON.stringify(responses.shift()), { status: 200 });
  }, async () => {
    const client = new EpicodeClient("key", "https://example.test/api/v1");
    const remembered = await client.remember("Deployed v2", ["ops"]);
    const recall = await client.recall("deployment", 2);
    assert.equal(remembered.id, "12");
    assert.equal(remembered.smrp.protocol.tool, "memory_create");
    assert.equal(recall.seed_count, 1);
    assert.equal(recall.emotion.arousal, 0.3);
  });

  assert.deepEqual(requestBodies, [
    { content: "Deployed v2", labels: ["ops"] },
    { query: "deployment", depth: 2 },
  ]);
});

test("ask exposes structured source memories", async () => {
  const body = envelope({
    question: "What happened?",
    answer: "A grounded answer.",
    memory_count: 1,
    memories: [
      {
        id: 12,
        labels: ["ops"],
        content: "Deployed v2",
        relevance: 0.9,
      },
    ],
    knowledge_card_used: "deployment",
  }, true, "memory_ask");

  await withFetch(async () => new Response(JSON.stringify(body), { status: 200 }), async () => {
    const client = new EpicodeClient("key", "https://example.test/api/v1");
    const result = await client.ask("What happened?");
    assert.equal(result.answer, "A grounded answer.");
    assert.equal(result.memories[0].id, 12);
    assert.equal(result.memories[0].content, "Deployed v2");
    assert.equal(result.knowledge_card_used, "deployment");
  });
});

test("SMRP tool errors throw instead of becoming false success", async () => {
  const body = envelope({}, false);
  await withFetch(async () => new Response(JSON.stringify(body), { status: 200 }), async () => {
    const client = new EpicodeClient("key", "https://example.test/api/v1");
    await assert.rejects(client.search("invalid"), (error) => {
      assert.ok(error instanceof EpicodeError);
      assert.equal(error.status, 200);
      assert.equal(error.message, "Epicode API error 200: query rejected");
      return true;
    });
  });
});

// --- Methods that previously hit non-existent routes / wrong fields. Shapes mirror the
// real backend handlers; backend/tests/sdk_contract.rs checks every SDK route exists.
function recorder(responseFor) {
  const calls = [];
  const fetch = async (url, init) => {
    const body = init.body ? JSON.parse(init.body) : undefined;
    calls.push({ url, method: init.method, body, headers: init.headers });
    return new Response(JSON.stringify(responseFor(url, body)), {
      status: 200,
      headers: { "Content-Type": "application/json" },
    });
  };
  return { calls, fetch };
}

test("identityStep sends value", async () => {
  const r = recorder(() =>
    envelope({ step: 1, progress: { current_step: 2 }, next_prompt: "Why?", pending: {} }, true, "identity_step")
  );
  await withFetch(r.fetch, async () => {
    const res = await new EpicodeClient("key", "https://x.test/api/v1").identityStep(1, "Ada");
    assert.equal(r.calls[0].url, "https://x.test/api/v1/identity/step");
    assert.deepEqual(r.calls[0].body, { step: 1, value: "Ada" });
    assert.equal(res.progress.current_step, 2);
    assert.equal(res.next_prompt, "Why?");
  });
});

test("recallWithTiers uses /recall and real SMRP tiers", async () => {
  const item = { id: 3, content: "c", labels: ["rust"], source: ["vector"], similarity: 0.9 };
  const r = recorder(() =>
    envelope({ query: "q", tiers: { primary: [item], contextual: [], experiential: [], hub: [{ ...item, id: 4 }] }, emotion: { pleasure: 0.2 } }, true, "memory_recall")
  );
  await withFetch(r.fetch, async () => {
    const res = await new EpicodeClient("key", "https://x.test/api/v1").recallWithTiers("q");
    assert.equal(r.calls[0].url, "https://x.test/api/v1/recall");
    assert.deepEqual(r.calls[0].body, { query: "q", depth: 2 });
    assert.deepEqual(res.tiers.map((t) => t.length), [1, 0, 0, 1]);
    assert.equal(res.tiers[0][0].id, "3");
    assert.equal(res.tiers[3][0].tier, 4);
    assert.equal(res.tiers[3][0].tier_name, "hub");
    assert.equal(res.tiers[0][0].emotional_valence.pleasure, 0.2);
    assert.equal(res.total_results, 2);
  });
});

test("knowledge sends numeric id; knowledgeGraph builds edges from relations", async () => {
  const r = recorder(() =>
    envelope({ id: 7, relations: [{ target: 9, type: "Semantic", strength: 0.8 }], count: 1 }, true, "knowledge_relations")
  );
  await withFetch(r.fetch, async () => {
    const c = new EpicodeClient("key", "https://x.test/api/v1");
    const k = await c.knowledge("7");
    assert.deepEqual(r.calls[0].body, { id: 7 });
    assert.equal(k.count, 1);
    const g = await c.knowledgeGraph(7);
    assert.equal(r.calls[1].url, "https://x.test/api/v1/knowledge");
    assert.deepEqual(g.nodes.map((n) => n.id), ["7", "9"]);
    assert.deepEqual(g.edges[0], { source: "7", target: "9", relation: "Semantic", strength: 0.8 });
  });
});

test("timeline/getNode/createNode unwrap SMRP; timestamp is numeric", async () => {
  const r = recorder((url) =>
    url.endsWith("/timeline")
      ? envelope({ events: [{ id: 1 }], total: 1 }, true, "timeline")
      : url.endsWith("/nodes")
        ? envelope({ id: 6 }, true, "node_create")
        : envelope({ id: 5, content: "x", labels: ["a"] }, true, "memory_get")
  );
  await withFetch(r.fetch, async () => {
    const c = new EpicodeClient("key", "https://x.test/api/v1");
    assert.equal((await c.timeline()).total, 1);
    const n = await c.getNode(5);
    assert.equal(n.id, "5");
    assert.deepEqual(n.labels, ["a"]);
    const created = await c.createNode("x", undefined, "1792030000");
    assert.equal(r.calls[2].body.timestamp, 1792030000);
    assert.equal(created.id, "6");
  });
});

test("dreamCycle calls the MCP dream_cycle tool", async () => {
  const report = "consolidated: 4, connections_formed: 7, merged: 1, evicted: 2, importance_updated: 30, insights: []";
  const r = recorder(() => ({
    jsonrpc: "2.0",
    id: 1,
    result: { structuredContent: envelope({ report, dry_run: true }, true, "dream_cycle"), content: [] },
  }));
  await withFetch(r.fetch, async () => {
    const res = await new EpicodeClient("key", "https://epicode.cn/api/v1").dreamCycle(true);
    assert.equal(r.calls[0].url, "https://epicode.cn/api/mcp");
    assert.equal(r.calls[0].headers["X-API-Key"], "key");
    assert.deepEqual(r.calls[0].body.params, { name: "dream_cycle", arguments: { dry_run: true } });
    assert.deepEqual(
      [res.memories_consolidated, res.new_associations, res.duplicates_merged, res.junk_evicted, res.importance_updated],
      [4, 7, 1, 2, 30]
    );
    assert.equal(res.dry_run, true);
    assert.equal(res.cycles_completed, 0);
  });
});

test("dreamCycle JSON-RPC error throws", async () => {
  const r = recorder(() => ({ jsonrpc: "2.0", id: 1, error: { code: -32602, message: "bad" } }));
  await withFetch(r.fetch, async () => {
    await assert.rejects(new EpicodeClient("key").dreamCycle(), EpicodeError);
    assert.equal(r.calls[0].url, "http://localhost:8080/api/mcp");
  });
});

test("admin routes live at the API root", async () => {
  const r = recorder(() => ({ success: true, total_users: 2, active_engines: 1 }));
  const { EpicodeAdmin } = await import("../dist/epicode.js");
  await withFetch(r.fetch, async () => {
    await new EpicodeAdmin("ak", "https://epicode.cn/api/v1").users();
    assert.equal(r.calls[0].url, "https://epicode.cn/api/admin/users");
    assert.equal(r.calls[0].headers["X-Admin-Key"], "ak");
  });
});
