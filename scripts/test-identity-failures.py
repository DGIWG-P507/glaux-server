"""Prove wrong UUID version acceptance trips its exact assertion, after a pass.

Uses the existing bounded disposable-source/logging helpers; never modifies the
checkout. A compiler/setup failure is not an assertion detection.
"""

import json
import os
from pathlib import Path
import runpy
import sys
import tempfile


ROOT = Path(__file__).resolve().parents[1]
HELPERS = runpy.run_path(str(ROOT / "scripts/test-validation-failures.py"))
TEST = "identity::tests::local_id_rejects_every_wrong_version_and_variant"


def main():
    require = HELPERS["require"]
    require(Path.cwd().resolve() == ROOT and not sys.argv[1:],
            "Run from the workspace root without selection overrides.")
    runner_temp = Path(os.environ["RUNNER_TEMP"]).resolve()
    evidence = runner_temp / "glaux-ci-evidence"
    evidence.mkdir(exist_ok=True)
    files = HELPERS["source_files"]()
    outcomes = []

    def record(row):
        outcomes.append(row)
        (evidence / "identity-failure-controls.json").write_text(json.dumps(outcomes, indent=2))
        print(json.dumps(row), flush=True)

    with tempfile.TemporaryDirectory(prefix="glaux-identity-controls-", dir=runner_temp) as directory:
        owned = Path(directory)
        baseline = owned / "baseline"
        target = owned / "cargo-target"
        HELPERS["copy_source"](files, ROOT, baseline)
        status, output = HELPERS["run_test"](
            baseline, target, TEST, evidence / "identity-baseline.log", package="glaux-domain",
        )
        passed = HELPERS["passed_exact_test"](status, output, TEST)
        record({"phase": "baseline", "test": TEST, **status, "passed": passed})
        require(passed, "Identity baseline did not execute and pass: " + output)
        faulty = owned / "wrong-version"
        HELPERS["copy_source"](files, baseline, faulty)
        HELPERS["replace"](
            faulty / "crates/glaux-domain/src/identity.rs",
            "if bytes[6] >> 4 != 7 || bytes[8] & 0xc0 != 0x80 {",
            "if bytes[8] & 0xc0 != 0x80 {",
        )
        status, output = HELPERS["run_test"](
            faulty, target, TEST, evidence / "identity-wrong-version.log", package="glaux-domain",
        )
        detected = HELPERS["detected_assertion"](
            status, output, TEST, ["left: true", "right: false"],
        ) and "017f22e2-79b0-0cc3-88c4-dc0c0c07398f" in output
        record({"phase": "fault", "test": TEST, **status, "detected": detected})
        require(detected, "Wrong-version acceptance escaped or failed for another reason: " + output)
    print("Identity failure control: 1 detected; 0 escaped.", flush=True)


if __name__ == "__main__":
    main()
