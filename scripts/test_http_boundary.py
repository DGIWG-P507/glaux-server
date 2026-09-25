"""Independent HTTP bytes against an owned loopback listener; no database target."""

from pathlib import Path
import subprocess
import sys


ROOT = Path(__file__).resolve().parents[1]
GROUPS = (
    "independent-wire-oracle-controls",
    "media-and-json-contracts",
    "safe-problems-methods-and-head",
    "bounded-bodies-headers-paths-and-timeouts",
    "configured-origin-link-isolation",
    "generated-accept-and-path-cases",
    "no-extra-routes-and-clean-shutdown",
)
FINAL = "Required HTTP boundary proof passed: 7 groups."


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def build_proof(source_root, target_directory):
    result = subprocess.run(
        ["cargo", "build", "--locked", "--offline", "-p", "glaux-server",
         "--example", "http-boundary-proof", "--target-dir", str(target_directory)],
        cwd=source_root, capture_output=True, text=True, timeout=180, check=False,
    )
    print(result.stdout + result.stderr, end="", flush=True)
    proof = target_directory / "debug/examples/http-boundary-proof"
    require(result.returncode == 0 and proof.is_file(),
            "Required HTTP boundary proof did not build")
    return proof


def run_binary(proof):
    return subprocess.run(
        [str(proof)], capture_output=True, text=True, timeout=40, check=False,
    )


def validate_output(output):
    prefix = "HTTP boundary group passed: "
    actual = [line for line in output.splitlines() if line.startswith(prefix)]
    require(actual == [prefix + name for name in GROUPS],
            "Required HTTP boundary groups missing, duplicated or reordered")
    require(output.splitlines().count(FINAL) == 1,
            "HTTP boundary final marker missing/duplicated")


def main():
    require(Path.cwd().resolve() == ROOT and not sys.argv[1:],
            "Run from the workspace root without target or selection overrides")
    result = run_binary(build_proof(ROOT, ROOT / "target"))
    output = result.stdout + result.stderr
    print(output, end="", flush=True)
    require(result.returncode == 0, "Required HTTP boundary proof failed")
    validate_output(output)
    print("HTTP boundary: 7 groups passed; 0 failed; 0 skipped", flush=True)
    print("HTTP boundary: all required checks passed.", flush=True)


if __name__ == "__main__":
    main()
