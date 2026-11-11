#!/usr/bin/env python3
"""Read one process's real AT-SPI tree without inventing Cua window handles."""
import argparse
import json
import sys
import gi

gi.require_version('Atspi', '2.0')
from gi.repository import Atspi


def snapshot(pid):
    desktop = Atspi.get_desktop(0)
    matches = []
    for index in range(desktop.get_child_count()):
        app = desktop.get_child_at_index(index)
        if app and app.get_process_id() == pid:
            matches.append(app)
    if len(matches) != 1:
        raise RuntimeError(f'Expected one AT-SPI application for PID {pid}, found {len(matches)}')
    app = matches[0]
    roots = [app.get_child_at_index(index) for index in range(app.get_child_count())]
    roots = [root for root in roots if root and root.get_state_set().contains(Atspi.StateType.SHOWING)]
    if not roots:
        raise RuntimeError('Owned AT-SPI application has no showing surface')
    elements = []
    def walk(node, depth):
        if depth > 24 or len(elements) >= 200:
            raise RuntimeError('AT-SPI tree exceeded the bounded read budget')
        states = node.get_state_set()
        elements.append({'role': node.get_role_name(), 'name': node.get_name(), 'depth': depth,
                         'enabled': states.contains(Atspi.StateType.ENABLED),
                         'showing': states.contains(Atspi.StateType.SHOWING)})
        for index in range(node.get_child_count()):
            child = node.get_child_at_index(index)
            if child:
                walk(child, depth + 1)
    for root in roots:
        walk(root, 0)
    return {'pid': pid, 'source': 'Native AT-SPI read; no Cua element tokens or window screenshot',
            'elements_complete': True, 'elements': elements,
            'tree_markdown': '\n'.join('  ' * item['depth'] + '- ' + item['role'] + ' ' + json.dumps(item['name']) for item in elements)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--pid', required=True, type=int)
    args = parser.parse_args()
    try:
        print(json.dumps(snapshot(args.pid)))
    except Exception as error:
        print(str(error), file=sys.stderr)
        return 1
    return 0


if __name__ == '__main__':
    sys.exit(main())
