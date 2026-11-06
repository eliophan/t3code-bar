#!/usr/bin/env python3 -I
import json
import os
import random

NOW = 1793956080000
HOUR = 3_600_000
DAY = 24 * HOUR
ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "state")

MODELS = [
    "claude-sonnet-5-5", "<synthetic>", "gpt-6.1-sol", "gpt-6-luna", "gpt-6-stella", "anthropic/claude-sonnet-5-5",
    "grok-5", "muse-unpriced-x", "opus", "claude-opus-9[1m]", "custom-model", "old-name", "chain-a", "loop-x",
    "claude-haiku-4-5", "foo-model", "bar-model", "dual-model",
]
M = {name: index for index, name in enumerate(MODELS)}
SESSIONS = ["", "sess-a", "sess-b", "sess-c", "sess-d", "sess-e", "sess-f", "sess-g"]

RATES = {
    "claude-sonnet-5-5": {"input_cost_per_token": 3e-6, "output_cost_per_token": 1.5e-5,
                          "cache_read_input_token_cost": 3e-7, "cache_creation_input_token_cost": 3.75e-6},
    "claude-opus-9": {"input_cost_per_token": 1.5e-5, "output_cost_per_token": 7.5e-5},
    "gpt-6.1-sol": {"input_cost_per_token": 2e-6, "output_cost_per_token": 1e-5},
    "gpt-6-luna": {"input_cost_per_token": 1e-7, "output_cost_per_token": 5e-7, "provider_specific_entry": {"fast": 2}},
    "gpt-6-stella": {"input_cost_per_token": 1e-6, "output_cost_per_token": 4e-6, "cache_read_input_token_cost": 1e-7,
                     "input_cost_per_token_priority": 2e-6, "output_cost_per_token_priority": 8e-6,
                     "input_cost_per_token_ultrafast": 4e-6, "output_cost_per_token_ultrafast": 1.6e-5,
                     "cache_read_input_token_cost_ultrafast": 3e-7},
    "grok-5": {"input_cost_per_token": 5e-6, "output_cost_per_token": 1.5e-5, "cache_read_input_token_cost": 1.25e-6},
    "anthropic/claude-haiku-4-5": {"input_cost_per_token": 1e-6, "output_cost_per_token": 5e-6,
                                   "cache_read_input_token_cost": 1e-7, "cache_creation_input_token_cost": 1.25e-6},
    "openrouter/foo-model": {"input_cost_per_token": 1e-6, "output_cost_per_token": 2e-6},
    "azure/foo-model": {"input_cost_per_token": 2e-6, "output_cost_per_token": 3e-6},
    "openrouter/bar-model": {"input_cost_per_token": 1e-6, "output_cost_per_token": 2e-6},
    "azure/bar-model": {"input_cost_per_token": 1e-6, "output_cost_per_token": 2e-6},
    "openrouter/dual-model": {"input_cost_per_token": 9e-6, "output_cost_per_token": 9e-6},
    "anthropic/dual-model": {"input_cost_per_token": 4e-6, "output_cost_per_token": 6e-6},
    "broken-model": {"input_cost_per_token": 1e-6},
    "sample_spec": {"max_tokens": "set to max_output_tokens if provider specifies it", "mode": "one of chat"},
}

SETTINGS = {
    "usagePriceOverrides": {"custom-model": {"inputCostPerMillionTokens": 5, "outputCostPerMillionTokens": 20,
                                             "cacheReadCostPerMillionTokens": 0.5}},
    "usageModelAliases": {"old-name": "claude-sonnet-5-5", "chain-a": "chain-b", "chain-b": "gpt-6.1-sol",
                          "loop-x": "loop-y", "loop-y": "loop-x"},
    "providerInstances": {"example": {"environment": {"API_TOKEN": "not-a-real-secret"}}},
}

rng = random.Random(20261106)


def row(ts, model, session, unc, cached, creation, out, reasoning=0, key=None, cost=None, speed=0, rate=None, turn=None):
    values = [ts, M[model], SESSIONS.index(session) if isinstance(session, str) else session, unc, cached, creation, out,
              reasoning, key, cost, speed, None if rate is None else M[rate]]
    if turn is not None:
        values.append(turn)
    return values


def at(days_ago, hours=0, minutes=0):
    return NOW - int(days_ago * DAY) - hours * HOUR - minutes * 60_000


