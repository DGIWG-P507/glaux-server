"""Detect two lossy numeric faults only after their exact baselines pass."""

import json
import os
from pathlib import Path
import runpy
import sys
import tempfile


ROOT = Path(__file__).resolve().parents[1]
HELPERS = runpy.run_path(str(ROOT / "scripts/test-validation-failures.py"))
SOURCE = Path("crates/glaux-domain/src/numeric.rs")
CASES = [
    ("lossy-comparison", "numeric::tests::distinct_large_integers_never_collapse",
     "self.value.cmp(&other.value)",
     "self.value.to_f64().unwrap().partial_cmp(&other.value.to_f64().unwrap()).unwrap()",
     ["left: Equal", "right: Less"]),
    ("lossy-binary-conversion", "numeric::tests::binary64_preserves_exact_bits_not_shortest_decimal_display",
     "BigRational::from_float(value).ok_or(NumericError::NonFinite)?",
     "Self::parse_json_number(&value.to_string())?.value",
     ['left: "1"', 'right: "3602879701896397"']),
]


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
        (evidence / "numeric-failure-controls.json").write_text(json.dumps(outcomes, indent=2))
        print(json.dumps(row), flush=True)

    with tempfile.TemporaryDirectory(prefix="glaux-numeric-controls-", dir=runner_temp) as directory:
        owned = Path(directory)
        baseline = owned / "baseline"
        target = owned / "cargo-target"
        HELPERS["copy_source"](files, ROOT, baseline)
        for name, test, _, _, _ in CASES:
            status, output = HELPERS["run_test"](
                baseline, target, test, evidence / f"numeric-baseline-{name}.log",
                package="glaux-domain",
            )
            passed = HELPERS["passed_exact_test"](status, output, test)
            record({"phase": "baseline", "control": name, "test": test, **status, "passed": passed})
            require(passed, "Numeric baseline did not execute and pass: " + output)
        for name, test, old, new, expected in CASES:
            faulty = owned / name
            HELPERS["copy_source"](files, baseline, faulty)
            HELPERS["replace"](faulty / SOURCE, old, new)
            status, output = HELPERS["run_test"](
                faulty, target, test, evidence / f"numeric-fault-{name}.log",
                package="glaux-domain",
            )
            detected = HELPERS["detected_assertion"](status, output, test, expected)
            record({"phase": "fault", "control": name, "test": test, **status,
                    "expected_assertion_values": expected, "detected": detected})
            require(detected, "Numeric fault escaped or failed for another reason: " + output)
    print("Numeric failure controls: 2 detected; 0 escaped.", flush=True)


if __name__ == "__main__":
    main()
