#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DEST="${1:-$HOME/.config/workmux/agents/pi}"
python3 - "$ROOT/pi" "$DEST" <<'PY'
import json, pathlib, shutil, sys, time
source, dest = map(lambda p: pathlib.Path(p).expanduser().resolve(), sys.argv[1:])
if dest == source or source in dest.parents or dest in source.parents:
    raise SystemExit('Source and deployment directories must be separate')
if dest == (pathlib.Path.home() / '.pi/agent').resolve():
    raise SystemExit('Refusing to overwrite the main host Pi profile')
json.loads((source / 'settings.json').read_text())
dest.mkdir(parents=True, exist_ok=True, mode=0o700)
# Copy only managed profile files, never runtime state; no deletion of live files.
for relative in ['settings.json', 'extensions', 'skills', 'prompts']:
    entry = source / relative
    files = [entry] if entry.is_file() else entry.rglob('*')
    for file in files:
        if not file.is_file() or file.name == '.gitkeep':
            continue
        target = dest / file.relative_to(source)
        target.parent.mkdir(parents=True, exist_ok=True)
        if target.exists() and target.read_bytes() != file.read_bytes():
            backup = dest / '.config-backups' / str(time.time_ns()) / file.relative_to(source)
            backup.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(target, backup)
        shutil.copy2(file, target)
print(f'Installed Linux Pi profile into {dest}')
PY
