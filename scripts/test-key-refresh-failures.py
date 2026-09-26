"""A compiled stale-key fault must fail the named expired-trust assertion."""

import json
import os
from pathlib import Path
import runpy
import sys
import tempfile

from test_key_refresh import FINAL, build_proof, require, run_binary, validate_output


ROOT = Path(__file__).resolve().parents[1]
HELPERS = runpy.run_path(str(ROOT / "scripts/test-validation-failures.py"))
SOURCE = Path("crates/glaux-server/src/authentication/keys.rs")


def main():
    require(Path.cwd().resolve() == ROOT and not sys.argv[1:],
            "Run from workspace root without target or selection overrides")
    runner_temp = Path(os.environ["RUNNER_TEMP"]).resolve()
    evidence = runner_temp / "glaux-ci-evidence"
    evidence.mkdir(exist_ok=True)
    files = HELPERS["source_files"]()
    original = (ROOT / SOURCE).read_bytes()
    results = []

    def record(phase, output, **facts):
        log = evidence / ("key-refresh-" + phase + ".log")
        log.write_text(output)
        row = {"phase": phase, "log": log.name, **facts}
        results.append(row)
        (evidence / "key-refresh-failure-controls.json").write_text(json.dumps(results, indent=2))
        print(json.dumps(row), flush=True)

    def passing(proof, owner, phase):
        owner.mkdir()
        result = run_binary(proof, owner)
        output = result.stdout + result.stderr
        record(phase, output, exit=result.returncode, passed=result.returncode == 0)
        require(result.returncode == 0, "Unmodified key-refresh proof failed: " + output)
        validate_output(output)

    target = ROOT / "target"
    with tempfile.TemporaryDirectory(prefix="glaux-key-controls-", dir=runner_temp) as directory:
        owner = Path(directory)
        baseline = owner / "baseline"
        faulty = owner / "stale-trust"
        HELPERS["copy_source"](files, ROOT, baseline)
        passing(build_proof(baseline, target), owner / "baseline-fixture", "baseline")
        HELPERS["copy_source"](files, baseline, faulty)
        path = faulty / SOURCE
        source = path.read_text()
        old = "let fresh = state.fresh(now);"
        new = "let fresh = state.fresh(now) || !state.keys.is_empty();"
        require(source.count(old) == 1, "Stale-trust fault target changed")
        path.write_text(source.replace(old, new))
        try:
            proof = build_proof(faulty, target)
            fixture = owner / "faulty-fixture"
            fixture.mkdir()
            result = run_binary(proof, fixture)
            output = result.stdout + result.stderr
            detected = (
                result.returncode == 101
                and "expired trust accepted during issuer outage" in output
                and "panicked at" in output
                and "Key refresh group passed: rotation-and-bounded-unknown-keys" in output
                and "Key refresh group passed: expired-trust-outage-and-recovery" not in output
                and FINAL not in output
            )
            record("stale-trust", output, exit=result.returncode, detected=detected)
            require(detected, "Stale trust escaped or failed for another reason: " + output)
        finally:
            require((ROOT / SOURCE).read_bytes() == original, "Real key-refresh source changed")
            build_proof(ROOT, target)
        passing(target / "debug/examples/key-refresh-proof", owner / "restored-fixture", "restored")
    print("Key refresh failure control: 1 detected; 0 escaped.", flush=True)


if __name__ == "__main__":
    main()
