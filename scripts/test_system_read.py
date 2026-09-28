"""Canonical System GET through the actual binary, a restart and the owned database."""
from pathlib import Path
import os
import subprocess
import sys
import tempfile
from database_harness import DisposablePostgis, HarnessError, docker
from test_system_create import signed_fixture

ROOT = Path(__file__).resolve().parents[1]
GROUPS = (
    "independent-wire-oracle-controls",
    "root-navigation-create-and-exact-retrieval",
    "restart-retains-identity-and-meaning",
    "missing-and-concealed-are-indistinguishable",
    "verified-token-callers-and-unauthenticated-requests",
    "disabled-creation-removes-retrieval",
)
FINAL = "Required System read proof passed: 6 groups."

def require(condition, message):
    if not condition:
        raise HarnessError(message)

def build_proof(source_root, target_directory):
    result = subprocess.run(
        ["cargo", "build", "--locked", "--offline", "-p", "glaux-server",
         "--bin", "glaux-server", "--example", "system-read-proof",
         "--target-dir", str(target_directory)],
        cwd=source_root, capture_output=True, text=True, timeout=180, check=False,
    )
    print(result.stdout + result.stderr, end="", flush=True)
    server = target_directory / "debug/glaux-server"
    proof = target_directory / "debug/examples/system-read-proof"
    require(result.returncode == 0 and server.is_file() and proof.is_file(),
            "Required System read server/proof did not build")
    return server, proof

def run_binary(server, proof):
    with tempfile.TemporaryDirectory(prefix="glaux-read-jwt-", dir=os.environ["RUNNER_TEMP"]) as directory, DisposablePostgis() as db:
        fixture = signed_fixture(Path(directory))
        db.setup()
        for binary, name in ((server, "glaux-system-read-server"),
                             (proof, "glaux-system-read-proof")):
            db.validate_target()
            docker("cp", str(binary), db.container_id + ":/tmp/" + name)
        db.validate_target()
        docker("cp", str(fixture), db.container_id + ":/tmp/glaux-system-read-fixtures.json")
        db.validate_target()
        return docker("exec", "--user", "postgres", db.container_id,
                      "/tmp/glaux-system-read-proof", timeout=120)

def validate_output(output):
    prefix = "System read group passed: "
    actual = [line for line in output.splitlines() if line.startswith(prefix)]
    require(actual == [prefix + name for name in GROUPS],
            "Required System read groups missing, duplicated or reordered")
    require(output.splitlines().count(FINAL) == 1,
            "System read final marker missing/duplicated")

def main():
    require(Path.cwd().resolve() == ROOT and not sys.argv[1:],
            "Run from workspace root without target or selection overrides")
    output = run_binary(*build_proof(ROOT, ROOT / "target"))
    print(output, flush=True)
    validate_output(output)
    print("System read: 6 groups passed; 0 failed; 0 skipped", flush=True)
    print("System read: all required checks passed.", flush=True)

if __name__ == "__main__":
    main()
