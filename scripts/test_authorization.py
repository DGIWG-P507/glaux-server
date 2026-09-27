"""Independent HTTP and real SQLx permission proof in an owned PostGIS target."""

from pathlib import Path
import subprocess
import sys

from database_harness import DisposablePostgis, HarnessError, docker


ROOT = Path(__file__).resolve().parents[1]
GROUPS = (
    "independent-wire-oracle-controls", "exact-authorized-queries",
    "accepted-write-and-source-boundary", "action-resource-and-asserted-authority",
    "policy-unavailable-and-revocation", "bounded-denial-audit",
    "audit-storage-failure-and-busy", "restored-policy-and-clean-shutdown",
)
FINAL = "Required authorization proof passed: 8 groups."


def require(condition, message):
    if not condition:
        raise HarnessError(message)


def build_proof(source_root, target_directory):
    result = subprocess.run(
        ["cargo", "build", "--locked", "--offline", "-p", "glaux-server", "--example",
         "authorization-proof", "--target-dir", str(target_directory)],
        cwd=source_root, capture_output=True, text=True, timeout=180, check=False,
    )
    print(result.stdout + result.stderr, end="", flush=True)
    binary = target_directory / "debug/examples/authorization-proof"
    require(result.returncode == 0 and binary.is_file(), "Required authorization proof did not build")
    return binary


def run_binary(binary):
    with DisposablePostgis() as db:
        db.setup()
        db.validate_target()
        docker("cp", str(binary), db.container_id + ":/tmp/glaux-authorization-proof")
        db.validate_target()
        return docker("exec", "--user", "postgres", db.container_id,
                      "/tmp/glaux-authorization-proof", timeout=120)


def validate_output(output):
    observed = [line for line in output.splitlines()
                if line.startswith("Authorization group passed:")]
    require(observed == ["Authorization group passed: " + group for group in GROUPS],
            "Required authorization groups missing, duplicated or reordered")
    require(output.splitlines().count(FINAL) == 1,
            "Authorization final marker missing/duplicated")


def main():
    require(Path.cwd().resolve() == ROOT and not sys.argv[1:],
            "Run from workspace root without target or selection overrides")
    output = run_binary(build_proof(ROOT, ROOT / "target"))
    print(output, flush=True)
    validate_output(output)
    print("Authorization: 8 required HTTP/database groups passed; 0 failed; 0 skipped", flush=True)
    print("Authorization: all required checks passed.", flush=True)


if __name__ == "__main__":
    main()
