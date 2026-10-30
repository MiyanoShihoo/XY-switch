import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { resolve } from 'node:path';
const [archive, target] = process.argv.slice(2);
assert(archive && target);
execFileSync('python', ['-c', String.raw`
import pathlib, stat, sys, zipfile
archive, root = pathlib.Path(sys.argv[1]), pathlib.Path(sys.argv[2]).resolve()
assert not root.exists(), 'Artifact extraction target must be fresh'
reserved = {'con', 'prn', 'aux', 'nul'} | {p + n for p in ('com', 'lpt') for n in '123456789¹²³'}
with zipfile.ZipFile(archive) as z:
    entries = z.infolist()
    assert len(entries) < 10000 and sum(e.file_size for e in entries) < 1024**3
    seen, files = set(), set()
    for e in entries:
        # ZipInfo normalizes backslashes on Windows and truncates NULs; inspect the original name.
        name = e.orig_filename
        assert name and not name.startswith('/') and not any(c in name for c in '<>:"\\|?*'), 'Unsafe archive path'
        assert not any(ord(c) < 32 for c in name), 'Unsafe archive control character'
        parts = (name[:-1] if name.endswith('/') else name).split('/')
        assert all(p not in ('', '.', '..') and not p.endswith((' ', '.')) for p in parts), 'Unsafe archive path component'
        assert all(p.split('.')[0].casefold() not in reserved for p in parts), 'Windows device archive path'
        dest = (root / name).resolve()
        assert dest.is_relative_to(root) and dest != root, 'Artifact path escapes target'
        mode = stat.S_IFMT(e.external_attr >> 16)
        assert mode in (0, stat.S_IFREG, stat.S_IFDIR), 'Artifact links and special files are not accepted'
        key = '/'.join(parts).casefold()
        assert key not in seen, 'Duplicate artifact path'
        seen.add(key)
        if not e.is_dir():
            files.add(key)
    for key in seen:
        assert all('/'.join(key.split('/')[:i]) not in files for i in range(1, len(key.split('/')))), 'Artifact file/directory collision'
    root.mkdir(parents=True)
    z.extractall(root)
print('Verified artifact ZIP extracted')
`, resolve(archive), resolve(target)], { stdio: 'inherit' });
