#!/usr/bin/env python3
"""Native Git entry point; baseline policy belongs to c9-preproduction-parity.py."""
import importlib.util
from pathlib import Path
import subprocess
import sys
import re


def main():
    updates = sys.stdin.buffer.read()
    common = Path(subprocess.check_output(['git','rev-parse','--git-common-dir'],text=True).strip()).resolve()
    previous = common/'hooks/pre-push.before-docsum-baseline'
    if previous.exists():
        result = subprocess.run([str(previous), *sys.argv[1:]], input=updates)
        if result.returncode: return result.returncode
    root = Path(subprocess.check_output(['git','rev-parse','--show-toplevel'],text=True).strip())
    owner = root/'scripts/c9-preproduction-parity.py'
    markers=('src-tauri/src/pipeline/summary/coherent/whole_document.rs',
             'src-tauri/src/pipeline/summary/comparisons/fixtures/whole-writer-baseline-manifest.json')
    rows=[line.split() for line in updates.decode().splitlines() if line.strip()]
    if any(len(row)!=4 or not re.fullmatch('[0-9a-f]{40}',row[1]) for row in rows):
        raise ValueError('invalid Git push update')
    adopted=[]
    for row in rows:
        if row[1]=='0'*40: continue
        # The proposed object is authoritative; current-directory files are not.
        entries=subprocess.check_output(['git','ls-tree',row[1],'--',*markers],text=True)
        if entries.strip(): adopted.append(' '.join(row))
    if not adopted: return 0
    manifest=root/markers[1]
    if not manifest.is_file() or not owner.is_file():
        raise ValueError('writer commit must be pushed from its tested checkout')
    spec=importlib.util.spec_from_file_location('docsum_baseline',owner)
    policy=importlib.util.module_from_spec(spec);spec.loader.exec_module(policy)
    policy.check_offline_push(('\n'.join(adopted)+'\n').encode(), common/'docsum-offline-baseline.json')
    return 0


if __name__ == '__main__':
    try: sys.exit(main())
    except Exception as error:
        print('DocSum push blocked: '+str(error),file=sys.stderr)
        sys.exit(1)
