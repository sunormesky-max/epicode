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
