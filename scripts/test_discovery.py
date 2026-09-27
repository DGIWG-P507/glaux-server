"""Actual binary, root-led raw HTTP and local assets in the owned offline cluster."""

import hashlib
import json
from pathlib import Path
import subprocess
import sys

from database_harness import DisposablePostgis, HarnessError, docker


ROOT = Path(__file__).resolve().parents[1]
GROUPS = (
    "independent-wire-oracle-controls",
    "root-navigation-and-honest-declarations",
    "actual-method-media-and-origin-contract",
    "offline-schema-examples-and-local-assets",
    "actual-prefix-proxy-and-forged-headers",
    "disabled-capabilities-clean-shutdown-and-unchanged-data",
)
FINAL = "Required discovery proof passed: 6 groups."
ASSETS = {
    "swagger-ui-bundle.js", "swagger-ui.css", "LICENSE", "NOTICE",
    "swagger-ui-bundle.js.LICENSE.txt",
}


def require(condition, message):
    if not condition:
        raise HarnessError(message)


def verify_assets(source_root):
    directory = source_root / "crates/glaux-server/assets/swagger-ui"
    manifest = json.loads((directory / "manifest.json").read_text())
    require(manifest["name"] == "Swagger UI" and manifest["version"] == "5.33.0"
            and manifest["commit"] == "cfd4a6c3cbaeeb7c13a8bada7c754de42d78cd5b",
            "Reviewed renderer pin changed")
    entries = manifest["files"]
    require(len(entries) == len(ASSETS) and {row["path"] for row in entries} == ASSETS,
            "Renderer manifest omits, duplicates or introduces an asset")
    for row in entries:
        payload = (directory / row["path"]).read_bytes()
        require(len(payload) == row["bytes"] and hashlib.sha256(payload).hexdigest() == row["sha256"],
                "Pinned renderer asset integrity failed: " + row["path"])
    print("Discovery assets: 5 exact pinned files verified offline.", flush=True)


def build_proof(source_root, target_directory):
    verify_assets(source_root)
    result = subprocess.run(
        ["cargo", "build", "--locked", "--offline", "-p", "glaux-server",
         "--bin", "glaux-server", "--example", "discovery-proof",
         "--target-dir", str(target_directory)],
        cwd=source_root, capture_output=True, text=True, timeout=180, check=False,
    )
    print(result.stdout + result.stderr, end="", flush=True)
    server = target_directory / "debug/glaux-server"
    proof = target_directory / "debug/examples/discovery-proof"
    require(result.returncode == 0 and server.is_file() and proof.is_file(),
            "Required discovery server/proof did not build")
    return server, proof


def run_binary(server, proof):
    with DisposablePostgis() as db:
        db.setup()
        for binary, name in ((server, "glaux-discovery-server"),
                             (proof, "glaux-discovery-proof")):
            db.validate_target()
            docker("cp", str(binary), db.container_id + ":/tmp/" + name)
        db.validate_target()
        return docker("exec", "--user", "postgres", db.container_id,
                      "/tmp/glaux-discovery-proof", timeout=120)


def validate_output(output):
    prefix = "Discovery group passed: "
    actual = [line for line in output.splitlines() if line.startswith(prefix)]
    require(actual == [prefix + name for name in GROUPS],
            "Required discovery groups missing, duplicated or reordered")
    require(output.splitlines().count(FINAL) == 1,
            "Discovery final marker missing/duplicated")


def main():
    require(not sys.argv[1:], "No target or selection override is accepted")
    output = run_binary(*build_proof(ROOT, ROOT / "target"))
    print(output, flush=True)
    validate_output(output)
    print("Discovery: 6 groups passed; 0 failed; 0 skipped", flush=True)
    print("Discovery: all required checks passed.", flush=True)


if __name__ == "__main__":
    main()
