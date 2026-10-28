#!/usr/bin/env python3
"""Compare optimized, in-process Swift/Rust parsers on the local sample corpus.

Build the Swift backend first: bash scripts/build_backend.sh
Then run: python3 scripts/benchmark_parsers.py
No samples or generated binaries are added to git.
"""
import argparse
import hashlib
import json
from pathlib import Path
import platform
import random
import statistics
import subprocess
import time

ROOT = Path(__file__).resolve().parent.parent


def command(args, **kwargs):
    return subprocess.run([str(a) for a in args], cwd=ROOT, check=True, **kwargs)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--samples', type=Path, default=ROOT / 'pictures')
    parser.add_argument('--output', type=Path, default=ROOT / '.build/parser-benchmark')
    parser.add_argument('--skip-build', action='store_true')
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    # Refuse a stale Swift library instead of comparing different implementations.
    for name, digest in json.loads((ROOT / '.build/backend/source-hashes.json').read_text()).items():
        if name == 'tests/NativeDriver.swift':
            continue
        assert hashlib.sha256((ROOT / name).read_bytes()).hexdigest() == digest, f'Rebuild backend: {name}'
    swift = args.output.resolve() / 'swift-bench'
    rust = ROOT / 'core/target/release/examples/bench_parse'
    if not args.skip_build:
        command(['cargo', 'build', '--release', '--locked', '--manifest-path', 'core/Cargo.toml', '--example', 'bench_parse'])
        command(['xcrun', 'swiftc', '-parse-as-library', '-O', '-target', 'x86_64-apple-macosx12.0',
                 '-module-cache-path', '/private/tmp/motionlive-backend-swift-cache',
                 '-I', '.build/backend', '-L', '.build/backend', '-lMomentStaysBackend',
                 '-Xlinker', '-rpath', '-Xlinker', str(ROOT / '.build/backend'),
                 'tests/ParseBenchmark.swift', '-o', swift])
    groups = ['Google', 'huawei', 'honor', 'xiaomi', 'Samsung']
    paths = sorted(p for group in groups for p in (args.samples / group).rglob('*')
                   if p.is_file() and p.suffix.lower() in ['.jpg', '.jpeg', '.heic', '.heif'])
    assert paths, 'No sample files found'
    rng = random.Random(20261028)
    rng.shuffle(paths)
    rows = []
    for index, path in enumerate(paths):
        engines = [('swift', [swift, path]), ('rust', [rust, path]), ('rust_owned', [rust, path, 'owned'])]
        rng.shuffle(engines)
        row = {'path': str(path.relative_to(args.samples)), 'bytes': path.stat().st_size,
               'sha256': hashlib.sha256(path.read_bytes()).hexdigest()}
        for engine, cmd in engines:
            result = command(cmd, text=True, capture_output=True, timeout=180)
            row[engine] = json.loads(result.stdout)
        summaries = [row[k].get('summary') for k in ['swift', 'rust', 'rust_owned']]
        row['accepted'] = all(s is not None for s in summaries)
        row['parity'] = summaries[0] == summaries[1] == summaries[2] if row['accepted'] else (
            all('error' in row[k] for k in ['swift', 'rust', 'rust_owned']) and
            len({row[k]['error'] for k in ['swift', 'rust', 'rust_owned']}) == 1)
        rows.append(row)
        # Persist after each input so an interrupted run still has reviewable data.
        (args.output / 'raw.json').write_text(json.dumps(rows, indent=2))
        if (index + 1) % 10 == 0 or index + 1 == len(paths):
            print(f'{index + 1}/{len(paths)} checked', flush=True)
    aggregates = []
    for group in groups + ['ALL']:
        selected = [r for r in rows if r['accepted'] and r['parity'] and
                    (group == 'ALL' or Path(r['path']).parts[0] == group)]
        if not selected:
            continue
        values = {k: statistics.mean(statistics.median(r[k]['ms']) for r in selected)
                  for k in ['swift', 'rust', 'rust_owned']}
        aggregates.append({'group': group, 'count': len(selected), 'mib': sum(r['bytes'] for r in selected) / 2**20,
                           **values, 'speedup': values['swift'] / values['rust'],
                           'owned_speedup': values['swift'] / values['rust_owned']})
    result = {'created':time.strftime('%Y-%m-%d %H:%M:%S %z'), 'platform':platform.platform(),
              'machine':platform.machine(), 'swift_version':command(['xcrun','swiftc','--version'],capture_output=True,text=True).stdout.strip(),
              'rust_version':command(['rustc','--version'],capture_output=True,text=True).stdout.strip(),
              'files':len(rows),'accepted':sum(r['accepted'] for r in rows),
              'mismatches':[r['path'] for r in rows if not r['parity']],
              'rejected':[{'path':r['path'],'swift':r['swift'].get('error'),'rust':r['rust'].get('error')} for r in rows if not r['accepted']],
              'aggregates':aggregates}
    (args.output / 'summary.json').write_text(json.dumps(result, indent=2))
    lines = ['# Swift / Rust parser benchmark', '',
             f"Run: {result['created']} · {result['platform']} · {result['machine']}", '',
             f"Inputs: {len(rows)}; accepted by both: {result['accepted']}; mismatches: {len(result['mismatches'])}.", '',
             'Optimized builds: Swift -O (backend and driver), Rust --release with LTO. Sequential single-process parsing of one in-memory file at a time.',
             'Per engine/file: two warmups, one calibration, nine measured batches targeting 10 ms each (1–100 parses/batch).',
             'Each file contributes its median batch latency; group figures are the arithmetic mean of those per-file medians.',
             'Excludes process startup, file reads, input array preparation, SHA-256 checks and JSON output. Includes parser allocations and result destruction.',
             'Swift uses an autorelease pool per parse. Rust native returns a video range; Rust owned additionally copies video bytes to match Swift output ownership more closely.',
             'Engine and file order are shuffled with a fixed seed. These are warm in-memory parser timings, not full conversion or disk-throughput measurements.', '',
             '| Samples | Files | MiB | Swift ms/file | Rust ms/file | Rust owned ms/file | Swift/Rust | Swift/Rust owned |',
             '|---|---:|---:|---:|---:|---:|---:|---:|']
    for a in aggregates:
        lines.append(f"| {a['group']} | {a['count']} | {a['mib']:.1f} | {a['swift']:.3f} | {a['rust']:.3f} | {a['rust_owned']:.3f} | {a['speedup']:.2f}× | {a['owned_speedup']:.2f}× |")
    lines += ['', 'Correctness checks compare format, frame, timestamp, timestamp tag, Ultra HDR flag, still SHA-256 and video SHA-256 before including a file in aggregates.', '',
              'Source versions are recorded in source-hashes.json; input hashes and all nine timings are in raw.json.', '',
              'Rejected files:', '```json', json.dumps(result['rejected'],indent=2), '```', '',
              'Mismatches:', '```json', json.dumps(result['mismatches'],indent=2), '```', '']
    (args.output / 'report.md').write_text('\n'.join(lines))
    sources = ['tests/ParseBenchmark.swift','core/examples/bench_parse.rs','scripts/benchmark_parsers.py']
    sources += [str(p.relative_to(ROOT)) for p in (ROOT / 'core/src').rglob('*.rs')]
    hashes = json.loads((ROOT / '.build/backend/source-hashes.json').read_text())
    hashes.update({name:hashlib.sha256((ROOT / name).read_bytes()).hexdigest() for name in sources})
    (args.output / 'source-hashes.json').write_text(json.dumps(hashes,indent=2))
    print(json.dumps(result, indent=2), flush=True)


if __name__ == '__main__':
    main()
