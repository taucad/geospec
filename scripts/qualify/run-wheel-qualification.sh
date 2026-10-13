#!/usr/bin/env bash
# Qualify one candidate wheel under one CPython in a clean virtual environment.
#
# The wheel installs from its file alone (no index, no dependencies), then the
# pinned `test` extra from the wheel's own metadata. pytest runs the binding
# tests and the lifecycle host test from a scratch directory with
# `--geospec-installed`, which refuses to start unless `geospec` and
# `geospec_engine_native` import from the environment's site-packages. The
# early and material corpus reports the JS/Python equality check compares
# come from the same installed package.
#
# Writes into <leg dir>/evidence: wheel.json (interpreter, wheel digest,
# installed locations, pytest exit status), pytest.xml, pip-install.json,
# python-early.json and python-material.json. Exits non-zero when pytest or
# either corpus report fails, after every output is written.
#
# Usage: run-wheel-qualification.sh --wheel <file.whl> --python <interpreter> --out <leg dir>

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/../.." && pwd)"
engine_root="$repo_root/$(node -p "require('$repo_root/names.json').engine.root")"
tests_dir="$engine_root/bindings/python/tests"
lifecycle_test="$engine_root/host-tests/lifecycle-m1/test_lifecycle.py"

wheel=''
python=''
out=''
while [[ $# -gt 0 ]]; do
  case "$1" in
    --wheel) wheel="$2"; shift 2 ;;
    --python) python="$2"; shift 2 ;;
    --out) out="$2"; shift 2 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done
if [[ -z "$wheel" || -z "$python" || -z "$out" ]]; then
  echo 'usage: run-wheel-qualification.sh --wheel <file.whl> --python <interpreter> --out <leg dir>' >&2
  exit 2
fi
wheel="$(cd "$(dirname "$wheel")" && pwd)/$(basename "$wheel")"
evidence="$(mkdir -p "$out/evidence" && cd "$out/evidence" && pwd)"

scratch="$(mktemp -d)"
trap 'rm -rf "$scratch"' EXIT
venv="$scratch/venv"
"$python" -m venv "$venv"
venv_python="$venv/bin/python"
unset PYTHONPATH
export PYTHONDONTWRITEBYTECODE=1 PIP_DISABLE_PIP_VERSION_CHECK=1

"$venv_python" -m pip install --no-index --no-deps --report "$evidence/pip-install.json" "$wheel"
test_requirements=()
while IFS= read -r requirement; do
  test_requirements+=("$requirement")
done < <(
  "$venv_python" - <<'PY'
import re
from importlib.metadata import requires
for line in requires("geospec-engine") or []:
    requirement, _, marker = line.partition(";")
    if re.fullmatch(r"\s*extra\s*==\s*[\"']test[\"']\s*", marker):
        print(requirement.strip())
PY
)
if [[ ${#test_requirements[@]} -eq 0 ]]; then
  echo 'the wheel declares no test extra' >&2
  exit 1
fi
"$venv_python" -m pip install "${test_requirements[@]}"

# Run from a scratch directory so neither the working directory nor a rootdir
# ini file can put the source tree on sys.path.
cd "$scratch"
pytest_exit=0
"$venv_python" -m pytest -p no:cacheprovider --geospec-installed --junitxml "$evidence/pytest.xml" -q \
  "$tests_dir" "$lifecycle_test" || pytest_exit=$?

report_exit=0
"$venv_python" "$tests_dir/test_conformance.py" "$evidence/python-early.json" || report_exit=$?
"$venv_python" "$tests_dir/test_conformance.py" --material "$evidence/python-material.json" || report_exit=$?

WHEEL="$wheel" PYTEST_EXIT="$pytest_exit" SOURCE_ROOT="$engine_root/bindings/python/python" \
  "$venv_python" - > "$evidence/wheel.json" <<'PY'
import hashlib
import json
import os
import platform
import sys
from importlib.metadata import version
from pathlib import Path

import geospec
import geospec_engine_native

source_root = Path(os.environ["SOURCE_ROOT"]).resolve()
locations = {m.__name__: str(Path(m.__file__).resolve()) for m in (geospec, geospec_engine_native)}
wheel = Path(os.environ["WHEEL"])
print(json.dumps({
    "python": platform.python_version(),
    "implementation": platform.python_implementation(),
    "executable": sys.executable,
    "platform": platform.platform(),
    "wheel": wheel.name,
    "wheelSha256": hashlib.sha256(wheel.read_bytes()).hexdigest(),
    "distributionVersion": version("geospec-engine"),
    "installed": locations,
    "fromSourceTree": any(Path(p).is_relative_to(source_root) for p in locations.values()),
    "pytestExit": int(os.environ["PYTEST_EXIT"]),
}, indent=2))
PY

echo "pytest exit ${pytest_exit}; corpus report exit ${report_exit}"
if [[ "$pytest_exit" -ne 0 || "$report_exit" -ne 0 ]]; then
  exit 1
fi
