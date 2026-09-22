"""Verify the reviewed task #8 package graph and required test discovery.

Expected package edges come from Guide 2.2, not from Cargo's reported graph.
Update this bootstrap-specific inventory when later approved tasks add dependencies
or behavior. This is not a general dependency policy or a conformance runner.
"""

from pathlib import Path

from cargo_inventory import cargo, cargo_inventory, require


root = Path(__file__).resolve().parents[1]
require(Path.cwd().resolve() == root, "Run from the workspace root.")
inventory = cargo_inventory(root)
print(f"Three workspace boundaries and {inventory['third_party_cargo_packages']} reviewed registry packages verified.")

listing = cargo("test", "--workspace", "--locked", "--offline", "--", "--list")
print(listing, end="")
required_tests = [
    "unfinished_server_does_not_report_success",
    "validation::tests::published_corpus_expectations",
    "validation::tests::parser_fuzz_regressions",
    "validation::tests::limits_and_safe_parse",
    "validation::tests::fixed_encoding_selection",
]
for test in required_tests:
    require(listing.splitlines().count(test + ": test") == 1,
            f"Required test not discovered exactly once: {test}")
print("Bootstrap package graph and required test discovery passed.")
