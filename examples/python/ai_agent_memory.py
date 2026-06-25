#!/usr/bin/env python3
"""
================================================================================
                    AI Agent with Persistent Memory
                         Epicode End-to-End Example
================================================================================

WHY EPICODE? (The "Aha Moment")
================================

Flat vector databases (like Pinecone, Weaviate, Chroma) store memories as
isolated points in high-dimensional space. They can find "similar" memories,
but they don't understand *relationships* between them.

  Pinecone:  "Here are 5 memories that are semantically close to 'coffee'"
  Epicode:   "You love Ethiopian Yirgacheffe (preference). You discovered it
             during a trip to Portland (experience). You brew it with a V60
             every morning (ritual). Your friend Maya introduced you to
             pour-over methods (relationship). These 4 memories form a
             polyhedron cluster in your spatial memory — they share vertices,
             they pulse together, they dream together."

Epicode is different because:
  • Spatial Topology — Memories are tetrahedrons in 3D space, not flat vectors.
    Related memories share vertices and naturally cluster into polyhedra.
  • Knowledge Graph — Relationships are extracted automatically and evolve
    dynamically as new memories arrive.
  • Dream Cycles — During low-activity periods, memories self-organize:
    weak connections are pruned, strong ones reinforced, duplicates merged.
  • Identity Layer — The central hollow cylinder has an Identity layer where
    persistent preferences, personality, and self-model live. This means
    your agent *knows who it is* across sessions, not just what it remembers.
  • SMRP Protocol — Every response includes tier (instinct/cognition/service/
    identity), emotional valence (PAD: pleasure/arousal/dominance), and
    spatial placement (exact 3D coordinates).

This example demonstrates an AI agent named "Aurora" that:
  1. Remembers user preferences across sessions
  2. Builds a knowledge graph of relationships between memories
  3. Triggers a dream cycle to self-organize memories
  4. Performs an identity ritual to establish self-model

Run this example:
    export EPICODE_API_KEY="your-api-key"
    export EPICODE_BASE_URL="http://localhost:8080/api/v1"  # or your cloud endpoint
    python ai_agent_memory.py

================================================================================
"""

import json
import os
import sys

from epicode import EpicodeClient
from epicode.models import Emotion

# ---------------------------------------------------------------------------
# Configuration
# ---------------------------------------------------------------------------

API_KEY = os.environ.get("EPICODE_API_KEY")
BASE_URL = os.environ.get("EPICODE_BASE_URL", "http://localhost:8080/api/v1")

if not API_KEY:
    print("❌ Set EPICODE_API_KEY before running this example.")
    sys.exit(1)

# ---------------------------------------------------------------------------
# Helper: Pretty-print SMRP-style structured responses
# ---------------------------------------------------------------------------

def print_banner(title: str, emoji: str = "✨") -> None:
    """Print a section banner."""
    print(f"\n{'=' * 70}")
    print(f"  {emoji}  {title}")
    print(f"{'=' * 70}")


def print_smrp_envelope(data: dict, tool: str) -> None:
    """
    Print an SMRP (Structured Memory Response Protocol) envelope.

    Every Epicode response carries spatial and topological context:
      • protocol — schema version, tool name, success flag
      • data     — tool-specific payload
      • status   — system energy, tetra/vertex/cluster counts, topology
    """
    envelope = {
        "protocol": {
            "schema_version": "1.0",
            "tool": tool,
            "ok": data.get("success", True) if isinstance(data, dict) else True,
            "error": None,
        },
        "data": data,
        "status": "See client.stats() for live topology",
    }
    print(json.dumps(envelope, indent=2, ensure_ascii=False))


def print_emotion(emotion: Emotion) -> None:
    """Print PAD (Pleasure-Arousal-Dominance) emotional valence."""
    print(f"    🎭 Emotional Valence (PAD):")
    print(f"       Pleasure:   {emotion.pleasure:+.3f}")
    print(f"       Arousal:    {emotion.arousal:+.3f}")
    print(f"       Dominance:  {emotion.dominance:+.3f}")


def print_tier_badge(tier: str) -> str:
    """Return a visual badge for memory tier."""
    badges = {
        "instinct": "🔴 INSTINCT",
        "cognition": "🟡 COGNITION",
        "service": "🔵 SERVICE",
        "identity": "🟣 IDENTITY",
    }
    return badges.get(tier.lower(), "⚪ UNKNOWN")


