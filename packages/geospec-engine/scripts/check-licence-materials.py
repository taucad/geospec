#!/usr/bin/env python3
"""Compare a freshly generated licence tree with the committed one.

generate-delivery-materials.py writes two kinds of material. The inventory
(declared licences, licence and notice texts, THIRD-PARTY-NOTICES.txt and the
component records in components.json) depends only on the locked dependency
closure and the sources. The build binding (SOURCE-RELINK.json/.md and the
components.json fields sourceRelink and currentMixedWasm) names the
source-relink kit and mixed build of the generating run, so it differs on
every run. This check compares the inventory and reports the binding
separately; it never edits either tree.

Usage: check-licence-materials.py GENERATED COMMITTED
Exit: 0 inventory equal; 1 inventory differs; 2 bad arguments.
"""

import hashlib
import json
from pathlib import Path
import sys


BINDING_FILES = frozenset({'SOURCE-RELINK.json', 'SOURCE-RELINK.md'})
BINDING_FIELDS = ('sourceRelink', 'currentMixedWasm')
COMPONENTS = 'components.json'


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def inventory(root):
    files = {
        path.relative_to(root).as_posix(): digest(path)
        for path in sorted(root.rglob('*'))
        if path.is_file()
    }
    components = json.loads((root / COMPONENTS).read_text())
    binding = {field: components.pop(field, None) for field in BINDING_FIELDS}
    components['materialFiles'] = [
        row for row in components['materialFiles'] if row['path'] not in BINDING_FILES
    ]
    texts = {path: sha for path, sha in files.items()
             if path not in BINDING_FILES and path != COMPONENTS}
    return texts, components, binding


def crate_keys(components):
    return {f"{row['cohort']}:{row['name']}@{row['version']}" for row in components['cargoComponents']}


def compare(generated, committed):
    new_texts, new_components, new_binding = inventory(generated)
    old_texts, old_components, old_binding = inventory(committed)
    problems = []
    for path in sorted(new_texts.keys() - old_texts.keys()):
        problems.append(f'missing from the committed tree: {path}')
    for path in sorted(old_texts.keys() - new_texts.keys()):
        problems.append(f'no longer generated: {path}')
    for path in sorted(new_texts.keys() & old_texts.keys()):
        if new_texts[path] != old_texts[path]:
            problems.append(f'content differs: {path}')
    for key in sorted(crate_keys(new_components) - crate_keys(old_components)):
        problems.append(f'crate added: {key}')
    for key in sorted(crate_keys(old_components) - crate_keys(new_components)):
        problems.append(f'crate removed: {key}')
    for field in sorted(new_components.keys() | old_components.keys()):
        if new_components.get(field) != old_components.get(field):
            problems.append(f'{COMPONENTS} field differs: {field}')
    old_kit = (old_binding['sourceRelink'] or {}).get('artifact')
    new_kit = (new_binding['sourceRelink'] or {}).get('artifact')
    if old_kit != new_kit:
        print(f'build binding (not compared): committed kit {old_kit}, generated kit {new_kit}')
    return problems


def main(arguments):
    if len(arguments) != 2:
        print(__doc__, file=sys.stderr)
        return 2
    generated, committed = (Path(value).resolve() for value in arguments)
    for root in (generated, committed):
        if not (root / COMPONENTS).is_file():
            print(f'not a licence tree: {root}', file=sys.stderr)
            return 2
    problems = compare(generated, committed)
    for problem in problems:
        print(problem)
    if problems:
        print(f'{committed} is out of date: commit the generated tree verbatim', file=sys.stderr)
        return 1
    print(f'{committed} matches the generated inventory')
    return 0


if __name__ == '__main__':
    raise SystemExit(main(sys.argv[1:]))
