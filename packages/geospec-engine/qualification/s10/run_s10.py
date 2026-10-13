#!/usr/bin/env python3
"""S10 cells F01, F02, F03 and H01 through the installed CPython wheel.

Usage: python -I run_s10.py --out <ledger.json>

Run it with an interpreter whose environment has the candidate wheel
installed. `-I` keeps this source tree off `sys.path`, so the probes import
the installed `geospec_engine_native`, never a checkout. The fixtures and the
frozen matrix are read from this directory's package. Ledger entries share the
Node runner's shape: `{cell, host, status, reason, evidence}`.
Exit: 0 when every probed cell passes; 1 otherwise.
"""

import argparse
import hashlib
import json
import platform
import resource
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
ENGINE_ROOT = HERE.parent.parent


def sha256(data):
    return hashlib.sha256(data).hexdigest()


def xorshift32(seed, length):
    """The Node runner's generator: identical bytes for a seed on every host."""
    state = (seed & 0xFFFFFFFF) or 1
    out = bytearray(length)
    for index in range(length):
        state ^= (state << 13) & 0xFFFFFFFF
        state ^= state >> 17
        state ^= (state << 5) & 0xFFFFFFFF
        out[index] = state & 0xFF
    return bytes(out)


def control(method, request_id, **fields):
    value = {
        "method": method,
        "requestId": request_id,
        "protocolVersion": 3,
        "registryVersion": 5,
        "canonicalProfile": "geospec-jcs-v1",
        **fields,
    }
    return json.dumps(value, separators=(",", ":")).encode()


def step_request(length):
    return control(
        "ingestSubject",
        "admit",
        format="step",
        frame={"coordinateSystem": "z-up", "sourceUnit": "auto", "outputUnit": "mm"},
        ingestOptions={},
        primaryByteLength=length,
        resources=[],
    )


def mesh_request(data):
    return control(
        "ingestSubject",
        "admit-mesh",
        contentHash=sha256(data),
        format="mesh-buffer-v1",
        frame={"coordinateSystem": "z-up", "unit": "mm"},
    )


class Probe:
    """One host session: fixtures, typed-outcome checks and the recovery."""

    def __init__(self, module, matrix):
        self.module = module
        self.matrix = matrix
        self.typed_codes = set(matrix["abi"]["typedCodes"])
        self.checks = []
        root = ENGINE_ROOT / matrix["fixtures"]["root"]
        self.fixtures = {}
        for name in ("fault", "truncated", "recovery"):
            declared = matrix["fixtures"][name]
            data = (root / declared["path"]).read_bytes()
            if sha256(data) != declared["sha256"]:
                raise SystemExit(f"Fixture {declared['path']} does not match its frozen SHA-256.")
            self.fixtures[name] = data

    def typed(self, name, operation):
        try:
            result = operation()
        except self.module.ProtocolError as error:
            code = getattr(error, "code", None)
            ok = code in self.typed_codes and len(str(error)) > 0
            self.checks.append({"name": name, "ok": ok, "outcome": "typed-error", "code": code})
            return code
        except Exception as error:  # noqa: BLE001 - an untyped failure is the finding.
            self.checks.append(
                {"name": name, "ok": False, "outcome": "untyped-error", "message": f"{type(error).__name__}: {error}"}
            )
            return None
        self.checks.append({"name": name, "ok": False, "outcome": "success", "sha256": sha256(bytes(result))})
        return None

    def admissions(self, engine):
        return json.loads(engine.observations())["physical"]["admissions"]

    def step_failures(self, engine, inputs):
        before = self.admissions(engine)
        for name, data in inputs:
            self.typed(name, lambda data=data: engine.ingest_subject(step_request(len(data)), data, []))
        after = self.admissions(engine)
        self.checks.append({"name": "no-partial-admission", "ok": before == after, "before": before, "after": after})

    def recover(self, engine):
        recovery = self.matrix["fixtures"]["recovery"]
        data = self.fixtures["recovery"]
        ingest = bytes(engine.ingest_subject(step_request(len(data)), data, []))
        subject_hash = json.loads(ingest)["result"]["subject"]["subjectHash"]
        claim = control(
            "submitClaims",
            "claim",
            plan={
                "subjects": [{"slot": "part", "subjectHash": subject_hash}],
                "claims": [
                    {
                        "claimId": "claim",
                        "capability": recovery["capability"],
                        "subjectSlots": ["part"],
                        "payload": recovery["payload"],
                        "polarity": "positive",
                        "workUnitBudget": recovery["workUnitBudget"],
                    }
                ],
            },
        )
        return {"ingest": sha256(ingest), "claim": sha256(bytes(engine.process_request(claim)))}