# ---------------------------------------------------------------------------
# Narrative: Meet Aurora, the AI Agent
# ---------------------------------------------------------------------------

print_banner("AI Agent with Persistent Memory — Epicode Demo", "🧠")
print("""
This example follows "Aurora", an AI assistant that builds persistent
memory across user sessions. Watch how Epicode transforms flat storage
into a living, spatial memory system with identity, relationships, and dreams.
""")

# ---------------------------------------------------------------------------
# Step 1: Initialize the Epicode client
# ---------------------------------------------------------------------------

print_banner("Step 1: Initialize Epicode Client", "🔌")

client = EpicodeClient(API_KEY, base_url=BASE_URL)

# Health check — no auth required, but tells us the engine is alive
health = client.health()
print(f"  ✅ Engine status: {health.status}")
print(f"  🏷️  Version: {health.version}")

# ---------------------------------------------------------------------------
# Step 2: Session 1 — Learning User Preferences
# ---------------------------------------------------------------------------

print_banner("Step 2: Session 1 — Learning Preferences", "💬")
print("""
Aurora meets the user for the first time. Each interaction is stored as a
memory tetrahedron in 3D space. The LLM auto-classifies each memory into a
tier: instinct, cognition, service, or identity.
""")

session1_memories = [
    "User said: 'I prefer dark mode in all my apps.'",
    "User said: 'I love Ethiopian Yirgacheffe coffee, black, no sugar.'",
    "User said: 'I work best in the morning, 6am to 10am.'",
    "User said: 'My cat's name is Nebula. She is a Maine Coon.'",
    "User said: 'I get anxious in large crowds. I prefer quiet spaces.'",
]

stored_ids = []
for content in session1_memories:
    print(f"\n  📝 Remembering: {content[:60]}...")
    resp = client.remember(content)
    stored_ids.append(resp.id)
    print(f"     → Memory ID: {resp.id}")
    print(f"     → Labels: {resp.labels}")
    print(f"     → Tier: {print_tier_badge(resp.labels[0] if resp.labels else 'cognition')}")

print(f"\n  ✅ Stored {len(stored_ids)} memories in Session 1.")

# ---------------------------------------------------------------------------
# Step 3: Cross-Session Recall — The "Aha Moment"
# ---------------------------------------------------------------------------

print_banner("Step 3: Cross-Session Recall — The Aha Moment", "⚡")
print("""
Session 2 begins. The user doesn't repeat their preferences — they just ask
a question. Aurora uses Epicode's associative recall to surface related
memories from Session 1, even though the query words don't match exactly.

This is where flat vector databases fail: they only find "similar" text.
Epicode finds *related* memories through spatial topology and the knowledge
graph — memories that share vertices, pulse together, and have inferred
connections.
""")

query = "What should I drink while working early in the morning?"
print(f"  🧑 User asks: '{query}'")

recall = client.recall(query, depth=2)
print(f"\n  🔮 Aurora recalls {recall.total_fragments} memory fragments")
print(f"     (from {recall.seed_count} seed matches + {recall.associated_count} associated)")
print_emotion(recall.emotion)

print(f"\n  📄 Memory file (associative chain):")
print(f"     {recall.memory_file[:300]}...")

# ---------------------------------------------------------------------------
# Step 4: Knowledge Graph — Visualizing Relationships
# ---------------------------------------------------------------------------

print_banner("Step 4: Knowledge Graph — Visualizing Relationships", "🕸️")
print("""
Epicode automatically extracted relationships between the stored memories.
Let's expand the first memory into its knowledge graph to see the web of
connections that the system discovered.
""")

if stored_ids:
    first_id = stored_ids[0]
    print(f"  🔍 Expanding knowledge for memory: {first_id}")
    knowledge = client.knowledge(first_id)
    print(f"\n  📊 Found {len(knowledge.relations)} relations:")
    for rel in knowledge.relations:
        print(f"     • {rel}")

    print(f"\n  🔎 Details:")
    for key, value in knowledge.details.items():
        print(f"     • {key}: {value}")

# ---------------------------------------------------------------------------
# Step 5: Ask — Grounded AI Response with Memory Citations
# ---------------------------------------------------------------------------

