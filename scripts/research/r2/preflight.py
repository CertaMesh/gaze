#!/usr/bin/env python3
"""Refuse standalone dependency drift before a source-baseline comparison."""
import hashlib
import json
from pathlib import Path
import subprocess
import tomllib
from experiment import HERE, ROOT

root_path, probe_path = ROOT / "Cargo.lock", HERE / "probe/Cargo.lock"
root, probe = (tomllib.loads(p.read_text())["package"] for p in (root_path, probe_path))
identity = lambda p: (p["name"], p["version"], p.get("source"), p.get("checksum"))
allowed = {identity(p) for p in root}
drift = [identity(p) for p in probe if p.get("source") and identity(p) not in allowed]
if drift:
    raise SystemExit("Probe registry dependencies differ from root lock: " + str(drift))
changed = subprocess.check_output(
    ["git", "diff", "--name-only", "034cd7447c9a736fccb6a9c2b165fc48fd38dd98", "--", "crates", "Cargo.lock"],
    cwd=ROOT, text=True,
).strip()
if changed:
    raise SystemExit("Production baseline changed: " + changed)
print(json.dumps(dict(root_lock_sha256=hashlib.sha256(root_path.read_bytes()).hexdigest(),
                      probe_lock_sha256=hashlib.sha256(probe_path.read_bytes()).hexdigest(),
                      registry_packages_checked=sum(bool(p.get("source")) for p in probe),
                      drift_count=len(drift), production_diff=[]), indent=2))
