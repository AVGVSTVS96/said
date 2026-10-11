#!/usr/bin/env python3
import argparse
import hashlib
import json
import shutil
import struct
import subprocess
import sys
import urllib.request
import wave
import zipfile
from pathlib import Path
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parents[1]


def digest(path):
    with path.open('rb') as source:
        return hashlib.file_digest(source, 'sha256').hexdigest()


def download(asset, cache):
    path = cache / asset['name']
    if not path.exists() or digest(path) != asset['sha256']:
        temporary = path.with_suffix(path.suffix + '.pending')
        with urllib.request.urlopen(asset['url'], timeout=120) as source, temporary.open('wb') as out:
            shutil.copyfileobj(source, out)
        if digest(temporary) != asset['sha256']:
            temporary.unlink()
            raise RuntimeError(f"source hash mismatch: {asset['name']}")
        temporary.replace(path)
    return path


def write_wave(path, samples):
    with wave.open(str(path), 'wb') as out:
        out.setnchannels(1)
        out.setsampwidth(2)
        out.setframerate(16000)
        out.writeframes(samples)


def resample(source, target):
    with wave.open(str(source), 'rb') as wav:
        if wav.getnchannels() != 1 or wav.getsampwidth() != 2:
            raise RuntimeError('synthesis requires mono i16')
        rate = wav.getframerate()
        data = wav.readframes(wav.getnframes())
    samples = [p[0] for p in struct.iter_unpack('<h', data)]
    converted = bytearray()
    for i in range(len(samples) * 16000 // rate):
        position, fraction = divmod(i * rate, 16000)
        a, b = samples[position], samples[min(position + 1, len(samples) - 1)]
        converted.extend(struct.pack('<h', (a * (16000 - fraction) + b * fraction) // 16000))
    write_wave(target, converted)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--output', type=Path, default=Path.home() / '.cache/said-models/bench-public')
    parser.add_argument('--ci', action='store_true')
    parser.add_argument('--receipts', type=Path)
    args = parser.parse_args()
    rows = [json.loads(line) for line in (ROOT / 'recipes.jsonl').read_text().splitlines()]
    expected = {row['id']: row for row in map(json.loads, (ROOT / 'clips.jsonl').read_text().splitlines())} if (ROOT / 'clips.jsonl').exists() else {}
    assets = json.loads((ROOT / 'sources.json').read_text())
    cache = Path.home() / '.cache/said-models/bench-source'
    cache.mkdir(parents=True, exist_ok=True)
    (args.output / 'clips').mkdir(parents=True, exist_ok=True)
    receipts = []
    for row in rows:
        if args.ci and not row['ci']:
            continue
        path = args.output / 'clips' / (row['id'] + '.wav')
        bundled = ROOT / 'clips' / path.name
        pin = expected.get(row['id'])
        if bundled.exists():
            shutil.copyfile(bundled, path)
        elif pin and path.exists() and digest(path) == pin['sha256']:
            pass
        elif row['generator'] == 'ami':
            audio = download(assets['ami_audio'][row['speaker']], cache)
            annotations = download(assets['ami_annotations'], cache)
            with zipfile.ZipFile(annotations) as archive:
                words = ET.fromstring(archive.read(f"words/ES2002a.{row['speaker']}.words.xml"))
            selected = [w for w in words if w.text and float(w.get('starttime', '-1')) >= row['start'] and float(w.get('endtime', '999999')) <= row['end']]
            reference = ''
            for word in selected:
                if word.get('punc') == 'true' and not reference:
                    continue
                reference += ('' if word.get('punc') == 'true' or not reference else ' ') + word.text
            if reference != row['reference']:
                raise RuntimeError(f"annotation mismatch: {row['id']}: {reference!r}")
            with wave.open(str(audio)) as wav:
                if wav.getframerate() != 16000 or wav.getsampwidth() != 2 or wav.getnchannels() != 1:
                    raise RuntimeError('AMI source format changed')
                start = round(row['start'] * 16000)
                end = round(row['end'] * 16000)
                wav.setpos(start)
                data = wav.readframes(end - start)
            write_wave(path, data)
        elif row['generator'] == 'control':
            state = 20261010
            samples = bytearray()
            for _ in range(round(row['seconds'] * 16000)):
                if row['id'] == 'silence':
                    value = 0
                else:
                    state = (1664525 * state + 1013904223) % 2**32
                    value = ((state >> 16) - 32768) // 8
                samples.extend(struct.pack('<h', value))
            write_wave(path, samples)
        else:
            model = download(assets['tts_model'], cache)
            download(assets['tts_config'], cache)
            python = cache.parent / 'bench-tools/piper/bin/python'
            if not python.exists():
                subprocess.run(['uv', 'venv', str(python.parent.parent), '--python', '3.12'], check=True)
                subprocess.run(['uv', 'pip', 'install', '--python', str(python), '-r', str(ROOT / 'scripts/tts-requirements.txt')], check=True)
            temporary = path.with_suffix('.native.wav')
            subprocess.run([str(python), '-m', 'piper', '--model', str(model), '--output-file', str(temporary),
                            '--noise-scale', '0', '--noise-w-scale', '0', '--length-scale', '1'],
                           input=row['spoken'], text=True, check=True)
            resample(temporary, path)
            temporary.unlink()
        sha = digest(path)
        if pin and sha != pin['sha256']:
            raise RuntimeError(f"clip hash mismatch: {row['id']}; do not benchmark different audio under this manifest")
        receipt = {k: row[k] for k in ('id', 'reference', 'tags', 'terms', 'ci', 'license', 'source', 'reference_status')}
        receipt.update(audio=f"clips/{row['id']}.wav", sha256=sha)
        for key in ('pair', 'polarity'):
            if key in row:
                receipt[key] = row[key]
        receipts.append(receipt)
        print(row['id'], sha, file=sys.stderr)
    if args.receipts:
        args.receipts.write_text(''.join(json.dumps(row, ensure_ascii=False) + '\n' for row in receipts))


if __name__ == '__main__':
    main()