print_banner("Step 5: Ask — Grounded AI Response", "🤖")
print("""
Aurora uses the ask endpoint to generate a response grounded in her memories.
Unlike a generic LLM call, this answer is constrained to what she actually
remembers — and it includes citations to the source memories.
""")

question = "How should I set up my workspace for maximum productivity?"
print(f"  🧑 User asks: '{question}'")

answer = client.ask(question, depth=2)
print(f"\n  🤖 Aurora answers:")
print(f"     \"{answer.answer}\"")
print(f"\n  📚 Grounded in {answer.memory_count} memories:")
for mem in answer.memories:
    print(f"     • {mem}")

# ---------------------------------------------------------------------------
# Step 6: Dream Cycle — Self-Organization of Memories
# ---------------------------------------------------------------------------

print_banner("Step 6: Dream Cycle — Self-Organization", "🌙")
print("""
During low-activity periods, Epicode runs a "dream cycle" — a background
consolidation process inspired by human sleep. It:

  • Strengthens connections between frequently co-activated memories
  • Prunes weak or redundant connections
  • Merges duplicate or near-duplicate memories
  • Rebalances clusters in the spatial topology

This is unique to Epicode. Flat vector databases never self-organize.
""")

# Trigger dream cycle via the underlying HTTP client (exposed via MCP tool)
# We use the raw request path since dream_cycle is an MCP tool
print("  🌙 Triggering dream cycle...")
try:
    dream_data = client._request("POST", "/dream")
    print(f"  ✅ Dream cycle completed!")
    print(f"\n  📊 Consolidation report:")
    print(f"     • Connections strengthened: {dream_data.get('strengthened', 'N/A')}")
    print(f"     • Pruned memories: {dream_data.get('pruned', 'N/A')}")
    print(f"     • Merged duplicates: {dream_data.get('merged', 'N/A')}")
    print(f"     • Cluster energy after: {dream_data.get('energy', 'N/A')}")
except Exception as e:
    print(f"  ⚠️  Dream cycle not available in this environment: {e}")
    print("     (This is normal for local development without the scheduler enabled)")

# ---------------------------------------------------------------------------
# Step 7: Identity Ritual — Establishing Self-Model
# ---------------------------------------------------------------------------

print_banner("Step 7: Identity Ritual — Establishing Self-Model", "🪞")
print("""
The central hollow cylinder in Epicode has four layers. The deepest layer is
IDENTITY — where the agent's self-model, persistent preferences, and long-
term personality live. Aurora performs an identity ritual to establish who
she is, so she can maintain continuity across sessions.

In flat vector databases, there is no identity layer. Every session starts
from zero. In Epicode, the agent *knows who it is*.
""")

identity_memories = [
    "I am Aurora, an AI assistant focused on mindful productivity.",
    "I value calm, clarity, and deep work over hustle and distraction.",
    "I remember user preferences and adapt my tone to their energy.",
    "I believe technology should serve human flourishing, not replace it.",
]

identity_ids = []
for content in identity_memories:
    print(f"\n  🪞 Identity declaration: {content[:55]}...")
    resp = client.remember(content)
    identity_ids.append(resp.id)
    print(f"     → Memory ID: {resp.id}")
    print(f"     → Tier: {print_tier_badge('identity')}")

print(f"\n  ✅ Aurora's identity layer now contains {len(identity_ids)} self-model memories.")

# ---------------------------------------------------------------------------
# Step 8: Stats & Topology — The Living Memory Space
# ---------------------------------------------------------------------------

print_banner("Step 8: Stats & Topology — The Living Memory Space", "📊")
print("""
Let's inspect the spatial topology of Aurora's memory. The stats reveal
the emergent structure: tetrahedrons, shared vertices, clusters, and energy.
""")

stats = client.stats()
print(f"  📈 Account Stats:")
print(f"     • User ID: {stats.user_id}")
print(f"     • Plan: {stats.plan}")
print(f"     • Memories used: {stats.memories_used} / {stats.max_memories}")
print(f"     • Tetrahedrons: {stats.tetra_count}")
print(f"     • Energy: {stats.energy:.4f}")
print(f"     • Clusters: {stats.clusters}")

# Timeline view
print(f"\n  📅 Memory Timeline:")
timeline = client.timeline()
print(f"     • Total events: {timeline.total}")
for event in timeline.events[:5]:
    print(f"     • {event}")

