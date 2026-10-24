"""Opt-in policy for checks consuming locally recorded benchmark data."""
import glob
import os
from pathlib import Path
import sys


def require_recordings(*paths, record):
    """Stop before data access unless opted in; missing opted-in inputs are errors.

    Paths may be files, directories or glob patterns. This never records data,
    opens a browser or downloads an input automatically.
    """
    name = Path(sys.argv[0]).name
    if os.environ.get('MRT_RECORDED_TESTS') != '1':
        print(f'SKIP {name}: recorded-data checks are disabled. '
              'Record your own inputs, then set MRT_RECORDED_TESTS=1. '
              'See docs/measurements.md.')
        raise SystemExit(0)
    missing = [str(path) for path in paths if not glob.glob(str(path))]
    if missing:
        print(f'{name}: missing recorded input(s):\n  ' + '\n  '.join(missing)
              + f'\nRecord first: {record}\nSee docs/measurements.md.', file=sys.stderr)
        raise SystemExit(2)
