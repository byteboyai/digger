#!/usr/bin/env python3
"""Regenerate tracked, entirely synthetic Claude transcripts; no machine input."""
import json
from pathlib import Path

OUT = Path(__file__).resolve().parents[1] / 'crates/desktop/tests/fixtures/conversations'
CWD = '__SYNTHETIC_PROJECT__'

def record(name, index, role, content):
    if role == 'assistant' and isinstance(content, str):
        content = [{'type': 'text', 'text': content}]
    return {'type': role, 'uuid': f'synthetic-{name}-{index:06}', 'sessionId': f'synthetic-{name}',
            'timestamp': '2026-10-18T12:00:00.000Z', 'cwd': CWD,
            'message': {'role': role, 'content': content, **({'usage': {'input_tokens': 17, 'output_tokens': 11, 'cache_read_input_tokens': 5, 'cache_creation_input_tokens': 3}} if role == 'assistant' else {})}}

def rounds(name, count):
    for i in range(count):
        yield record(name, i*2, 'user', f'Synthetic request {i+1}: inspect fixture {name}.')
        yield record(name, i*2+1, 'assistant', [{'type': 'text', 'text': f'## Round {i+1}\nSynthetic answer.\n\n- Checked the sample\n- No real user data'}])

def write(name, records):
    with (OUT / f'{name}.jsonl').open('w') as f:
        for item in records:
            f.write((item if isinstance(item, str) else json.dumps(item, ensure_ascii=False, separators=(',', ':'))) + '\n')

write('short', rounds('short', 3))
write('long', rounds('long', 500))
write('huge', rounds('huge', 5000))
write('tools', [record('tools', 0, 'user', 'Synthetic tool exercise.'),
    record('tools', 1, 'assistant', [{'type': 'thinking', 'thinking': 'Synthetic reasoning: inspect both files.'},
        {'type': 'text', 'text': 'Reading synthetic files.'},
        {'type': 'tool_use', 'id': 'synthetic-read-a', 'name': 'Read', 'input': {'file_path': '/synthetic/a.md'}},
        {'type': 'tool_use', 'id': 'synthetic-read-b', 'name': 'Read', 'input': {'file_path': '/synthetic/b.md'}}]),
    record('tools', 2, 'user', [{'type': 'tool_result', 'tool_use_id': 'synthetic-read-a', 'content': 'Synthetic file A'}]),
    record('tools', 3, 'user', [{'type': 'tool_result', 'tool_use_id': 'synthetic-read-b', 'content': 'Synthetic failure B', 'is_error': True}]),
    record('tools', 4, 'assistant', 'Synthetic tools complete.')])
write('hostile', [record('hostile', 0, 'user', '<script>window.fixture=1</script><img src=x onerror=alert(1)>\n[unsafe](javascript:alert(1))\n[html](data:text/html,synthetic)\n\u200bzero\u202ertl\u202c\n```unclosed\n' + 'x'*100_000),
    record('hostile', 1, 'assistant', 'Synthetic multi-megabyte single turn:\n' + 'large fixture content\n'*160_000)])
write('empty', [{'type': 'system', 'cwd': CWD, 'sessionId': 'synthetic-empty', 'timestamp': '2026-10-18T12:00:00.000Z', 'content': 'Synthetic system metadata only'}])
write('broken', ['{synthetic malformed JSON', *rounds('broken', 1), '{"unfinished":'])
