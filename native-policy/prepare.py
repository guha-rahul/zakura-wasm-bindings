#!/usr/bin/env python3
"""Materialize the owned wallet policy patches from verified, read-only Cargo inputs."""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tarfile

ROOT = Path(__file__).resolve().parent

def prepare(cargo_home):
    vendor = ROOT / 'vendor'
    if vendor.exists():
        raise RuntimeError('vendor already exists; preserve it for inspection')
    vendor.mkdir()
    receipt = {}
    for name, package in json.loads((ROOT / 'upstream.json').read_text()).items():
        stem = name + '-' + package['version']
        matches = list((Path(cargo_home) / 'registry/src').glob('*/' + stem))
        if len(matches) != 1:
            raise RuntimeError('expected one cached source: ' + stem)
        source = matches[0]
        archive = Path(cargo_home) / 'registry/cache' / source.parent.name / (stem + '.crate')
        digest = lambda p: hashlib.sha256(p.read_bytes()).hexdigest()
        if digest(archive) != package['checksum']:
            raise RuntimeError('archive checksum mismatch: ' + stem)
        with tarfile.open(archive) as archive_file:
            expected = {str(Path(m.name).relative_to(stem)):
                hashlib.sha256(archive_file.extractfile(m).read()).hexdigest()
                for m in archive_file.getmembers() if m.isfile()}
        actual = {str(p.relative_to(source)): digest(p) for p in source.rglob('*')
                  if p.is_file() and p.name not in ['.cargo-ok', '.cargo-checksum.json']}
        if actual != expected:
            raise RuntimeError('source differs from locked archive: ' + stem)
        destination = vendor / name
        shutil.copytree(source, destination)
        patch = ROOT / (name + '.patch')
        subprocess.run(['git', 'apply', '--check', str(patch)], cwd=destination, check=True)
        subprocess.run(['git', 'apply', str(patch)], cwd=destination, check=True)
        receipt[name] = dict(upstream=package['checksum'], patch=digest(patch),
            files={str(p.relative_to(destination)): digest(p)
                   for p in sorted(destination.rglob('*')) if p.is_file()})
    (vendor / 'receipt.json').write_text(json.dumps(receipt, indent=2) + '\n')

if __name__ == '__main__':
    prepare(os.environ.get('CARGO_HOME', '/home/jack/.cargo'))
