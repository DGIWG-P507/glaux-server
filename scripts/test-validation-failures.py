"""Prove structural and scalar assertions detect bounded disposable faults.

Run after the unmodified required suite passes on the approved hosted runner.
Each selected test must also pass in a fresh source copy before any faults run.
Only this script's TemporaryDirectory is cleaned; the real checkout is read-only.
"""

import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import time


ROOT = Path(__file__).resolve().parents[1]
VALIDATION = Path("crates/glaux-standards/src/validation.rs")
GUARD = Path("crates/glaux-standards/src/schema_guard.rs")
SCALAR = Path("crates/glaux-standards/src/scalar.rs")
SCALAR_DOMAIN = Path("crates/glaux-domain/src/scalar.rs")
NUMERIC_SCALAR = Path("crates/glaux-standards/src/scalar/numeric.rs")
TIME_SCALAR = Path("crates/glaux-standards/src/scalar/time.rs")
RANGE = Path("crates/glaux-standards/src/range.rs")
AGGREGATE = Path("crates/glaux-standards/src/aggregate.rs")
CHOICE_VALUE = Path("crates/glaux-standards/src/choice/value.rs")
ARRAY = Path("crates/glaux-standards/src/array.rs")
TIMEOUT_SECONDS = 180


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def replace(path, old, new):
    original = path.read_text()
    require(original.count(old) == 1, f"Fault target changed: {path.name}: {old}")
    path.write_text(original.replace(old, new))


def wrong_binary_root(root):
    replace(root / VALIDATION,
            'format!("{SWE}encodings.json#/$defs/BinaryEncoding")',
            'format!("{SWE}encodings.json")')


def accept_quantity_without_structural_validation(root):
    path = root / VALIDATION
    original = path.read_text()
    start = "    pub fn validate("
    end = "    /// Separately checked descriptor"
    require(original.count(start) == 1 and original.count(end) == 1,
            "Fault target changed: structural validation function boundaries.")
    before, function = original.split(start)
    function, after = function.split(end)
    old = "if self.validators[&contract].is_valid(&value) {"
    require(function.count(old) == 1, "Fault target changed: structural validation condition.")
    changed = function.replace(
        old, "if contract == Contract::Quantity || self.validators[&contract].is_valid(&value) {",
    )
    path.write_text(before + start + changed + end + after)


def bypass_raw_size_limit(root):
    replace(root / VALIDATION, "if input.len() > MAX_BYTES {",
            "if false && input.len() > MAX_BYTES {")


def ignore_unallowlisted_schema_references(root):
    old = "let target = guard.target(&reference.target)?;"
    new = """if !guard.resources.contains_key(&reference.target) {
            continue;
        }
        let target = guard.target(&reference.target)?;"""
    replace(root / GUARD, old, new)


def collapse_false_to_absent(root):
    replace(root / SCALAR_DOMAIN, "value.map(ScalarValue::Boolean)",
            "value.filter(|value| *value).map(ScalarValue::Boolean)")


def collapse_empty_text_to_absent(root):
    replace(root / SCALAR_DOMAIN, "value.clone().map(ScalarValue::Text)",
            "value.clone().filter(|value| !value.is_empty()).map(ScalarValue::Text)")


def bypass_category_membership(root):
    replace(root / SCALAR,
            "self.check_tokens(value)?; // Category membership must not be bypassed.",
            "// Fault: skip Category membership checking.")


def round_large_count(root):
    replace(root / NUMERIC_SCALAR,
            "CountValue::try_from(value).map_err(ScalarError::Numeric)",
            """let value = if value == ExactNumber::parse_json_number("9007199254740993").unwrap() {
        ExactNumber::parse_json_number("9007199254740992").unwrap()
    } else { value };
    CountValue::try_from(value).map_err(ScalarError::Numeric)""")


def bypass_numeric_constraint(root):
    replace(root / NUMERIC_SCALAR, "if !(enumerated || in_interval) {",
            "if false && !(enumerated || in_interval) {")


def silently_rewrite_unit(root):
    replace(root / NUMERIC_SCALAR, "        code,\n        href,",
            '        code: code.map(|code| if code == "cm" { "m".to_owned() } else { code }),\n        href,')


def truncate_time_fraction(root):
    replace(root / TIME_SCALAR,
            "ExactInstant::parse_rfc3339(text).map_err(ScalarError::Time)?",
            'ExactInstant::parse_rfc3339(&text.replace(".1234567890123456789", ".123456")).map_err(ScalarError::Time)?')


def invent_numeric_utc_instant(root):
    replace(root / SCALAR_DOMAIN,
            "_ => Err(UnsupportedTimeConversion),",
            """_ => {
                static INVENTED: std::sync::OnceLock<ExactInstant> = std::sync::OnceLock::new();
                Ok(INVENTED.get_or_init(|| ExactInstant::parse_rfc3339("1970-01-01T00:00:00Z").unwrap()))
            },""")


