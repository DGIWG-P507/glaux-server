"""Prove that the restore proof rejects a writable, open or incomplete clone procedure."""
import json
import os
from pathlib import Path
import sys
import tempfile
from database_harness import HarnessError
from test_system_restore import (FINAL, PROCEDURE, build_proof, require, run_binary,
                                 validate_output)

ROOT = Path(__file__).resolve().parents[1]
PASSED = "System restore group passed: "
BACKUP = 'pg_dump --host="$host" --no-password --format=custom --file="$dump" "$source_db"\n'


def without(marker):
    def mutate(text):
        lines = [line for line in text.splitlines(keepends=True) if line.rstrip().endswith(marker)]
        require(len(lines) == 2, "Restore fault target changed: " + marker)
        return "".join(line for line in text.splitlines(keepends=True) if line not in lines)
    return mutate


def dropped_retry_state(text):
    require(text.count(BACKUP) == 1, "Backup fault target changed")
    return text.replace(BACKUP, BACKUP.replace(
        "--format=custom", "--format=custom --exclude-table-data=public.system_create_retry"))


# Each fault must fail its own intended assertion after the earlier groups pass.
CONTROLS = (
    ("writable-clone", without("# CLONE_READ_ONLY"),
     "restored clone accepted a write",
     "restored-workflow-matches-manifest", "clone-refuses-effects-and-outside-access"),
    ("open-clone", without("# CLONE_CONNECT"),
     "outside role connected to the isolated clone",
     "restored-workflow-matches-manifest", "clone-refuses-effects-and-outside-access"),
    ("dropped-retry-state", dropped_retry_state,
     "restored clone differs from the manifest or backup inventory",
     "source-workflow-and-independent-manifest", "guarded-backup-and-isolated-restore"),
)


def main():
    require(Path.cwd().resolve() == ROOT and not sys.argv[1:],
            "Run from workspace root without target or selection overrides")
    runner_temp = Path(os.environ["RUNNER_TEMP"]).resolve()
    evidence = runner_temp / "glaux-ci-evidence"
    evidence.mkdir(exist_ok=True)
    original = PROCEDURE.read_bytes()
    results = []

    def record(phase, output, **facts):
        log = evidence / ("system-restore-" + phase + ".log")
        log.write_text(output)
        row = {"phase": phase, "log": log.name, **facts}
        results.append(row)
        (evidence / "system-restore-failure-controls.json").write_text(json.dumps(results, indent=2))
        print(json.dumps(row), flush=True)

    binaries = build_proof(ROOT, ROOT / "target")
    output = run_binary(*binaries)
    validate_output(output)
    record("baseline", output, passed=True)
    # The faults change only disposable copies of the procedure; no rebuild.
    with tempfile.TemporaryDirectory(prefix="glaux-system-restore-controls-", dir=runner_temp) as directory:
        for name, mutate, message, last_passed, first_failed in CONTROLS:
            faulty = Path(directory) / (name + ".sh")
            faulty.write_text(mutate(original.decode()))
            try:
                output = run_binary(*binaries, procedure=faulty)
            except HarnessError as error:
                output = str(error)
                detected = (
                    "Docker exec failed (101)" in output
                    and message in output
                    and "panicked at" in output
                    and PASSED + last_passed in output
                    and PASSED + first_failed not in output
                    and FINAL not in output
                )
            else:
                detected = False
            record(name, output, detected=detected)
            require(detected, name + " escaped or failed for another reason: " + output)
    require(PROCEDURE.read_bytes() == original, "Real restore procedure was modified")
    print("System restore failure controls: 3 detected; 0 escaped.", flush=True)

if __name__ == "__main__":
    main()
