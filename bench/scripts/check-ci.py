#!/usr/bin/env python3
import json
import sys
from pathlib import Path

summary = json.loads(Path(sys.argv[1]).read_text())
assert summary['ci'] and summary['mode'] == 'real_time'
assert summary['clips'] == 12
for engine, result in summary['engines'].items():
    assert result['successful_clips'] == 12 and result['failed_clips'] == 0, engine
    assert result['normalized']['reference_words'] >= 100, engine
    assert result['negation']['opportunities'] >= 6, engine
    assert result['negations_dropped'] <= 3 and result['negations_added'] == 0 and result['negations_changed'] <= 1, engine
    assert result['filler']['opportunities'] >= 4 and result['filler']['matched'] >= 1, engine
    assert result['repetition']['opportunities'] >= 3, engine
    assert result['normalized_wer'] <= 0.35, f"{engine}: normalized WER {result['normalized_wer']:.1%} exceeds 35%"
    assert result['hallucinated_words'] == 0, engine
    assert result['latency']['max_replay_lag_ms'] < 1000, engine
    assert result['peak_rss_bytes'] is not None and result['peak_rss_bytes'] > 0, engine
records = [json.loads(line) for line in Path(sys.argv[1]).with_name('clips.jsonl').read_text().splitlines()]
for record in records:
    if record.get('id') in {'dont-delete', 'do-delete', 'never-rewrite', 'always-rewrite'}:
        assert record['status'] == 'ok'
        counts = record['score']
        assert counts['negations_dropped'] == counts['negations_added'] == counts['negations_changed'] == 0
print('Public CI benchmark counts pass.')
