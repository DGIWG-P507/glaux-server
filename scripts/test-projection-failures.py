"""Direction-policy faults must compile and fail the precise passing assertion."""

import json
import os
from pathlib import Path
import runpy
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]
HELPERS = runpy.run_path(str(ROOT / "scripts/test-validation-failures.py"))
SOURCE = Path("crates/glaux-standards/src/projection.rs")
CASES = [
    ("original-as-request",
     "projection::tests::minimal_requests_and_complete_responses_have_independent_contracts",
     "let requests = request_catalog(&originals)?;", "let requests = originals.clone();",
     ["left: Err(Structure)", "right: Ok(())"]),
    ("write-only-output-leak", "projection::tests::stream_schema_is_write_only_in_responses",
     'if resource.stream() && value.get("schema").is_some() {',
     'if false && resource.stream() && value.get("schema").is_some() {',
     ["left: Ok(())", 'right: Err(WriteOnly("/schema"))']),
    ("protected-uid-bypass",
     "projection::policy_tests::trusted_context_separates_uid_parent_and_ignored_local_id",
     ".is_some_and(|expected| expected != &uid)",
     ".is_some_and(|expected| expected != &uid && false)",
     ['left: Ok(Object {"definition": String("http://www.w3.org/ns/sosa/Sensor"), '
      '"label": String("Sensor"), "type": String("PhysicalSystem"), '
      '"uniqueId": String("urn:example:sensor:two")})',
      'right: Err(Protected("/uniqueId"))']),
    ("locked-schema-removal",
     "projection::policy_tests::locked_contract_rejects_change_and_patch_removal",
     'projection == Projection::MergedPatch && value.get("schema").is_none()',
     'false && projection == Projection::MergedPatch && value.get("schema").is_none()',
     ['left: Ok(Object {"name": String("Temperature")})',
      'right: Err(Protected("/schema"))']),
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
        (evidence / "projection-failure-controls.json").write_text(json.dumps(outcomes, indent=2))
        print(json.dumps(row), flush=True)

    with tempfile.TemporaryDirectory(prefix="glaux-projection-controls-", dir=runner_temp) as directory:
        owned = Path(directory)
        baseline, target = owned / "baseline", owned / "cargo-target"
        HELPERS["copy_source"](files, ROOT, baseline)
        for name, test, _, _, _ in CASES:
            status, output = HELPERS["run_test"](
                baseline, target, test, evidence / f"projection-baseline-{name}.log",
            )
            passed = HELPERS["passed_exact_test"](status, output, test)
            record({"phase": "baseline", "control": name, "test": test, **status, "passed": passed})
            require(passed, "Projection baseline did not execute and pass: " + output)
        for name, test, old, new, expected in CASES:
            faulty = owned / name
            HELPERS["copy_source"](files, baseline, faulty)
            HELPERS["replace"](faulty / SOURCE, old, new)
            status, output = HELPERS["run_test"](
                faulty, target, test, evidence / f"projection-fault-{name}.log",
            )
            detected = HELPERS["detected_assertion"](status, output, test, expected)
            record({"phase": "fault", "control": name, "test": test, **status,
                    "expected_assertion_values": expected, "detected": detected})
            require(detected, "Projection fault escaped or failed for another reason: " + output)
    print("Projection failure controls: 4 detected; 0 escaped.", flush=True)


if __name__ == "__main__":
    main()
