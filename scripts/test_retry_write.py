"""Scoped retry receipts proved inside the owned, isolated real database."""

from pathlib import Path
import subprocess
import sys

from database_harness import DisposablePostgis, HarnessError, docker


ROOT = Path(__file__).resolve().parents[1]
GROUPS = (
    "original-outcome-generated-ids-lost-response",
    "every-content-field-conflicts-without-effect",
    "caller-source-target-operation-and-reauthorization",
    "expiry-database-clock-optional-key-input-bounds",
    "receipt-and-commit-rollback-and-exact-bindings",
    "both-orders-identical-and-conflicting-concurrency",
    "bounded-generated-retry-state-sequences",
    "restricted-serving-role",
)
BOUNDARIES = ("receipt", "commit")
RACES = ("first-1-identical", "first-2-identical", "first-1-conflicting", "first-2-conflicting",
         "different-actor", "different-source")
FINAL = "Required retry write proof passed: 8 groups."


def require(condition, message):
    if not condition:
        raise HarnessError(message)


def build_proof(source_root, target_directory):
    result = subprocess.run(
        ["cargo", "build", "--locked", "--offline", "-p", "glaux-server", "--example",
         "retry-write-proof", "--target-dir", str(target_directory)],
        cwd=source_root, capture_output=True, text=True, timeout=180, check=False,
    )
    print(result.stdout + result.stderr, end="", flush=True)
    binary = target_directory / "debug/examples/retry-write-proof"
    require(result.returncode == 0 and binary.is_file(), "Required retry write proof did not build")
    return binary


def run_binary(binary):
    with DisposablePostgis() as db:
        db.setup()
        db.validate_target()
        docker("cp", str(binary), db.container_id + ":/tmp/glaux-retry-write-proof")
        db.validate_target()
        return docker("exec", "--user", "postgres", db.container_id,
                      "/tmp/glaux-retry-write-proof", timeout=120)


def validate_output(output):
    for prefix, names in (("Retry write group passed: ", GROUPS),
                          ("Retry rollback boundary passed: ", BOUNDARIES),
                          ("Retry race passed: ", RACES)):
        actual = [line for line in output.splitlines() if line.startswith(prefix)]
        require(actual == [prefix + name for name in names],
                "Required retry proof evidence missing, duplicated or reordered: " + prefix)
    model = [line for line in output.splitlines()
             if line.startswith("Retry model passed: v1 seeds 17,91,314; 72 transitions; partitions ")]
    require(len(model) == 1, "Bounded retry model evidence missing/duplicated")
    require(output.splitlines().count(FINAL) == 1, "Retry final marker missing/duplicated")


def main():
    require(not sys.argv[1:], "No target or selection override is accepted")
    output = run_binary(build_proof(ROOT, ROOT / "target"))
    print(output, flush=True)
    validate_output(output)
    print("Retry write: 8 groups, 2 rollback boundaries, 6 races and 72 generated transitions passed; 0 failed; 0 skipped", flush=True)
    print("Retry write database: all required checks passed.", flush=True)


if __name__ == "__main__":
    main()