def probe_empty(probe, engine):
    empty = b""
    before = probe.admissions(engine)
    probe.typed("step-empty", lambda: engine.ingest_subject(step_request(0), empty, []))
    probe.typed("mesh-empty", lambda: engine.ingest_mesh(mesh_request(empty), empty))
    for method in ("process_request", "canonical_plan", "evaluate_plan", "evaluate_claim", "subject_handle"):
        probe.typed(f"{method}-empty", lambda method=method: getattr(engine, method)(empty))
    probe.typed("release_subject-empty", lambda: engine.release_subject(empty))
    probe.typed("canonicalize-empty", lambda: probe.module.canonicalize(empty))
    after = probe.admissions(engine)
    probe.checks.append({"name": "no-partial-admission", "ok": before == after, "before": before, "after": after})


def probe_truncated(probe, engine):
    fault = probe.fixtures["fault"]
    probe.step_failures(
        engine,
        [
            ("truncated-step", probe.fixtures["truncated"]),
            ("fault-prefix-1024", fault[:1024]),
            ("fault-prefix-half", fault[: len(fault) // 2]),
        ],
    )


def probe_garbage(probe, engine):
    small = xorshift32(0x53100003, 512)
    large = xorshift32(0x53100004, 4096)
    probe.step_failures(engine, [("garbage-step-512", small), ("garbage-step-4096", large)])
    probe.typed("garbage-mesh", lambda: engine.ingest_mesh(mesh_request(small), small))
    probe.typed("garbage-request", lambda: engine.process_request(large))
    probe.typed("garbage-canonicalize", lambda: probe.module.canonicalize(small))


PROBES = {"F01": probe_empty, "F02": probe_truncated, "F03": probe_garbage}


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--out", required=True, type=Path)
    args = parser.parse_args()
    import geospec_engine_native as module

    if Path(module.__file__).resolve().is_relative_to(ENGINE_ROOT):
        raise SystemExit(f"geospec_engine_native resolved inside the source tree: {module.__file__}")
    matrix = json.loads((HERE / "matrix.json").read_text())
    host = f"python-{sys.version_info.major}.{sys.version_info.minor}"
    control_digests = Probe(module, matrix).recover(module.Engine())
    entries = []
    recoveries = []
    for cell, run in PROBES.items():
        repetitions = []
        for rep in range(1, matrix["predeclared"]["ordinaryRepetitions"] + 1):
            probe = Probe(module, matrix)
            engine = module.Engine()
            run(probe, engine)
            recovery = probe.recover(engine)
            engine.close()
            recoveries.append({"cell": cell, "rep": rep, "recovery": recovery, "equal": recovery == control_digests})
            signature = sha256(json.dumps([[c["name"], c["ok"], c.get("outcome"), c.get("code")] for c in probe.checks]).encode())
            repetitions.append({"signature": signature, "checks": probe.checks})
        failures = [
            f"repetition {index + 1} check {check['name']} failed: {json.dumps(check)}"
            for index, repetition in enumerate(repetitions)
            for check in repetition["checks"]
            if not check["ok"]
        ]
        if len({repetition["signature"] for repetition in repetitions}) != 1:
            failures.append("repetitions disagree")
        entries.append(
            {
                "cell": cell,
                "host": host,
                "status": "fail" if failures else "pass",
                "reason": "; ".join(failures),
                "evidence": {"repetitions": repetitions},
            }
        )
    unequal = [f"{item['cell']}#{item['rep']}" for item in recoveries if not item["equal"]]
    entries.append(
        {
            "cell": "H01",
            "host": host,
            "status": "fail" if unequal else "pass",
            "reason": ", ".join(unequal),
            "evidence": {"control": control_digests, "recoveries": recoveries},
        }
    )
    peak = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss
    # Darwin reports ru_maxrss in bytes, Linux in KiB.
    peak_bytes = peak if sys.platform == "darwin" else peak * 1024
    if peak_bytes > matrix["limits"]["maxRssBytes"]:
        for entry in entries:
            entry["status"] = "fail"
            entry["reason"] = f"peak RSS {peak_bytes} exceeds {matrix['limits']['maxRssBytes']}; {entry['reason']}"
    ledger = {
        "schema": "geospec-s10-ledger-v1",
        "host": host,
        "runtime": platform.python_version(),
        "platform": f"{sys.platform}-{platform.machine()}",
        "matrixSha256": sha256((HERE / "matrix.json").read_bytes()),
        "module": module.__file__,
        "maxRssBytes": peak_bytes,
        "control": control_digests,
        "entries": entries,
    }
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(ledger, indent=2) + "\n")
    for entry in entries:
        print(entry["cell"], entry["status"], entry["reason"])
    return 0 if all(entry["status"] == "pass" for entry in entries) else 1


if __name__ == "__main__":
    sys.exit(main())
