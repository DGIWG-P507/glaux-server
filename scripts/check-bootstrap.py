"""Verify the reviewed package graph and required test discovery.

Expected package edges come from Guide 2.2, not from Cargo's reported graph.
Update this bootstrap-specific inventory when later approved tasks add dependencies
or behavior. This is not a general dependency policy or a conformance runner.
"""

from pathlib import Path

from cargo_inventory import cargo, cargo_inventory, require
from required_tests import REQUIRED_RUST_TESTS


root = Path(__file__).resolve().parents[1]
require(Path.cwd().resolve() == root, "Run from the workspace root.")
inventory = cargo_inventory(root)
print(f"Three workspace boundaries and {inventory['third_party_cargo_packages']} reviewed registry packages verified.")

listing = cargo("test", "--workspace", "--locked", "--offline", "--", "--list")
print(listing, end="")
for test in REQUIRED_RUST_TESTS:
    require(listing.splitlines().count(test + ": test") == 1,
            f"Required test not discovered exactly once: {test}")
print("Bootstrap package graph and required test discovery passed.")
