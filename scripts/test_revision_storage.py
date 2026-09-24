"""Bounded SQLx revision/artifact proof in the existing owned PostGIS harness."""

import hashlib
from pathlib import Path
import subprocess
import sys

from database_harness import DisposablePostgis, HarnessError, docker


ROOT = Path(__file__).resolve().parents[1]
PROOF = ROOT / "target/debug/examples/revision-storage-proof"
GROUPS = (
    "migration-preservation", "exact-artifacts", "exact-times-and-references",
    "append-preserves-history", "atomic-rejection", "immutable-history",
    "checked-reconstruction", "limits-and-reapply",
)
FINAL = "Required revision storage proof passed: 8 groups."
FIXTURES = (
    (b'{"type":"PhysicalSystem","label":"Alpha","value":1}', 51,
     "8d3e448241a86daedf90f6ea36eebbc62c82829307bcb6eae1a9954c031ec215"),
    (b'{\n  "value": 1, "label": "Alpha", "type": "PhysicalSystem"\n}\n', 61,
     "27c281a95454810345d0780509dafb4154b3807ea473658aba7e5f09b41e9268"),
    (b'{"type":"PhysicalSystem","label":"Beta","value":1}', 50,
     "045014d3404cc5fa1f1d0b98c6d3852111a94bfbfa63356e451c8caa076e6c3b"),
    (b"Z" * 1_048_576, 1_048_576,
     "bf63d8a95fcc2e64619813aae35fdcbe871fdd9264caa3f365eb3aed0f679129"),
    (b"", 0, "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"),
)


def require(condition, message):
    if not condition:
        raise HarnessError(message)


def main():
    require(not sys.argv[1:], "No selection or target override is accepted")
    for data, length, expected in FIXTURES:
        require(len(data) == length and hashlib.sha256(data).hexdigest() == expected,
                "Independent source fixture length/digest does not match the authored constant")
    print("Revision fixture controls passed: 5 independent lengths and SHA-256 digests", flush=True)
    build = subprocess.run(
        ["cargo", "build", "--locked", "--offline", "-p", "glaux-server", "--example",
         "revision-storage-proof", "--target-dir", str(ROOT / "target")],
        cwd=ROOT, capture_output=True, text=True, timeout=180, check=False,
    )
    print(build.stdout + build.stderr, end="", flush=True)
    require(build.returncode == 0 and PROOF.is_file(), "Required revision storage proof did not build")
    with DisposablePostgis() as db:
        db.setup()
        db.validate_target()
        docker("cp", str(PROOF), db.container_id + ":/tmp/glaux-revision-storage-proof")
        db.validate_target()
        output = docker("exec", "--user", "postgres", db.container_id,
                        "/tmp/glaux-revision-storage-proof", timeout=120)
        print(output, flush=True)
        observed = [line for line in output.splitlines()
                    if line.startswith("Revision storage group passed:")]
        expected = ["Revision storage group passed: " + name for name in GROUPS]
        require(observed == expected, "Revision groups missing, duplicated or reordered")
        require(output.splitlines().count(FINAL) == 1, "Revision final marker missing/duplicated")
    print("Revision storage: 8 Rust groups passed; 0 failed; 0 skipped", flush=True)
    print("Revision storage database: all required checks passed.", flush=True)


if __name__ == "__main__":
    main()
