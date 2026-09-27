"""Detect bypassed source permission from a passing isolated HTTP/database proof."""

import json
import os
from pathlib import Path
import runpy
import sys
import tempfile

from database_harness import HarnessError
from test_authorization import FINAL, build_proof, require, run_binary, validate_output


ROOT = Path(__file__).resolve().parents[1]
HELPERS = runpy.run_path(str(ROOT / "scripts/test-validation-failures.py"))
SOURCE = Path("crates/glaux-server/src/authorization.rs")


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
        log = evidence / ("authorization-" + phase + ".log")
        log.write_text(output)
        row = {"phase": phase, "log": log.name, **facts}
        results.append(row)
        (evidence / "authorization-failure-controls.json").write_text(json.dumps(results, indent=2))
        print(json.dumps(row), flush=True)

    target = ROOT / "target"
    with tempfile.TemporaryDirectory(prefix="glaux-authorization-controls-", dir=runner_temp) as directory:
        baseline = Path(directory) / "baseline"
        faulty = Path(directory) / "source-bypass"
        HELPERS["copy_source"](files, ROOT, baseline)
        output = run_binary(build_proof(baseline, target))
        validate_output(output)
        record("baseline", output, passed=True)
        HELPERS["copy_source"](files, baseline, faulty)
        path = faulty / SOURCE
        source = path.read_text()
        anchor = "grant.source == source // SOURCE_PERMISSION_COMPARISON"
        require(source.count(anchor) == 1, "Source-permission mutation anchor changed")
        # Preserve action/resource selection, but admit every source. The valid
        # baseline must still work so the intended denied-source case detects it.
        path.write_text(source.replace(anchor,
                        "(grant.source == source || grant.source != source) // SOURCE_PERMISSION_COMPARISON"))
        try:
            binary = build_proof(faulty, target)
            try:
                output = run_binary(binary)
            except HarnessError as error:
                output = str(error)
                detected = (
                    "Docker exec failed (101)" in output
                    and "cross-source create escaped authorization" in output
                    and "panicked at" in output
                    and "Authorization group passed: exact-authorized-queries" in output
                    and "Authorization group passed: accepted-write-and-source-boundary" not in output
                    and FINAL not in output
                )
            else:
                detected = False
            record("source-bypass", output, detected=detected)
            require(detected, "Source-permission bypass escaped or failed for another reason: " + output)
        finally:
            require((ROOT / SOURCE).read_bytes() == original, "Real authorization source was modified")
            build_proof(ROOT, target)
        output = run_binary(target / "debug/examples/authorization-proof")
        validate_output(output)
        record("restored", output, passed=True)
    print("Authorization failure control: 1 detected; 0 escaped.", flush=True)


if __name__ == "__main__":
    main()
