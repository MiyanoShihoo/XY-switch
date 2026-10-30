import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { resolve } from 'node:path';
const [archive, target] = process.argv.slice(2);
assert(archive && target);
execFileSync('python', ['-c', `
import pathlib, stat, sys, zipfile
archive, root = pathlib.Path(sys.argv[1]), pathlib.Path(sys.argv[2]).resolve()
assert not root.exists(), 'Artifact extraction target must be fresh'
with zipfile.ZipFile(archive) as z:
    entries = z.infolist()
    assert len(entries) < 10000 and sum(e.file_size for e in entries) < 1024**3
    seen = set()
    for e in entries:
        assert '\\\\' not in e.filename and ':' not in e.filename, 'Unsafe archive path'
        dest = (root / e.filename).resolve()
        assert dest.is_relative_to(root) and dest != root, 'Artifact path escapes target'
        assert not stat.S_ISLNK(e.external_attr >> 16), 'Artifact links are not accepted'
        assert str(dest).lower() not in seen, 'Duplicate artifact path'
        seen.add(str(dest).lower())
    root.mkdir(parents=True)
    z.extractall(root)
print('Verified artifact ZIP extracted')
`, resolve(archive), resolve(target)], { stdio: 'inherit' });
