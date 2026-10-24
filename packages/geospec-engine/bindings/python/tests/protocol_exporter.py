"""A subprocess protocol 1 exporter for tests. GEOSPEC_FIXTURE_MODE selects the response it gives."""

import hashlib
import json
import os
from pathlib import Path
import sys
import time

args = sys.argv[1:]


def value(flag):
    return args[args.index(flag) + 1] if flag in args else None


out = Path(value("--out") or ".")
project = Path(value("--project") or ".")
file = value("--file")
mode = os.environ.get("GEOSPEC_FIXTURE_MODE", "success")


def sha256(path):
    return hashlib.sha256((project / path).read_bytes()).hexdigest()


def success(**overrides):
    (out / "model.glb").write_bytes(b"\x01\x02\x03")
    (out / "texture.png").write_bytes(b"\x09")
    (out / "args.json").write_text(json.dumps({"args": args, "cwd": os.getcwd()}))
    document = {
        "protocol": 1,
        "format": value("--format"),
        "sourceUnit": "cm",
        "primary": "model.glb",
        "resources": [{"name": "texture.png", "path": "texture.png"}],
        "consumedSources": [{"path": file, "sha256": sha256(file)}, {"path": "lib/jaw.ts", "sha256": sha256("lib/jaw.ts")}],
    }
    document.update(overrides)
    return {key: entry for key, entry in document.items() if entry is not None}


def emit(document, exit=0):
    sys.stdout.write(json.dumps(document))
    sys.exit(exit)


# Record what the exporter received before any mode can fail, so tests can read it from the project.
record = os.environ.get("GEOSPEC_FIXTURE_RECORD")
if record:
    Path(record).write_text(json.dumps({"args": args, "cwd": os.getcwd(), "env": os.environ.get("GEOSPEC_FIXTURE_ENV")}))

if mode == "success":
    emit(success())
if mode == "bare":
    (out / "model.step").write_bytes(b"step")
    emit({"protocol": 1, "format": value("--format"), "primary": "model.step", "consumedSources": [{"path": file, "sha256": sha256(file)}]})
if mode == "refuse":
    emit({"protocol": 1, "error": {"code": "TAU_KERNEL_MISSING", "message": "No kernel for main.kcl.", "suggestion": "Add zoo()."}}, 3)
if mode == "refuse-exit-0":
    emit({"protocol": 1, "error": {"code": "FIXTURE_REFUSED", "message": "The fixture refused."}})
if mode == "garbage":
    sys.stderr.write("fixture stderr")
    sys.stdout.write("not json")
    sys.exit(3)
if mode == "nan":
    sys.stdout.write('{"protocol": 1, "format": "glb", "primary": "model.glb", "consumedSources": [], "x": NaN}')
    sys.exit(0)
if mode == "exit-1-success":
    emit(success(), 1)
if mode == "protocol-2":
    emit(success(protocol=2))
if mode == "protocol-true":
    emit(success(protocol=True))
if mode == "bad-error":
    emit({"protocol": 1, "error": {"code": "", "message": "empty code"}}, 1)
if mode == "unknown-format":
    emit(success(format="gltf"))
if mode == "other-format":
    emit(success(format="step" if value("--format") == "glb" else "glb"))
if mode == "bad-unit":
    emit(success(sourceUnit="furlong"))
if mode == "null-unit":
    emit(success(sourceUnit=None) | {"sourceUnit": None})
if mode == "bad-resources":
    emit(success(resources=[{"name": "texture.png"}]))
if mode == "escape-primary":
    emit(success(primary="../outside.glb"))
if mode == "absolute-primary":
    emit(success(primary=str(project / file)))
if mode == "escape-resource":
    emit(success(resources=[{"name": "secret", "path": "../../etc/passwd"}]))
if mode == "symlink-primary":
    (out / "link.glb").symlink_to(project / file)
    emit(success(primary="link.glb"))
if mode == "missing-primary":
    emit(success(primary="absent.glb"))
if mode == "nul-primary":
    emit(success(primary="model\u0000.glb"))
if mode == "nul-source":
    emit(success(consumedSources=[{"path": "main\u0000.ts", "sha256": "0" * 64}]))
if mode == "escape-source":
    emit(success(consumedSources=[{"path": "../outside.ts", "sha256": "0" * 64}]))
if mode == "stale-source":
    emit(success(consumedSources=[{"path": file, "sha256": "0" * 64}]))
if mode == "prefixed-and-missing":
    emit(success(consumedSources=[{"path": file, "sha256": "sha256:" + sha256(file).upper()}, {"path": "lib/absent.ts", "sha256": "missing"}]))
if mode == "missing-exists":
    emit(success(consumedSources=[{"path": file, "sha256": "missing"}]))
if mode == "bad-digest":
    emit(success(consumedSources=[{"path": file, "sha256": "md5:abc"}]))
if mode == "no-sources":
    emit(success(consumedSources=[]))
if mode == "signal":
    os.kill(os.getpid(), 9)
if mode == "hang":
    time.sleep(60)
sys.exit(f"unknown mode {mode}")
