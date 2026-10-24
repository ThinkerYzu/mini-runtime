#!/usr/bin/env python3
"""Check the tracked publication file allowlist without opening private outputs."""
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
manifest = ROOT / "tools/publication-files.txt"
approved = set(manifest.read_text().splitlines())
tracked = set(subprocess.check_output(
    ["git", "ls-files", "-z"], cwd=ROOT).decode().strip("\0").split("\0")) - {""}
unapproved = sorted(tracked - approved)
missing = sorted(path for path in approved if not (ROOT / path).is_file())
if unapproved or missing:
    for path in unapproved:
        print("Review and explicitly allowlist new tracked file:", path, file=sys.stderr)
    for path in missing:
        print("Missing approved source/fixture:", path, file=sys.stderr)
    raise SystemExit(1)
print(f"Publication allowlist passes: {len(tracked)} tracked files.")
