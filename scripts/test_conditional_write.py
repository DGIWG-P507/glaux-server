"""Real conditional application writes inside an owned isolated database."""

from pathlib import Path
import subprocess
import sys

from database_harness import DisposablePostgis, HarnessError, docker


ROOT = Path(__file__).resolve().parents[1]
GROUPS = (
    "migration-authoritative-head-and-ambiguity",
    "matching-stale-unconditional-and-exact-history",
    "missing-uninitialized-invalid-and-owned-transaction",
    "all-update-boundaries-and-commit",
    "both-conditional-orders-and-unconditional-race",
    "restricted-serving-role",
)
BOUNDARIES = ("label", "artifact", "revision", "audit", "outgoing", "head", "commit")
RACES = ("first-2-conditional", "first-3-conditional", "first-2-unconditional")
FINAL = "Required conditional write proof passed: 6 groups."


def require(condition, message):
    if not condition:
        raise HarnessError(message)


def build_proof(source_root, target_directory):
    result = subprocess.run(
        ["cargo", "build", "--locked", "--offline", "-p", "glaux-server", "--example",
         "conditional-write-proof", "--target-dir", str(target_directory)],
        cwd=source_root, capture_output=True, text=True, timeout=180, check=False,
    )
    print(result.stdout + result.stderr, end="", flush=True)
    binary = target_directory / "debug/examples/conditional-write-proof"
    require(result.returncode == 0 and binary.is_file(), "Required conditional write proof did not build")
    return binary


def run_binary(binary):
    with DisposablePostgis() as db:
        db.setup()
        db.validate_target()
        docker("cp", str(binary), db.container_id + ":/tmp/glaux-conditional-write-proof")
        db.validate_target()
        return docker("exec", "--user", "postgres", db.container_id,
                      "/tmp/glaux-conditional-write-proof", timeout=120)


def validate_output(output):
    for prefix, names in (("Conditional write group passed: ", GROUPS),
                          ("Conditional rollback boundary passed: ", BOUNDARIES),
                          ("Conditional race passed: ", RACES)):
        actual = [line for line in output.splitlines() if line.startswith(prefix)]
        require(actual == [prefix + name for name in names],
                "Required conditional proof evidence missing, duplicated or reordered: " + prefix)
    require(output.splitlines().count(FINAL) == 1, "Conditional final marker missing/duplicated")


def main():
    require(not sys.argv[1:], "No target or selection override is accepted")
    output = run_binary(build_proof(ROOT, ROOT / "target"))
    print(output, flush=True)
    validate_output(output)
    print("Conditional write: 6 groups, 7 rollback boundaries and 3 races passed; 0 failed; 0 skipped", flush=True)
    print("Conditional write database: all required checks passed.", flush=True)


if __name__ == "__main__":
    main()
