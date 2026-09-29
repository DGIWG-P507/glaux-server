"""Isolated backup and restore of the first System in the owned disposable database."""
from pathlib import Path
import subprocess
import sys
from database_harness import DisposablePostgis, HarnessError, docker
from test_system_create import require

ROOT = Path(__file__).resolve().parents[1]
PROCEDURE = ROOT / "scripts/system-restore.sh"
GROUPS = (
    "source-workflow-and-independent-manifest",
    "guarded-backup-and-isolated-restore",
    "restored-workflow-matches-manifest",
    "clone-refuses-effects-and-outside-access",
    "incomplete-or-corrupt-clones-are-rejected",
    "recovery-point-excludes-later-changes",
)
FINAL = "Required System restore proof passed: 6 groups."

def build_proof(source_root, target_directory):
    result = subprocess.run(
        ["cargo", "build", "--locked", "--offline", "-p", "glaux-server",
         "--bin", "glaux-server", "--example", "system-restore-proof",
         "--target-dir", str(target_directory)],
        cwd=source_root, capture_output=True, text=True, timeout=180, check=False,
    )
    print(result.stdout + result.stderr, end="", flush=True)
    server = target_directory / "debug/glaux-server"
    proof = target_directory / "debug/examples/system-restore-proof"
    require(result.returncode == 0 and server.is_file() and proof.is_file(),
            "Required System restore server/proof did not build")
    return server, proof

def run_binary(server, proof, procedure=PROCEDURE):
    with DisposablePostgis() as db:
        db.setup()
        for source, name in ((server, "glaux-system-restore-server"),
                             (proof, "glaux-system-restore-proof"),
                             (procedure, "glaux-system-restore.sh")):
            db.validate_target()
            docker("cp", str(source), db.container_id + ":/tmp/" + name)
        db.validate_target()
        return docker("exec", "--user", "postgres", db.container_id,
                      "/tmp/glaux-system-restore-proof", timeout=150)

def validate_output(output):
    prefix = "System restore group passed: "
    actual = [line for line in output.splitlines() if line.startswith(prefix)]
    require(actual == [prefix + name for name in GROUPS],
            "Required System restore groups missing, duplicated or reordered")
    require(output.splitlines().count(FINAL) == 1,
            "System restore final marker missing/duplicated")

def main():
    require(Path.cwd().resolve() == ROOT and not sys.argv[1:],
            "Run from workspace root without target or selection overrides")
    output = run_binary(*build_proof(ROOT, ROOT / "target"))
    print(output, flush=True)
    validate_output(output)
    print("System restore: 6 groups passed; 0 failed; 0 skipped", flush=True)
    print("System restore: all required checks passed.", flush=True)

if __name__ == "__main__":
    main()
