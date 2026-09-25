"""Real atomic application transaction in an owned, isolated PostgreSQL target."""

import hashlib
from pathlib import Path
import subprocess
import sys

from database_harness import DisposablePostgis, HarnessError, docker


ROOT = Path(__file__).resolve().parents[1]
GROUPS = (
    "migration-preservation", "accepted-exact-facts",
    "all-write-boundaries-and-commit", "invalid-context-and-owned-transaction",
    "separate-observer-before-commit", "safe-denial-storage-and-failure",
    "serving-permissions-and-audit-retention-boundary",
)
BOUNDARIES = (
    "identity", "system", "first-alias", "second-alias", "parent-guard", "parent",
    "artifact", "revision", "audit", "outgoing", "commit",
)
FINAL = "Required atomic write proof passed: 7 groups."


def require(condition, message):
    if not condition:
        raise HarnessError(message)


def build_proof(source_root, target_directory):
    result = subprocess.run(
        ["cargo", "build", "--locked", "--offline", "-p", "glaux-server", "--example",
         "atomic-write-proof", "--target-dir", str(target_directory)],
        cwd=source_root, capture_output=True, text=True, timeout=180, check=False,
    )
    print(result.stdout + result.stderr, end="", flush=True)
    binary = target_directory / "debug/examples/atomic-write-proof"
    require(result.returncode == 0 and binary.is_file(), "Required atomic write proof did not build")
    return binary


def run_binary(binary):
    with DisposablePostgis() as db:
        db.setup()
        db.validate_target()
        docker("cp", str(binary), db.container_id + ":/tmp/glaux-atomic-write-proof")
        db.validate_target()
        return docker("exec", "--user", "postgres", db.container_id,
                      "/tmp/glaux-atomic-write-proof", timeout=120)


def validate_output(output):
    observed = [line for line in output.splitlines()
                if line.startswith("Atomic write group passed:")]
    require(observed == ["Atomic write group passed: " + group for group in GROUPS],
            "Required atomic write groups missing, duplicated or reordered")
    boundaries = [line for line in output.splitlines()
                  if line.startswith("Atomic rollback boundary passed:")]
    require(boundaries == ["Atomic rollback boundary passed: " + boundary for boundary in BOUNDARIES],
            "Required rollback boundaries missing, duplicated or reordered")
    require(output.splitlines().count(FINAL) == 1, "Atomic write final marker missing/duplicated")


def main():
    require(not sys.argv[1:], "No target or selection override is accepted")
    data = b'{"type":"PhysicalSystem","label":"Alpha","value":1}'
    require(len(data) == 51 and hashlib.sha256(data).hexdigest()
            == "8d3e448241a86daedf90f6ea36eebbc62c82829307bcb6eae1a9954c031ec215",
            "Independent source fixture length/digest mismatch")
    binary = build_proof(ROOT, ROOT / "target")
    output = run_binary(binary)
    print(output, flush=True)
    validate_output(output)
    print("Atomic write: 7 Rust groups and 11 rollback boundaries passed; 0 failed; 0 skipped", flush=True)
    print("Atomic write database: all required checks passed.", flush=True)


if __name__ == "__main__":
    main()
