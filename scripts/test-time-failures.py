"""Exact-time faults must compile and fail the intended passing assertions."""

import json
import os
from pathlib import Path
import runpy
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]
HELPERS = runpy.run_path(str(ROOT / "scripts/test-validation-failures.py"))
SOURCE = Path("crates/glaux-domain/src/temporal.rs")
CASES = [
    ("offset-sign", "temporal::tests::offset_sign_and_unknown_offset_preserve_instant",
     "local_second - i64::from(offset_seconds)", "local_second + i64::from(offset_seconds)",
     ["left: 7200", "right: 0"]),
    ("discard-fraction", "temporal::tests::fractional_boundaries_are_not_rounded",
     ".then_with(|| self.fraction.cmp(&other.fraction))", ".then_with(|| Ordering::Equal)",
     ["left: Equal", "right: Less"]),
    ("discard-leap-slot", "temporal::tests::leap_slot_is_distinct_and_offsets_are_checked_in_utc",
     ".then_with(|| self.leap.cmp(&other.leap))", ".then_with(|| Ordering::Equal)",
     ["left: Greater", "right: Less"]),
]


def main():
    require = HELPERS["require"]
    require(Path.cwd().resolve() == ROOT and not sys.argv[1:],
            "Run from workspace root without selection overrides.")
    runner_temp = Path(os.environ["RUNNER_TEMP"]).resolve()
    evidence = runner_temp / "glaux-ci-evidence"
    evidence.mkdir(exist_ok=True)
    files = HELPERS["source_files"]()
    outcomes = []

    def record(row):
        outcomes.append(row)
        (evidence / "time-failure-controls.json").write_text(json.dumps(outcomes, indent=2))
        print(json.dumps(row), flush=True)

    with tempfile.TemporaryDirectory(prefix="glaux-time-controls-", dir=runner_temp) as directory:
        owned = Path(directory)
        baseline, target = owned / "baseline", owned / "cargo-target"
        HELPERS["copy_source"](files, ROOT, baseline)
        for name, test, _, _, _ in CASES:
            status, output = HELPERS["run_test"](
                baseline, target, test, evidence / f"time-baseline-{name}.log",
                package="glaux-domain",
            )
            passed = HELPERS["passed_exact_test"](status, output, test)
            record({"phase": "baseline", "control": name, "test": test, **status, "passed": passed})
            require(passed, "Time baseline did not execute and pass: " + output)
        for name, test, old, new, expected in CASES:
            faulty = owned / name
            HELPERS["copy_source"](files, baseline, faulty)
            HELPERS["replace"](faulty / SOURCE, old, new)
            status, output = HELPERS["run_test"](
                faulty, target, test, evidence / f"time-fault-{name}.log",
                package="glaux-domain",
            )
            detected = HELPERS["detected_assertion"](status, output, test, expected)
            record({"phase": "fault", "control": name, "test": test, **status,
                    "expected_assertion_values": expected, "detected": detected})
            require(detected, "Time fault escaped or failed for another reason: " + output)
    print("Time failure controls: 3 detected; 0 escaped.", flush=True)


if __name__ == "__main__":
    main()
