#!/usr/bin/env python3
"""Verify one running process is an anchored Hyprland layer surface."""
import argparse
import json
import subprocess
import sys
from pathlib import Path


def read(subject):
    return json.loads(subprocess.check_output(['hyprctl', '-j', subject], text=True))


def verify(pid):
    monitors = {m['name']: m for m in read('monitors')}
    layers = read('layers')
    own = [(name, level, surface) for name, output in layers.items()
           for level, surfaces in output['levels'].items() for surface in surfaces
           if surface.get('pid') == pid]
    if len(own) != 1:
        raise ValueError(f'Expected one owned layer surface, found {len(own)}')
    name, level, surface = own[0]
    if any(c.get('pid') == pid for c in read('clients')):
        raise ValueError('Working panel still appears as an ordinary compositor client')
    monitor = monitors[name]
    width, height = monitor['width'], monitor['height']
    if monitor.get('transform', 0) % 2:
        width, height = height, width
    width, height = width / monitor['scale'], height / monitor['scale']
    left, top = monitor['x'], monitor['y']
    right = left + width
    if abs(surface['w'] - 320) > 1 or not 1 <= surface['h'] <= 600:
        raise ValueError(f'Unexpected panel dimensions: {surface}')
    gap_right = right - surface['x'] - surface['w']
    gap_top = surface['y'] - top - monitor['reserved'][1]
    if not 0 <= gap_right <= 32 or not 0 <= gap_top <= 48:
        raise ValueError(f'Panel is not below the bar near the upper-right edge: {surface}')
    if surface['y'] + surface['h'] > top + height or surface.get('alpha', 1) <= 0:
        raise ValueError('Panel is clipped or not visible')
    return {'pass': True, 'pid': pid, 'surface': surface, 'level': level,
            'monitor': {k: monitor[k] for k in ('name', 'width', 'height', 'scale', 'transform', 'x', 'y', 'reserved')}, 'right_gap': gap_right, 'gap_below_reserved_bar': gap_top,
            'scope': 'Read-only Hyprland geometry; does not prove pointer or keyboard behavior'}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--pid', type=int, required=True)
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    try:
        result = verify(args.pid)
    except (ValueError, KeyError, subprocess.SubprocessError) as error:
        result = {'pass': False, 'pid': args.pid, 'error': str(error)}
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps(result))
    return 0 if result['pass'] else 1


if __name__ == '__main__':
    sys.exit(main())
