#!/usr/bin/env python3
import argparse
import getpass
import os
from pathlib import Path
import subprocess
import tarfile
import tempfile


parser = argparse.ArgumentParser()
parser.add_argument('--private-home', action='append', default=[str(Path.home())])
parser.add_argument('--private-name', action='append', default=[getpass.getuser()])
parser.add_argument('artifacts', nargs='+', type=Path)
args = parser.parse_args()
patterns = [value.encode() for value in args.private_home + args.private_name if value]
files = 0


def scan(path, scratch):
    global files
    if path.is_dir():
        for child in sorted(path.iterdir()):
            if not child.is_symlink():
                scan(child, scratch)
        return
    output = subprocess.check_output(['strings', '-a', str(path)])
    if any(pattern in output or pattern in path.name.encode() for pattern in patterns):
        raise SystemExit(f'Private build identity found in artifact: {path.name}')
    files += 1
    if tarfile.is_tarfile(path) or path.name.endswith('.tar.zst'):
        listing = subprocess.check_output(["tar", "-tvf", str(path)])
        if any(pattern in listing for pattern in patterns):
            raise SystemExit(f"Private archive metadata found in artifact: {path.name}")
        extracted = Path(tempfile.mkdtemp(dir=scratch))
        subprocess.run(['tar', '-xf', str(path), '-C', str(extracted)], check=True)
        scan(extracted, scratch)
    elif path.suffix == '.dmg':
        mount = Path(tempfile.mkdtemp(dir=scratch))
        subprocess.run(['hdiutil', 'attach', '-readonly', '-nobrowse', '-mountpoint', str(mount), str(path)], check=True, stdout=subprocess.DEVNULL)
        try:
            scan(mount, scratch)
        finally:
            subprocess.run(['hdiutil', 'detach', str(mount)], check=True, stdout=subprocess.DEVNULL)


with tempfile.TemporaryDirectory(dir=os.environ.get('TMPDIR', '/var/tmp')) as scratch:
    for artifact in args.artifacts:
        scan(artifact.resolve(), scratch)
        print(f'Private-path check passed: {artifact.name}')
print(f'Scanned {files} files, including nested archives.')
