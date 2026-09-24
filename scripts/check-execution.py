"""Run a required suite, preserving failure and rejecting missing pass evidence."""

import subprocess
import sys


def main():
    if sys.argv[1:] == ["rust"]:
        command = [
            "cargo", "test", "--workspace", "--locked", "--offline",
            "--", "--nocapture",
        ]
        from required_tests import REQUIRED_RUST_TESTS
        markers = ["test " + name + " ... ok" for name in REQUIRED_RUST_TESTS]
        missing = "Required Rust test did not execute successfully"
    elif sys.argv[1:] == ["database"]:
        command = [sys.executable, "-u", "scripts/test_database.py"]
        markers = ["Database lifecycle: 7 passed; 0 failed; 0 skipped"]
        missing = "Required database suite did not execute successfully"
    elif sys.argv[1:] == ["schema-fuzz"]:
        command = ["cargo", "run", "--locked", "--offline", "-p", "glaux-standards", "--example", "schema-parser-fuzz"]
        markers = ["Required schema-parser fuzz invariants passed: 1024 cases."]
        missing = "Required schema-parser fuzz campaign did not execute successfully"
    elif sys.argv[1:] == ["numeric-fuzz"]:
        command = ["cargo", "run", "--locked", "--offline", "-p", "glaux-domain", "--example", "numeric-parser-fuzz"]
        markers = ["Required numeric-parser fuzz invariants passed: 2048 cases."]
        missing = "Required numeric-parser fuzz campaign did not execute successfully"
    elif sys.argv[1:] == ["time-fuzz"]:
        command = ["cargo", "run", "--locked", "--offline", "-p", "glaux-domain", "--example", "time-parser-fuzz"]
        markers = ["Required time-parser fuzz invariants passed: 2048 cases."]
        missing = "Required time-parser fuzz campaign did not execute successfully"
    elif sys.argv[1:] == ["time-database"]:
        command = [sys.executable, "-u", "scripts/test_time_database.py"]
        markers = ["Exact-time database: 5 passed; 0 failed; 0 skipped"]
        missing = "Required exact-time database proof did not execute successfully"
    elif sys.argv[1:] == ["system-storage"]:
        command = [sys.executable, "-u", "scripts/test_system_storage.py"]
        markers = ["System storage database: all required checks passed."]
        missing = "Required System storage database proof did not execute successfully"
    else:
        sys.exit("Specify exactly rust, database, schema-fuzz, numeric-fuzz, time-fuzz, time-database or system-storage; no test-selection override.")
    print("Required command: " + " ".join(command), flush=True)
    try:
        result = subprocess.run(
            command, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
            text=True, timeout=180, check=False,
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        sys.exit(f"Required runner unavailable/timeout: {error}")
    print(result.stdout, end="", flush=True)
    if result.returncode:
        sys.exit(f"Required command failed with exit {result.returncode}")
    if any(result.stdout.splitlines().count(marker) != 1 for marker in markers):
        sys.exit(missing)
    print("Required " + sys.argv[1] + " execution verified.", flush=True)


if __name__ == "__main__":
    main()