def scatter(count, models, sessions, key_prefix=None, speeds=(0,), max_age_days=100):
    out = []
    for index in range(count):
        ts = NOW - rng.randrange(0, max_age_days * DAY)
        model = rng.choice(models)
        cached = rng.randrange(0, 200_000)
        out.append(row(ts, model, rng.choice(sessions), rng.randrange(0, 30_000), cached, rng.randrange(0, 40_000),
                       rng.randrange(0, 8_000), rng.randrange(0, 500),
                       key=f"{key_prefix}{index}" if key_prefix else None, speed=rng.choice(speeds)))
    out.sort(key=lambda r: r[0])
    return out


edge = []
for label, ts in {
    "24h-start": NOW - DAY, "24h-before": NOW - DAY - 1, "24h-last": NOW - 1, "24h-end": NOW,
    "7d-start": 1793552400000, "7d-before": 1793552400000 - 1,
    "30d-start": 1791392400000, "30d-before": 1791392400000 - 1,
    "90d-start": 1786208400000, "90d-before": 1786208400000 - 1,
    "today-last": 1794074400000 - 1, "today-next": 1794074400000,
}.items():
    edge.append(row(ts, "claude-sonnet-5-5", "sess-g", 1000, 2000, 300, 400, key=f"edge-{label}"))

claude_a = scatter(60, ["claude-sonnet-5-5", "old-name", "claude-haiku-4-5", "claude-opus-9[1m]", "opus", "custom-model", "loop-x"],
                   ["sess-a", "sess-b", "sess-c"], key_prefix="msg-a-")
claude_a += edge
claude_a += [
    row(at(1, 3), "<synthetic>", "sess-a", 5, 0, 0, 5, key="synthetic-1"),
    row(at(2), "claude-sonnet-5-5", "sess-a", 100, 0, 0, 50, key="msg-a-0"),
    row(at(1), "custom-model", "sess-b", 120_000, 30_000, 5_000, 9_000, key="custom-reported", cost=7.77),
    row(at(1, 1), "claude-sonnet-5-5", "sess-c", 4_000, 90_000, 2_000, 1_200, key="reported-sonnet", cost=0.9, speed=1),
    row(at(0, 5), "claude-sonnet-5-5", "sess-c", 10, 0, 0, 10, key="tiny"),
    row(at(3), "foo-model", "sess-a", 50_000, 0, 0, 2_000, key="foo-1"),
    row(at(3, 1), "bar-model", "sess-a", 50_000, 0, 0, 2_000, key="bar-1"),
    row(at(3, 2), "dual-model", "sess-a", 50_000, 0, 0, 2_000, key="dual-1"),
    row(at(4), "gpt-6.1-sol", "sess-b", 30_804, 0, 0, 61, key="gpt-via-claude"),
]
claude_a.sort(key=lambda r: r[0])

claude_b = scatter(25, ["claude-sonnet-5-5"], ["sess-a", "sess-d"], key_prefix="msg-b-")
claude_b += [row(at(2), "claude-sonnet-5-5", "sess-a", 777, 0, 0, 77, key="msg-a-0"),
             row(at(1, 2), "chain-a", "sess-d", 8_000, 0, 0, 800, key="chain-1")]
claude_b.sort(key=lambda r: r[0])

history = [row(at(2, h), "gpt-6.1-sol", "sess-a", 20_000 + h, 4_000, 0, 600 + h, 100) for h in range(0, 30, 3)]
history += [row(at(2, 4), "gpt-6.1-sol", "sess-a", 20_000, 4_000, 0, 600, 100)] * 2
codex_parent = history + [row(at(1, h), "gpt-6-luna", "sess-a", 140_552, 0, 0, 925, 529, speed=1) for h in (1, 5)]
codex_parent.sort(key=lambda r: r[0])
codex_fork = list(codex_parent) + [
    row(at(0, 2), "gpt-6-stella", "sess-a", 50_000, 400_000, 0, 3_000, 800, speed=2),
    row(at(0, 3), "gpt-6-stella", "sess-a", 50_000, 400_000, 0, 3_000, 800, speed=1),
    row(at(0, 4), "gpt-6-stella", "sess-a", 50_000, 400_000, 0, 3_000, 800),
]
codex_fork.sort(key=lambda r: r[0])
codex_solo = scatter(80, ["gpt-6.1-sol", "gpt-6-luna", "gpt-6-stella"], ["sess-e", "sess-f"], speeds=(0, 0, 1, 2))