def accept_extra_range_endpoint(root):
    replace(root / RANGE, "if values.len() != 2 {", "if values.len() < 2 {")


def sort_record_fields(root):
    replace(root / AGGREGATE, "        for child in raw_children {",
            """        let mut raw_children = raw_children;
        if contract == Contract::DataRecord {
            raw_children.sort_by_key(|child| {
                let value: serde_json::Value = serde_json::from_str(child.get()).unwrap();
                value["name"].as_str().unwrap().to_owned()
            });
        }
        for child in raw_children {""")


def accept_another_choice_arm(root):
    replace(root / CHOICE_VALUE,
            "let value = check_child(child, &selected.value).map_err(|error| error.at(index))?;",
            """let value = check_child(child, &selected.value).or_else(|original| {
                contract.children().iter()
                    .find_map(|alternative| check_child(alternative, &selected.value).ok())
                    .ok_or(original)
            }).map_err(|error| error.at(index))?;""")


def drop_inner_array_dimensions(root):
    replace(root / ARRAY, "dimensions.push(element_count);",
            "if dimensions.is_empty() { dimensions.push(element_count); }")


CASES = [
    ("array-inner-dimension-dropped", "array::tests::array_fixed_dimensions_preserve_exact_order",
     drop_inner_array_dimensions, ['left: [Some("9007199254740993")]',
                                  'right: [Some("9007199254740993"), Some("7")]']),
    ("choice-wrong-arm-fallback", "choice::tests::choice_dispatches_exact_selected_arm",
     accept_another_choice_arm, ["left: None", "right: Some(Scalar(ConstraintViolation))"]),
    ("record-fields-sorted", "aggregate::tests::aggregate_record_preserves_declared_field_order",
     sort_record_fields, ['left: ["aBand", "mNested", "zCount"]',
                          'right: ["zCount", "aBand", "mNested"]']),
    ("range-extra-endpoint", "range::tests::range_pair_cardinality_rejects_extra_values",
     accept_extra_range_endpoint, ["left: None", "right: Some(Cardinality)"]),
    ("time-fraction-truncated", "scalar::time_tests::time_calendar_defaults_preserve_exact_instants",
     truncate_time_fraction, ['left: "0.123456"', 'right: "0.1234567890123456789"']),
    ("time-numeric-invented-utc", "scalar::time_tests::time_numeric_coordinates_preserve_origin_and_context",
     invent_numeric_utc_instant, ["left: None", "right: Some(UnsupportedTimeConversion)"]),
    ("binary-aggregate-root", "validation::tests::fixed_encoding_selection", wrong_binary_root,
     ["left: Err(Structure)", "right: Ok(())"]),
    ("quantity-structural-bypass", "validation::tests::parser_fuzz_regressions",
     accept_quantity_without_structural_validation, ["left: Ok(())", "right: Err(Structure)"]),
    ("raw-size-limit-bypass", "validation::tests::limits_and_safe_parse", bypass_raw_size_limit,
     ["left: Err(Malformed)", "right: Err(Size)"]),
    ("unallowlisted-reference-bypass", "schema_guard::tests::rejects_http_file_data_and_uri_escape_canaries",
     ignore_unallowlisted_schema_references,
     ["left: Ok(())", 'right: Err("schema reference is outside the embedded catalog")']),
    ("scalar-false-collapsed", "scalar::tests::scalar_source_metadata_and_presence",
     collapse_false_to_absent, ["left: None", "right: Some(Boolean(false))"]),
    ("scalar-empty-text-collapsed", "scalar::tests::scalar_source_metadata_and_presence",
     collapse_empty_text_to_absent, ["left: None", 'right: Some(Text(""))']),
    ("scalar-category-membership-bypass",
     "scalar::tests::scalar_enumerations_preserve_tokens_and_enforce_membership",
     bypass_category_membership, ["left: None", "right: Some(ConstraintViolation)"]),
    ("numeric-count-rounded", "scalar::numeric_tests::numeric_count_exact_large_values",
     round_large_count, ["left: Ok(9007199254740992)", "right: Ok(9007199254740993)"]),
    ("numeric-constraint-bypass", "scalar::numeric_tests::numeric_constraints_are_inclusive_unions",
     bypass_numeric_constraint, ["left: None", "right: Some(ConstraintViolation)"]),
    ("numeric-unit-rewrite", "scalar::numeric_tests::numeric_source_metadata_and_units_are_preserved",
     silently_rewrite_unit, ['left: Some("m")', 'right: Some("cm")']),
]