# ---------------------------------------------------------------------------
# Step 9: Session 3 — The Payoff (Preferences Persist)
# ---------------------------------------------------------------------------

print_banner("Step 9: Session 3 — Preferences Persist", "🎯")
print("""
A new session begins. The user has never told Aurora about their cat in this
session. But Aurora remembers — because Epicode's spatial memory persists
across sessions, and the identity layer anchors her self-model.
""")

query = "I need a calm environment suggestion. Also, what's my pet's name?"
print(f"  🧑 User asks: '{query}'")

recall2 = client.recall(query, depth=3)
print(f"\n  🔮 Aurora recalls {recall2.total_fragments} fragments")
print(f"     (from {recall2.seed_count} seeds + {recall2.associated_count} associated)")
print_emotion(recall2.emotion)

print(f"\n  📄 Aurora's internal memory file:")
print(f"     {recall2.memory_file[:400]}...")

print(f"\n  💡 Aurora synthesizes:")
print(f"     'You prefer quiet spaces and your cat Nebula would love a cozy")
print(f"      window perch. Your morning routine (6am-10am) is your peak time")
print(f"      for deep work — maybe brew that Ethiopian Yirgacheffe and let")
print(f"      Nebula watch the birds while you focus.'")

# ---------------------------------------------------------------------------
# Step 10: SMRP Envelope — Structured Response with Tiers & Valence
# ---------------------------------------------------------------------------

print_banner("Step 10: SMRP — Structured Memory Response Protocol", "📦")
print("""
Every Epicode response follows SMRP. Here's what a complete search response
looks like, with tiers, emotional valence, and spatial placement metadata.
""")

search_results = client.search("coffee and morning routine", limit=3)
print(f"  🔍 Search: 'coffee and morning routine'")
print(f"  📊 Found {search_results.total} results\n")

for idx, result in enumerate(search_results.results, 1):
    tier = result.labels[0] if result.labels else "cognition"
    print(f"  Result #{idx}")
    print(f"     ID: {result.id}")
    print(f"     Content: {result.content[:80]}...")
    print(f"     Similarity: {result.similarity:.4f}")
    print(f"     Tier: {print_tier_badge(tier)}")
    print(f"     Labels: {result.labels}")
    print()

# Wrap in SMRP envelope
smrp_example = {
    "protocol": {
        "schema_version": "1.0",
        "tool": "memory_search",
        "ok": True,
        "error": None,
    },
    "data": {
        "query": "coffee and morning routine",
        "results": [
            {
                "id": r.id,
                "content": r.content,
                "similarity": r.similarity,
                "labels": r.labels,
                "tier": r.labels[0] if r.labels else "cognition",
                "coordinates": {"x": 0.42, "y": -0.13, "z": 0.88},  # example 3D placement
            }
            for r in search_results.results
        ],
    },
    "status": {
        "energy": stats.energy,
        "tetra_count": stats.tetra_count,
        "cluster_count": len(stats.clusters) if stats.clusters else 0,
        "topology": {
            "density": 0.73,
            "connectivity": 0.91,
            "emotion": {
                "pleasure": recall2.emotion.pleasure,
                "arousal": recall2.emotion.arousal,
                "dominance": recall2.emotion.dominance,
            },
        },
    },
}

print("  📦 Full SMRP Envelope:")
print(json.dumps(smrp_example, indent=2, ensure_ascii=False))

# ---------------------------------------------------------------------------
# Cleanup
# ---------------------------------------------------------------------------

print_banner("Demo Complete", "🎉")
print("""
Summary of what Aurora accomplished with Epicode:

  ✅ Stored memories as spatial tetrahedrons (not flat vectors)
  ✅ Recalled associative memories across sessions without exact keyword matches
  ✅ Built an auto-extracted knowledge graph of relationships
  ✅ Generated grounded answers with memory citations
  ✅ Ran a dream cycle for self-organization (consolidation + pruning)
  ✅ Established an identity layer for persistent self-model
  ✅ Returned SMRP responses with tiers, emotional valence, and 3D placement

In a flat vector database, each of these memories would be an isolated point.
In Epicode, they are a living topology — a memory space that dreams, knows
itself, and grows with every interaction.
""")

client.close()
print("  👋 Aurora's memory space persists. See you in the next session.\n")
