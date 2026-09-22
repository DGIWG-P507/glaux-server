"""Bounded regression controls for required execution, in disposable source copies.

The normal workflow must pass first. Each control invokes the same checked-in
suite entry point and inspects why it failed; compiler/setup failures are not
accepted as behavioral assertion detection. Faults never change the real checkout.
"""

import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]
EVIDENCE = Path(os.environ["RUNNER_TEMP"]) / "glaux-ci-evidence"
EVIDENCE.mkdir(exist_ok=True)


def replace(path, old, new):
    text = path.read_text()
    if text.count(old) != 1:
        raise AssertionError(f"Fault target changed: {path.name}")
    path.write_text(text.replace(old, new))


def assertion(root):
    replace(root / "crates/glaux-server/src/main.rs", "ExitCode::from(2)", "ExitCode::from(0)")


def missing_image(root):
    path = root / "scripts/database-image.json"
    pin = json.loads(path.read_text())
    pin["image"] = "postgis/postgis@sha256:" + "0" * 64
    path.write_text(json.dumps(pin))


def setup_failure(root):
    (root / "crates/glaux-server/migrations/0001_enable_postgis.sql").write_text("SELECT 1/0;")


def runner_error(root):
    (root / "scripts/test_database.py").unlink()


def empty_rust(root):
    (root / "crates/glaux-server/tests/bootstrap.rs").write_text("// Empty control target.\n")


def filtered_rust(root):
    replace(root / "scripts/check-execution.py", '"--", "--nocapture",',
            '"--", "--nocapture", "--skip", "unfinished_server_does_not_report_success",')


def ignored_rust(root):
    replace(root / "crates/glaux-server/tests/bootstrap.rs", "#[test]", "#[test]\n#[ignore]")


def skipped_database(root):
    replace(root / "scripts/test_database.py",
            "    def test_identity_migration_and_exact_seed(self):",
            '    @unittest.skip("disposable control")\n    def test_identity_migration_and_exact_seed(self):')


def empty_database(root):
    replace(root / "scripts/test_database.py",
            "unittest.defaultTestLoader.loadTestsFromTestCase(DatabaseLifecycleTests)",
            "unittest.TestSuite()")


CASES = [
    ("assertion", "rust", assertion, ["test result: FAILED.", "assertion"]),
    ("missing-image", "database", missing_image, ["No such image", "FAILED (errors="]),
    ("setup-failure", "database", setup_failure, ["division by zero", "Setup failed"]),
    ("runner-error", "database", runner_error, ["can't open file", "Required command failed"]),
    ("empty-rust", "rust", empty_rust, ["0 tests", "Required Rust test did not execute successfully"]),
    ("filtered-rust", "rust", filtered_rust, ["1 filtered out", "Required Rust test did not execute successfully"]),
    ("ignored-rust", "rust", ignored_rust, ["1 ignored", "Required Rust test did not execute successfully"]),
    ("skipped-database", "database", skipped_database, ["skipped=1", "Database lifecycle did not execute"]),
    ("empty-database", "database", empty_database, ["discovery mismatch: expected 7, got 0"]),
]


def main():
    listing = subprocess.run(["git", "ls-files", "-z"], cwd=ROOT, check=True,
                             stdout=subprocess.PIPE).stdout.decode().split("\0")
    files = [Path(path) for path in listing if path]
    for relative in files:
        if relative.is_absolute() or ".." in relative.parts or (ROOT / relative).is_symlink():
            sys.exit("Unrecognised source-copy path; refusing to stage controls")
    outcomes = []
    # No user path is deleted: TemporaryDirectory owns a fresh task-specific tree.
    for name, suite, inject, expected in CASES:
        with tempfile.TemporaryDirectory(prefix="glaux-ci-control-", dir=os.environ["RUNNER_TEMP"]) as directory:
            target = Path(directory)
            for relative in files:
                destination = target / relative
                destination.parent.mkdir(parents=True, exist_ok=True)
                destination.write_bytes((ROOT / relative).read_bytes())
            inject(target)
            result = subprocess.run(
                [sys.executable, "scripts/check-execution.py", suite], cwd=target,
                text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                timeout=210, check=False,
            )
            (EVIDENCE / (name + ".log")).write_text(result.stdout)
            detected = result.returncode != 0 and all(text in result.stdout for text in expected)
            outcomes.append({"control": name, "suite": suite, "exit": result.returncode,
                             "expected_diagnostics": expected, "detected": detected})
            print(json.dumps(outcomes[-1]), flush=True)
            if not detected:
                print(result.stdout, flush=True)
                sys.exit("Failure control escaped or failed for an unrelated reason: " + name)
    (EVIDENCE / "failure-controls.json").write_text(json.dumps(outcomes, indent=2))
    print("CI failure controls: 9 detected; 0 escaped.", flush=True)


if __name__ == "__main__":
    main()