def source_files():
    result = subprocess.run(
        ["git", "ls-files", "-z"], cwd=ROOT, check=True,
        stdout=subprocess.PIPE, timeout=30,
    )
    files = [Path(path) for path in result.stdout.decode().split("\0") if path]
    require(files, "No tracked source files found for validation controls.")
    for relative in files:
        source = ROOT / relative
        require(not relative.is_absolute() and ".." not in relative.parts
                and not source.is_symlink() and source.is_file()
                and source.resolve().is_relative_to(ROOT),
                f"Unrecognised source-copy path: {relative}")
    return [path for path in files if not {".git", "target"}.intersection(path.parts)]


def copy_source(files, source, destination):
    for relative in files:
        target = destination / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes((source / relative).read_bytes())


def run_test(root, target_directory, test, log_path, *, package="glaux-standards"):
    command = [
        "cargo", "test", "--locked", "--offline", "-p", package,
        "--lib", test, "--", "--exact", "--nocapture",
    ]
    environment = os.environ.copy()
    environment["CARGO_TARGET_DIR"] = str(target_directory)
    environment["CARGO_TERM_COLOR"] = "never"
    environment["RUST_BACKTRACE"] = "0"
    started = time.monotonic()
    try:
        result = subprocess.run(
            command, cwd=root, env=environment, check=False, text=True,
            stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=TIMEOUT_SECONDS,
        )
        output = result.stdout
        status = {"exit": result.returncode, "timeout": False}
    except subprocess.TimeoutExpired as error:
        output = error.stdout or ""
        if isinstance(output, bytes):
            output = output.decode(errors="replace")
        status = {"exit": None, "timeout": True}
    except OSError as error:
        output = f"Required Cargo runner unavailable: {error}\n"
        status = {"exit": None, "timeout": False}
    log_path.write_text(output)
    status.update({
        "command": command, "timeout_seconds": TIMEOUT_SECONDS,
        "elapsed_seconds": round(time.monotonic() - started, 3), "log": log_path.name,
    })
    return status, output


def passed_exact_test(status, output, test):
    lines = output.splitlines()
    return (status["exit"] == 0 and not status["timeout"]
            and lines.count("running 1 test") == 1
            and lines.count(f"test {test} ... ok") == 1
            and len(re.findall(
                r"^test result: ok\. 1 passed; 0 failed; 0 ignored; 0 measured; \d+ filtered out;",
                output, re.M,
            )) == 1)


def detected_assertion(status, output, test, expected):
    lines = output.splitlines()
    stripped = [line.strip() for line in lines]
    return (status["exit"] == 101 and not status["timeout"]
            and lines.count("running 1 test") == 1
            and lines.count(f"test {test} ... FAILED") == 1
            and f"thread '{test}'" in output
            and "assertion `left == right` failed" in output
            and all(marker in stripped for marker in expected)
            and len(re.findall(
                r"^test result: FAILED\. 0 passed; 1 failed; 0 ignored; 0 measured; \d+ filtered out;",
                output, re.M,
            )) == 1)


def main():
    require(Path.cwd().resolve() == ROOT, "Run from the workspace root.")
    require(not sys.argv[1:], "No test-selection override is accepted.")
    runner_temp = Path(os.environ["RUNNER_TEMP"]).resolve()
    evidence = runner_temp / "glaux-ci-evidence"
    evidence.mkdir(exist_ok=True)
    files = source_files()
    outcomes = []

    def record(row):
        outcomes.append(row)
        (evidence / "validation-failure-controls.json").write_text(json.dumps(outcomes, indent=2))
        print(json.dumps(row), flush=True)

    with tempfile.TemporaryDirectory(prefix="glaux-validation-controls-", dir=runner_temp) as directory:
        owned = Path(directory)
        baseline = owned / "baseline"
        target_directory = owned / "cargo-target"
        copy_source(files, ROOT, baseline)
        # Finish all passing baselines before injecting any behavioral fault.
        for name, test, _, _ in CASES:
            status, output = run_test(
                baseline, target_directory, test, evidence / f"validation-baseline-{name}.log",
            )
            passed = passed_exact_test(status, output, test)
            record({"phase": "baseline", "control": name, "test": test,
                    **status, "passed": passed})
            if not passed:
                print(output, flush=True)
                sys.exit("Unmodified validation assertion did not pass; no fault proof: " + name)
        for name, test, inject, expected in CASES:
            target = owned / name
            copy_source(files, baseline, target)
            inject(target)
            status, output = run_test(
                target, target_directory, test, evidence / f"validation-fault-{name}.log",
            )
            detected = detected_assertion(status, output, test, expected)
            record({"phase": "fault", "control": name, "test": test, **status,
                    "expected_assertion_values": expected, "detected": detected})
            if not detected:
                print(output, flush=True)
                sys.exit("Validation fault escaped or failed for an unrelated reason: " + name)
    print(f"Validation failure controls: {len(CASES)} detected; 0 escaped.", flush=True)


if __name__ == "__main__":
    main()
