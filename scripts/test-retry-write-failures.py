"""Detect omitted intent comparison, from passing real-database baseline."""

import json
import os
from pathlib import Path
import runpy
import sys
import tempfile

from database_harness import HarnessError
from test_retry_write import FINAL, build_proof, require, run_binary, validate_output


ROOT = Path(__file__).resolve().parents[1]
HELPERS = runpy.run_path(str(ROOT / "scripts/test-validation-failures.py"))
SOURCE = Path("crates/glaux-server/src/application.rs")


def main():
    require(Path.cwd().resolve() == ROOT and not sys.argv[1:],
            "Run from the workspace root without target or selection overrides")
    runner_temp = Path(os.environ["RUNNER_TEMP"]).resolve()
    evidence = runner_temp / "glaux-ci-evidence"
    evidence.mkdir(exist_ok=True)
    files = HELPERS["source_files"]()
    original = (ROOT / SOURCE).read_bytes()
    results = []

    def record(phase, output, **facts):
        log = evidence / ("retry-write-" + phase + ".log")
        log.write_text(output)
        row = {"phase": phase, "log": log.name, **facts}
        results.append(row)
        (evidence / "retry-write-failure-controls.json").write_text(json.dumps(results, indent=2))
        print(json.dumps(row), flush=True)

    target = ROOT / "target"
    with tempfile.TemporaryDirectory(prefix="glaux-retry-controls-", dir=runner_temp) as directory:
        baseline = Path(directory) / "baseline"
        faulty = Path(directory) / "omit-intent-comparison"
        HELPERS["copy_source"](files, ROOT, baseline)
        output = run_binary(build_proof(baseline, target))
        validate_output(output)
        record("baseline", output, passed=True)
        HELPERS["copy_source"](files, baseline, faulty)
        path = faulty / SOURCE
        source = path.read_text()
        old = "if stored_digest != *digest {"
        require(source.count(old) == 1, "Retry content-comparison fault target changed")
        path.write_text(source.replace(old, "if false && stored_digest != *digest {"))
        try:
            binary = build_proof(faulty, target)
            try:
                output = run_binary(binary)
            except HarnessError as error:
                output = str(error)
                detected = (
                    "Docker exec failed (101)" in output
                    and "changed retry intent was not rejected: case 0" in output
                    and "panicked at" in output
                    and "Retry write group passed: original-outcome-generated-ids-lost-response" in output
                    and "Retry write group passed: every-content-field-conflicts-without-effect" not in output
                    and FINAL not in output
                )
            else:
                detected = False
            record("omit-intent-comparison", output, detected=detected)
            require(detected, "Omitted intent comparison escaped or failed for another reason: " + output)
        finally:
            require((ROOT / SOURCE).read_bytes() == original, "Real application source was modified")
            build_proof(ROOT, target)
        output = run_binary(target / "debug/examples/retry-write-proof")
        validate_output(output)
        record("restored", output, passed=True)
    print("Retry write failure control: 1 detected; 0 escaped.", flush=True)


if __name__ == "__main__":
    main()
