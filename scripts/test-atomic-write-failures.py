"""Detect omitted durable work from a passing real-database baseline.

Only a disposable source copy is changed. Each execution receives a new owned
database. Build/setup/cleanup failures and timeouts are not detected mutations.
"""

import json
import os
from pathlib import Path
import runpy
import sys
import tempfile

from database_harness import HarnessError
from test_atomic_write import FINAL, build_proof, require, run_binary, validate_output


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
        log = evidence / ("atomic-write-" + phase + ".log")
        log.write_text(output)
        row = {"phase": phase, "log": log.name, **facts}
        results.append(row)
        (evidence / "atomic-write-failure-controls.json").write_text(json.dumps(results, indent=2))
        print(json.dumps(row), flush=True)

    # Reuse compiled dependencies, not test results. Cargo rebuilds this example
    # from the selected source tree; always restore the checkout's binary below.
    target = ROOT / "target"
    with tempfile.TemporaryDirectory(prefix="glaux-atomic-controls-", dir=runner_temp) as directory:
        baseline = Path(directory) / "baseline"
        faulty = Path(directory) / "omit-outgoing"
        HELPERS["copy_source"](files, ROOT, baseline)
        output = run_binary(build_proof(baseline, target))
        validate_output(output)
        record("baseline", output, passed=True)
        HELPERS["copy_source"](files, baseline, faulty)
        path = faulty / SOURCE
        source = path.read_text()
        anchor = "// Required outgoing work shares this transaction."
        require(source.count(anchor) == 1, "Outgoing-work mutation anchor changed")
        before, after = source.split(anchor)
        lines = after.splitlines(keepends=True)
        call_index = next((i for i, line in enumerate(lines) if line.strip()), None)
        require(call_index is not None, "Outgoing-work insertion call is absent")
        call = lines[call_index].strip()
        require(call.startswith("insert_outgoing(") and call.endswith(".await?;"),
                "Outgoing-work insertion no longer has the reviewed single-call form")
        lines[call_index] = "        // Disposable control: intentionally omit outgoing work.\n"
        path.write_text(before + anchor + "".join(lines))
        try:
            binary = build_proof(faulty, target)
            try:
                output = run_binary(binary)
            except HarnessError as error:
                output = str(error)
                detected = (
                    "Docker exec failed (101)" in output
                    and "required outgoing work facts missing" in output
                    and "panicked at" in output
                    and "Atomic write group passed: migration-preservation" in output
                    and "Atomic write group passed: accepted-exact-facts" not in output
                    and FINAL not in output
                )
            else:
                detected = False
            record("omit-outgoing", output, detected=detected)
            require(detected, "Omitted outgoing work escaped or failed for another reason: " + output)
        finally:
            require((ROOT / SOURCE).read_bytes() == original, "Real application source was modified")
            build_proof(ROOT, target)
        output = run_binary(target / "debug/examples/atomic-write-proof")
        validate_output(output)
        record("restored", output, passed=True)
    print("Atomic write failure control: 1 detected; 0 escaped.", flush=True)


if __name__ == "__main__":
    main()
