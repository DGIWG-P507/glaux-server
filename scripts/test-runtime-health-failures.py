"""Detect false healthy storage from a passing CLI/listener/database baseline."""

import json
import os
from pathlib import Path
import runpy
import sys
import tempfile

from database_harness import HarnessError
from test_runtime_health import FINAL, build_proof, require, run_binary, validate_output


ROOT = Path(__file__).resolve().parents[1]
HELPERS = runpy.run_path(str(ROOT / "scripts/test-validation-failures.py"))
SOURCE = Path("crates/glaux-server/src/runtime.rs")


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
        log = evidence / ("runtime-health-" + phase + ".log")
        log.write_text(output)
        row = {"phase": phase, "log": log.name, **facts}
        results.append(row)
        (evidence / "runtime-health-failure-controls.json").write_text(json.dumps(results, indent=2))
        print(json.dumps(row), flush=True)

    target = ROOT / "target"
    with tempfile.TemporaryDirectory(prefix="glaux-health-controls-", dir=runner_temp) as directory:
        baseline = Path(directory) / "baseline"
        faulty = Path(directory) / "always-healthy-storage"
        HELPERS["copy_source"](files, ROOT, baseline)
        output = run_binary(*build_proof(baseline, target))
        validate_output(output)
        record("baseline", output, passed=True)
        HELPERS["copy_source"](files, baseline, faulty)
        path = faulty / SOURCE
        source = path.read_text()
        old = "if health.storage_ready().await {"
        require(source.count(old) == 1, "Runtime readiness fault target changed")
        path.write_text(source.replace(old, "if health.storage_ready().await || true {"))
        try:
            binaries = build_proof(faulty, target)
            try:
                output = run_binary(*binaries)
            except HarnessError as error:
                output = str(error)
                detected = (
                    "Docker exec failed (101)" in output
                    and "unavailable storage incorrectly reported ready" in output
                    and "panicked at" in output
                    and "Runtime health group passed: actual-listener-minimal-health-only" in output
                    and "Runtime health group passed: isolated-storage-outage-and-recovery" not in output
                    and FINAL not in output
                )
            else:
                detected = False
            record("always-healthy-storage", output, detected=detected)
            require(detected, "False readiness escaped or failed for another reason: " + output)
        finally:
            require((ROOT / SOURCE).read_bytes() == original, "Real runtime source was modified")
            build_proof(ROOT, target)
        output = run_binary(target / "debug/glaux-server",
                            target / "debug/examples/runtime-health-proof")
        validate_output(output)
        record("restored", output, passed=True)
    print("Runtime health failure control: 1 detected; 0 escaped.", flush=True)


if __name__ == "__main__":
    main()
