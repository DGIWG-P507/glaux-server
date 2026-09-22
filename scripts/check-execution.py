"""Run a required suite, preserving failure and rejecting missing pass evidence."""

import subprocess
import sys


def main():
    if sys.argv[1:] == ["rust"]:
        command = [
            "cargo", "test", "--workspace", "--locked", "--offline",
            "--", "--nocapture",
        ]
        marker = "test unfinished_server_does_not_report_success ... ok"
        missing = "Required Rust test did not execute successfully"
    elif sys.argv[1:] == ["database"]:
        command = [sys.executable, "-u", "scripts/test_database.py"]
        marker = "Database lifecycle: 7 passed; 0 failed; 0 skipped"
        missing = "Required database suite did not execute successfully"
    else:
        sys.exit("Specify exactly rust or database; no test-selection override.")
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
    if result.stdout.splitlines().count(marker) != 1:
        sys.exit(missing)
    print("Required " + sys.argv[1] + " execution verified.", flush=True)


if __name__ == "__main__":
    main()