pi_rows = [
    row(at(1, 6), "anthropic/claude-sonnet-5-5", "sess-d", 10, 900, 50, 40, 5, key='["pi","m1","a","anthropic/claude-sonnet-5-5"]',
        cost=0.01, rate="claude-sonnet-5-5"),
    row(at(1, 7), "anthropic/claude-sonnet-5-5", "sess-d", 10_000, 0, 0, 2_000, 0, key='["pi","m2","b","anthropic/claude-sonnet-5-5"]',
        cost=0.10, rate="claude-sonnet-5-5"),
    row(at(5), "grok-5", "sess-e", 1_000, 600, 100, 50, 20, key="s:p1:grok-5", cost=0.002),
]
grok_rows = scatter(20, ["grok-5"], ["sess-g"], key_prefix="grok-")
muse_rows = scatter(15, ["muse-unpriced-x", "gpt-6.1-sol"], ["sess-f", "sess-g"], key_prefix="muse-")
muse_rows.append(row(at(0, 6), "muse-unpriced-x", "sess-f", 5_000, 0, 0, 500, key="muse-reported", cost=0.33))
muse_rows.sort(key=lambda r: r[0])

recent_mtime = NOW - 1_000.25
files = {
    "/home/u/.claude/projects/p1/a.jsonl": ("claude", recent_mtime, claude_a[:-3], claude_a[-3:]),
    "/home/u/.claude/projects/p1/b.jsonl": ("claude", recent_mtime - 5.5, claude_b, []),
    "/home/u/.claude/projects/p1/subagents/c.jsonl": ("claude", recent_mtime - 9, scatter(10, ["claude-sonnet-5-5"], ["sess-d"], "msg-c-"), []),
    "/home/u/.claude/projects/old/stale.jsonl": ("claude", NOW - 100 * DAY + 0.5, [row(at(0, 1), "claude-sonnet-5-5", "sess-a", 1_000_000, 0, 0, 1_000_000, key="stale-recent")], []),
    "/home/u/.claude/projects/old/older.jsonl": ("claude", NOW - 40 * DAY + 0.25, [row(at(10), "claude-sonnet-5-5", "sess-b", 2_000_000, 0, 0, 100_000, key="older-1")], []),
    "/home/u/.claude/elsewhere/x.jsonl": ("claude", recent_mtime, [row(at(1), "claude-sonnet-5-5", "sess-a", 999, 0, 0, 999, key="outside-source")], []),
    "/home/u/.codex/sessions/2026/11/parent.jsonl": ("codex", recent_mtime - 20, codex_parent, []),
    "/home/u/.codex/sessions/2026/11/fork.jsonl": ("codex", recent_mtime - 10, codex_fork, []),
    "/home/u/.codex/sessions/2026/10/solo.jsonl": ("codex", recent_mtime - 30, codex_solo, []),
    "/home/u/.pi/agent/sessions/p.jsonl": ("pi", recent_mtime, pi_rows, []),
    "/home/u/.grok/sessions/s/updates.jsonl": ("grok", recent_mtime, grok_rows, []),
    "/home/u/.local/share/muse/sessions/m/session.jsonl": ("muse", recent_mtime, muse_rows, []),
}

sources = {}
for directory, kind in [("/home/u/.local/share/muse/sessions", "muse"), ("/home/u/.codex/sessions", "codex"),
                        ("/home/u/.claude/projects", "claude"), ("/home/u/.pi/agent/sessions", "pi"), ("/home/u/.grok/sessions", "grok")]:
    sources[f"{kind}\u0000{directory}"] = {"dir": directory, "volumeId": "16777234:1"}

cache = {
    "sources": sources, "version": 11, "models": MODELS, "sessions": SESSIONS,
    "files": {
        path: {"s": 1000, "m": mtime, "p": kind, "r": rows, "t": tail, "mr": 1 if kind == "claude" else 0, "o": 0, "gl": 0, "gh": 0, "cs": None}
        for path, (kind, mtime, rows, tail) in files.items()
    },
}

os.makedirs(ROOT, exist_ok=True)
with open(os.path.join(ROOT, "usage-scan-cache-v11.json"), "w") as handle:
    json.dump(cache, handle, separators=(",", ":"))
with open(os.path.join(ROOT, "usage-model-rates.json"), "w") as handle:
    json.dump({"fetchedAtMs": NOW - HOUR, "document": RATES}, handle, indent=1)
with open(os.path.join(ROOT, "settings.json"), "w") as handle:
    json.dump(SETTINGS, handle, indent=1)
