#!/usr/bin/env python3 -I
import datetime as dt
import importlib.util
import json
import os
import sys

repro_path, state, now_iso, zone = sys.argv[1:5]
LABELS = {1: "Past 24h", 7: "7 days", 30: "30 days", 90: "90 days"}

spec = importlib.util.spec_from_file_location("repro", repro_path)
repro = importlib.util.module_from_spec(spec)
spec.loader.exec_module(repro)

now_ms = int(dt.datetime.fromisoformat(now_iso.replace("Z", "+00:00")).timestamp() * 1000)
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


def money(value):
    return f"{value:.6f}"


for days, label in LABELS.items():
    repro._resolved.clear()
    window = repro.make_window(days, now_ms, zone)
    summary = repro.summarize(doc, window, now_ms, table, overrides, aliases, state)
    m = repro.merge(summary, set())
    print(f"PERIOD {label}")
    print(f"tokens processed={m['total']} uncached={m['unc']} cached={m['cached']} creation={m['write']} output={m['out']} reasoning={m['reasoning']}")
    cat = m["cat"]
    print(f"cost total={money(m['costUsd'])} input={money(cat['input'])} cacheRead={money(cat['cacheRead'])} "
          f"cacheWrite={money(cat['cacheWrite'])} output={money(cat['output'])} other={money(m['unsplit'])}")
    print(f"speed standard={money(m['standard'])} fast={money(m['fast'])} ultrafast={money(m['ultra'])} premium={money(m['premium'])}")
    print(f"savings {money(m['save'])}")
    print(f"sessions {m['sessions']} records {m['records']} unpriced {m['unpriced']}")
    order = list(repro.PROVIDER_ORDER)
    names = sorted((p for p, v in m["providers"].items() if v["tokens"] > 0 or v["cost"] > 0),
                   key=lambda p: (order.index(p) if p in order else len(order), p))
    for name in names:
        v = m["providers"][name]
        print(f"provider {name} tokens={v['tokens']} cost={money(v['cost'])} sessions={v['sessions']}")
