"""Verify only issue #4's initial package graph and required test discovery.

Expected package edges come from Guide 2.2, not from Cargo's reported graph.
Update this bootstrap-specific inventory when later approved tasks add dependencies
or behavior. This is not a general dependency policy or a conformance runner.
"""

import json
import subprocess
import sys
import tomllib
from pathlib import Path


def require(condition, message):
    if not condition:
        raise SystemExit(message)


def cargo(*args):
    result = subprocess.run(
        ["cargo", *args], check=False, text=True, capture_output=True
    )
    # Preserve Cargo diagnostics as well as the parsed output.
    print(result.stderr, end="", file=sys.stderr)
    if result.returncode:
        print(result.stdout, end="")
        result.check_returncode()
    return result.stdout


root = Path(__file__).resolve().parents[1]
require(Path.cwd().resolve() == root, "Run from the workspace root.")
toolchain = tomllib.loads((root / "rust-toolchain.toml").read_text())["toolchain"]["channel"]
metadata = json.loads(cargo("metadata", "--format-version=1", "--locked", "--offline"))
expected = {
    "glaux-domain": set(),
    "glaux-standards": {"glaux-domain"},
    "glaux-server": {"glaux-domain", "glaux-standards"},
}
packages = {package["id"]: package for package in metadata["packages"]}
require(len(packages) == 3, "Bootstrap must contain exactly three local packages.")
require(set(packages) == set(metadata["workspace_members"]), "Unexpected external package.")
require(
    {package["name"] for package in packages.values()} == set(expected),
    "Unexpected production package set.",
)
nodes = {node["id"]: node for node in metadata["resolve"]["nodes"]}
require(set(nodes) == set(packages), "Incomplete or unexpected resolved graph.")
for package_id, package in packages.items():
    name = package["name"]
    require(package["source"] is None, f"{name}: expected a local workspace package.")
    require(package["license"] == "Apache-2.0", f"{name}: incorrect original-code licence.")
    require(package["edition"] == "2024", f"{name}: unexpected edition.")
    require(package["rust_version"] == toolchain, f"{name}: toolchain metadata drift.")
    require(package["publish"] == [], f"{name}: premature registry publication enabled.")
    actual = {packages[edge["pkg"]]["name"] for edge in nodes[package_id]["deps"]}
    require(actual == expected[name], f"{name}: wrong dependency edges: {sorted(actual)}")
    print(f"{name} -> {sorted(actual)}; {package['version']}; {package['license']}")

listing = cargo("test", "--workspace", "--locked", "--offline", "--", "--list")
print(listing, end="")
required_test = "unfinished_server_does_not_report_success: test"
require(listing.splitlines().count(required_test) == 1, "Required bootstrap test not discovered once.")
print("Bootstrap package graph and required test discovery passed.")
