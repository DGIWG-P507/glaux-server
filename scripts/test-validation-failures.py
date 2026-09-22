"""Prove four validation assertions detect bounded faults in disposable copies.

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


CASES = [
    ("binary-aggregate-root", "validation::tests::fixed_encoding_selection", wrong_binary_root,
     ["left: Err(Structure)", "right: Ok(())"]),
    ("quantity-structural-bypass", "validation::tests::parser_fuzz_regressions",
     accept_quantity_without_structural_validation, ["left: Ok(())", "right: Err(Structure)"]),
    ("raw-size-limit-bypass", "validation::tests::limits_and_safe_parse", bypass_raw_size_limit,
     ["left: Err(Malformed)", "right: Err(Size)"]),
    ("unallowlisted-reference-bypass", "schema_guard::tests::rejects_http_file_data_and_uri_escape_canaries",
     ignore_unallowlisted_schema_references,
     ["left: Ok(())", 'right: Err("schema reference is outside the embedded catalog")']),
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
    print("Validation failure controls: 4 detected; 0 escaped.", flush=True)


if __name__ == "__main__":
    main()
