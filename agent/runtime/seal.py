import hashlib
import json
from pathlib import Path

root = Path(__file__).resolve().parent
files = []
for p in sorted(root.rglob('*')):
    if p.is_symlink():
        continue
    if p.is_file() and p.name not in ('runtime.json',):
        h = hashlib.sha256()
        with p.open('rb') as stream:
            for block in iter(lambda: stream.read(1024 * 1024), b''):
                h.update(block)
        files.append([p.relative_to(root).as_posix(), h.hexdigest()])
encoded = json.dumps(files, separators=(',', ':')).encode()
(root / 'runtime.json').write_bytes(encoded)
print(json.dumps({'sha256': hashlib.sha256(encoded).hexdigest(), 'files': len(files)}))
