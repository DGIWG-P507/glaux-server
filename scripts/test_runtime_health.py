"""Actual CLI, independent HTTP and storage health in the owned isolated cluster."""

from pathlib import Path
import subprocess
import sys

from database_harness import DisposablePostgis, HarnessError, docker


ROOT = Path(__file__).resolve().parents[1]
GROUPS = (
    "typed-config-and-loopback-matrix",
    "secret-references-and-safe-diagnostics",
    "independent-wire-oracle-controls",
    "explicit-schema-startup-and-verified-network-route",
    "actual-listener-minimal-health-only",
    "isolated-storage-outage-and-recovery",
    "schema-drift-rejection-without-repair",
    "bounded-shutdown-and-sentinel-preservation",
)
FINAL = "Required runtime health proof passed: 8 groups."


def require(condition, message):
    if not condition:
        raise HarnessError(message)


def build_proof(source_root, target_directory):
    result = subprocess.run(
        ["cargo", "build", "--locked", "--offline", "-p", "glaux-server",
         "--bin", "glaux-server", "--example", "runtime-health-proof",
         "--target-dir", str(target_directory)],
        cwd=source_root, capture_output=True, text=True, timeout=180, check=False,
    )
    print(result.stdout + result.stderr, end="", flush=True)
    server = target_directory / "debug/glaux-server"
    proof = target_directory / "debug/examples/runtime-health-proof"
    require(result.returncode == 0 and server.is_file() and proof.is_file(),
            "Required runtime health server/proof did not build")
    return server, proof


def run_binary(server, proof):
    with DisposablePostgis() as db:
        db.setup()
        for binary, name in ((server, "glaux-runtime-health-server"),
                             (proof, "glaux-runtime-health-proof")):
            db.validate_target()
            docker("cp", str(binary), db.container_id + ":/tmp/" + name)
        db.validate_target()
        return docker("exec", "--user", "postgres", db.container_id,
                      "/tmp/glaux-runtime-health-proof", timeout=120)


def validate_output(output):
    prefix = "Runtime health group passed: "
    actual = [line for line in output.splitlines() if line.startswith(prefix)]
    require(actual == [prefix + name for name in GROUPS],
            "Required runtime health groups missing, duplicated or reordered")
    require(output.splitlines().count(FINAL) == 1,
            "Runtime health final marker missing/duplicated")


def main():
    require(not sys.argv[1:], "No target or selection override is accepted")
    output = run_binary(*build_proof(ROOT, ROOT / "target"))
    print(output, flush=True)
    validate_output(output)
    print("Runtime health: 8 groups passed; 0 failed; 0 skipped", flush=True)
    print("Runtime health: all required checks passed.", flush=True)


if __name__ == "__main__":
    main()
