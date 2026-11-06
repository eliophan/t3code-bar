#!/usr/bin/env python3 -I
import datetime as dt
import importlib.util
import json
import os
import sys

NOW = "2026-11-06T09:08:00Z"
ZONE = "Asia/Ho_Chi_Minh"
LABELS = {1: "Past 24h", 7: "7 days", 30: "30 days", 90: "90 days"}

spec = importlib.util.spec_from_file_location("repro", sys.argv[1])
repro = importlib.util.module_from_spec(spec)
spec.loader.exec_module(repro)

state = os.path.join(os.path.dirname(os.path.abspath(__file__)), "state")
now_ms = int(dt.datetime.fromisoformat(NOW.replace("Z", "+00:00")).timestamp() * 1000)
_, _, doc = repro.load_cache(state)
table = repro.parse_rate_table(json.load(open(os.path.join(state, "usage-model-rates.json")))["document"])
settings = json.load(open(os.path.join(state, "settings.json")))
overrides = repro.override_table(settings.get("usagePriceOverrides"))
raw_aliases = settings.get("usageModelAliases") or {}
aliases = {}
for model in raw_aliases:
    seen, target = {model}, raw_aliases[model]
    while target in raw_aliases and target not in seen:
        seen.add(target)
        target = raw_aliases[target]
    if target not in raw_aliases:
        aliases[model] = target



def series(summary, window):
    """Cost and processed tokens per provider for each hour slot or local day, as T3's chart columns hold them."""
    hourly = window["resolution"] == "hour"
    columns = {}
    for b in summary["buckets"]:
        key = str(b["hour"]) if hourly else b["day"]
        entry = columns.setdefault(key, {}).setdefault(b["provider"], {"cost": 0.0, "tokens": 0})
        entry["cost"] += b["costUsd"]
        entry["tokens"] += sum(b["totals"][:4])
    return {"resolution": "hour" if hourly else "day", "columns": columns}


golden = {"now": NOW, "timeZone": ZONE, "periods": {}}
for days, label in LABELS.items():
    repro._resolved.clear()
    window = repro.make_window(days, now_ms, ZONE)
    summary = repro.summarize(doc, window, now_ms, table, overrides, aliases, state)
    m = repro.merge(summary, set())
    golden["periods"][label] = {
        "costUsd": m["costUsd"], "totalTokens": m["total"], "uncachedInput": m["unc"], "cachedInput": m["cached"],
        "cacheCreation": m["write"], "output": m["out"], "reasoning": m["reasoning"], "records": m["records"],
        "unpricedRecords": m["unpriced"], "sessions": m["sessions"], "cacheSavingsUsd": m["save"],
        "costByType": {**m["cat"], "other": m["unsplit"]},
        "costBySpeed": {"standard": m["standard"], "fast": m["fast"], "ultrafast": m["ultra"], "premium": m["premium"]},
        "providers": {name: v for name, v in m["providers"].items() if v["tokens"] > 0 or v["cost"] > 0},
        "models": {f"{provider}/{model}": {"tokens": v["tokens"], "cost": v["cost"], "records": v["records"], "unpriced": v["unpriced"]}
                   for (provider, model), v in m["models"].items()},
        "duplicatesDropped": summary["duplicatesDropped"],
        "series": series(summary, window),
    }
json.dump(golden, sys.stdout, indent=1, sort_keys=True)
print()
