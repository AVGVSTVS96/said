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
    assert result['hallucinated_words'] == 0, engine
    assert result['latency']['max_replay_lag_ms'] < 1000, engine
    assert result['peak_rss_bytes'] is not None and result['peak_rss_bytes'] > 0, engine
records = [json.loads(line) for line in Path(sys.argv[1]).with_name('clips.jsonl').read_text().splitlines()]
for engine in summary['engines']:
    for subset, ceiling, expected_clips in [('synthetic', 0.30, 8), ('AMI', 0.40, 2)]:
        chosen = [r for r in records if r['engine'] == engine and r['status'] == 'ok'
                  and r['reference'].strip() and ('natural' in r['tags']) == (subset == 'AMI')]
        assert len(chosen) == expected_clips, (engine, subset, 'missing speech clips')
        assert all(r['recognition']['text'].strip() for r in chosen), (engine, subset, 'empty speech recognition')
        counts = [r['score']['normalized'] for r in chosen]
        words = sum(c['reference_words'] for c in counts)
        errors = sum(c['substitutions'] + c['deletions'] + c['insertions'] for c in counts)
        wer = errors / words
        assert wer <= ceiling, f"{engine}: {subset} normalized WER {wer:.1%} exceeds {ceiling:.0%}"
for record in records:
    if record.get('id') in {'dont-delete', 'do-delete', 'never-rewrite', 'always-rewrite'}:
        assert record['status'] == 'ok'
        counts = record['score']
        assert counts['negations_dropped'] == counts['negations_added'] == counts['negations_changed'] == 0
print('Public CI benchmark counts pass.')
