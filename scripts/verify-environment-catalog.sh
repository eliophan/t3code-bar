#!/usr/bin/env bash
set -euo pipefail

catalog=${1:-${T3_CODE_BAR_STATE_DIR:-$HOME/.t3/userdata}/bar-environments.json}
if [[ ! -f "$catalog" ]]; then
  echo "catalog missing: $catalog" >&2
  exit 2
fi

python3 - "$catalog" <<'PY'
import json
import sys
from pathlib import Path

path = Path(sys.argv[1])
records = json.loads(path.read_text())
if not isinstance(records, list):
    raise SystemExit("catalog must be an array")
ids = [r.get("environmentId") for r in records]
if any(not isinstance(value, str) or not value.strip() for value in ids):
    raise SystemExit("every record needs a non-empty environmentId")
if len(ids) != len(set(ids)):
    raise SystemExit("environmentId values must be unique")
for record in records:
    for key in ("label", "machine", "endpoint"):
        if not isinstance(record.get(key), str) or not record[key].strip():
            raise SystemExit(f"record {record['environmentId']} has no {key}")
labels = {}
for record in records:
    labels.setdefault(record["label"], []).append(record["environmentId"])
print(f"catalog={path}")
print(f"records={len(records)}")
for label, environment_ids in labels.items():
    print(f"label={label!r} ids={','.join(environment_ids)}")
if any(len(environment_ids) > 1 for environment_ids in labels.values()):
    print("duplicate-labels=preserved-by-environmentId")
PY
