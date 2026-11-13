#!/usr/bin/env python3
"""Report whether crates.io contains this version from this exact Git commit."""
import argparse
import io
import json
from pathlib import Path
import subprocess
import tarfile
import time
import tomllib
import urllib.error
import urllib.request

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--require-published', action='store_true')
args = parser.parse_args()
root = Path(__file__).resolve().parents[1]
package = tomllib.loads((root / 'Cargo.toml').read_text())['package']
name, version = package['name'], package['version']
commit = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=root, text=True).strip()
def fetch(url):
    request = urllib.request.Request(url, headers={'User-Agent': 'pscon-release (https://github.com/vyusec/pscon)'})
    return urllib.request.urlopen(request, timeout=30)
for attempt in range(12 if args.require_published else 1):
    try:
        with fetch(f'https://crates.io/api/v1/crates/{name}/{version}') as response:
            metadata = json.load(response)['version']
        break
    except urllib.error.HTTPError as error:
        if error.code != 404:
            raise
        if not args.require_published:
            print('published=false')
            raise SystemExit(0)
        if attempt == 11:
            raise SystemExit('Version is still absent from crates.io after publishing')
        time.sleep(5)
if metadata['yanked']:
    raise SystemExit('This version is yanked; create a new version instead')
with fetch(f'https://static.crates.io/crates/{name}/{name}-{version}.crate') as response:
    archive_bytes = response.read()
with tarfile.open(fileobj=io.BytesIO(archive_bytes), mode='r:gz') as archive:
    vcs = json.load(archive.extractfile(f'{name}-{version}/.cargo_vcs_info.json'))
if vcs['git']['sha1'] != commit:
    raise SystemExit('Version already belongs to another Git revision; bump the version instead')
print('published=true')
