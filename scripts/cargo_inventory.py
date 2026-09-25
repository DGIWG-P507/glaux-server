"""Account for the approved locked Cargo graph and enforce its reviewed snapshot.

The explicit --candidate mode only emits evidence on the approved hosted runner.
It does not approve or write a snapshot. Ordinary callers require the reviewed,
committed docs/cargo-dependencies.json to match every recorded field.
"""

import hashlib
import json
from pathlib import Path
import re
import subprocess
import sys
import tomllib


ROOT = Path(__file__).resolve().parents[1]
SNAPSHOT = Path("docs/cargo-dependencies.json")
REGISTRY = "registry+https://github.com/rust-lang/crates.io-index"
WORKSPACE_EDGES = {
    "glaux-domain": set(),
    "glaux-standards": {"glaux-domain"},
    "glaux-server": {"glaux-domain", "glaux-standards"},
}
# Task-owned selections; the resolved graph is separately reviewed as a snapshot.
DIRECT_DEPENDENCIES = {
    "glaux-server": {
        "axum": {"version": "=0.8.8", "default_features": False,
                 "features": ["http1", "tokio"]},
        "serde": {"version": "=1.0.229", "default_features": False,
                  "features": ["derive", "std"]},
        "serde_json": {"version": "=1.0.151", "default_features": True,
                       "features": ["arbitrary_precision", "raw_value"]},
        "sqlx": {"version": "=0.9.0", "default_features": False,
                 "features": ["migrate", "postgres", "runtime-tokio", "tls-rustls-ring-webpki"]},
        "tokio": {"version": "=1.53.1", "default_features": False,
                  "features": ["macros", "net", "rt", "signal", "sync", "time"]},
    },
    "glaux-standards": {
        "jsonschema": {"version": "=0.56.0", "default_features": False, "features": []},
        "serde_json": {"version": "=1.0.151", "default_features": True,
                       "features": ["arbitrary_precision", "raw_value"]},
    },
    "glaux-domain": {
        "uuid": {"version": "=1.26.1", "default_features": False, "features": []},
        "getrandom": {"version": "=0.4.3", "default_features": False, "features": []},
        "fluent-uri": {"version": "=0.4.1", "default_features": False, "features": []},
        "num-bigint": {"version": "=0.4.8", "default_features": False, "features": []},
        "num-rational": {"version": "=0.4.2", "default_features": False, "features": ["num-bigint"]},
        "num-traits": {"version": "=0.2.19", "default_features": False, "features": []},
    },
}
NETWORK_CLIENTS = {
    "attohttpc", "awc", "curl", "curl-sys", "isahc",
    "minreq", "reqwest", "surf", "ureq",
}
NOTICE_NAME = re.compile(r"^(?:licen[cs]e|copying|notice|copyright)(?:$|[._-])", re.I)


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def cargo(*args, root=ROOT):
    result = subprocess.run(
        ["cargo", *args], cwd=root, check=False, text=True,
        capture_output=True, timeout=180,
    )
    print(result.stderr, end="", file=sys.stderr)
    if result.returncode:
        print(result.stdout, end="", file=sys.stderr)
        result.check_returncode()
    return result.stdout


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def identity(package):
    return (package["name"], package["version"], package.get("source") or "local workspace")


def label(package):
    name, version, source = identity(package)
    return f"{name} {version} ({source})"


def sorted_records(records):
    return sorted(records, key=lambda item: json.dumps(item, sort_keys=True))


def notice_files(package):
    """Hash actual packaged notices; absent files never imply licence approval."""
    directory = Path(package["manifest_path"]).resolve().parent
    selected = set()
    for path in directory.rglob("*"):
        relative = path.relative_to(directory)
        if path.is_file() and (
            NOTICE_NAME.match(path.name)
            or any(part.lower() == "licenses" for part in relative.parts[:-1])
        ):
            selected.add(path)
    declared_file = package.get("license_file")
    if declared_file:
        path = directory / declared_file
        require(path.is_file(), f"{label(package)}: declared licence file is missing.")
        selected.add(path)
    result = []
    for path in sorted(selected):
        require(path.resolve().is_relative_to(directory),
                f"{label(package)}: licence/notice file escapes its package.")
        result.append({"path": path.relative_to(directory).as_posix(), "sha256": sha256(path)})
    return result


def check_workspace(package, node, packages, root, toolchain):
    name = package["name"]
    require(package["source"] is None, f"{name}: expected a local workspace package.")
    require(Path(package["manifest_path"]).resolve() == root / "crates" / name / "Cargo.toml",
            f"{name}: unexpected workspace package location.")
    require(package["license"] == "Apache-2.0", f"{name}: incorrect original-code licence.")
    require(package["edition"] == "2024", f"{name}: unexpected edition.")
    require(package["rust_version"] == toolchain, f"{name}: toolchain metadata drift.")
    require(package["publish"] == [], f"{name}: premature registry publication enabled.")
    local_edges = {packages[edge["pkg"]]["name"] for edge in node["deps"]
                   if packages[edge["pkg"]]["source"] is None}
    require(local_edges == WORKSPACE_EDGES[name],
            f"{name}: wrong inward workspace dependency edges: {sorted(local_edges)}")
    expected_external = DIRECT_DEPENDENCIES.get(name, {})
    # serde_json also generates the embedded corpus catalog in build.rs.
    expected_declarations = {(dependency, None) for dependency in WORKSPACE_EDGES[name]}
    expected_declarations |= {(dependency, None) for dependency in expected_external}
    if name == "glaux-standards":
        expected_declarations.add(("serde_json", "build"))
    declarations = package["dependencies"]
    require(len(declarations) == len(expected_declarations),
            f"{name}: unexpected direct dependency declarations.")
    declared = {(dependency["name"], dependency["kind"]) for dependency in declarations}
    require(declared == expected_declarations,
            f"{name}: unexpected direct dependency names/kinds.")
    for dependency in declarations:
        dependency_name = dependency["name"]
        require(dependency["target"] is None and not dependency["optional"]
                and dependency["rename"] is None,
                f"{name}/{dependency_name}: unreviewed dependency target/alias.")
        if dependency_name in expected_external:
            expected = expected_external[dependency_name]
            actual = {
                "version": dependency["req"],
                "default_features": dependency["uses_default_features"],
                "features": sorted(dependency["features"]),
            }
            require(actual == expected and dependency["source"] == REGISTRY
                    and not dependency.get("path") and not dependency.get("registry"),
                    f"{name}/{dependency_name}: direct dependency pin or feature drift.")
        else:
            require(dependency["source"] is None and not dependency["features"]
                    and dependency["uses_default_features"]
                    and Path(dependency["path"]).resolve() == root / "crates" / dependency_name,
                    f"{name}/{dependency_name}: local dependency declaration drift.")
    external_edges = {packages[edge["pkg"]]["name"] for edge in node["deps"]
                      if packages[edge["pkg"]]["source"] is not None}
    require(external_edges == set(expected_external), f"{name}: wrong external dependency edges.")


def cargo_inventory(root=ROOT, *, enforce_snapshot=True):
    root = root.resolve()
    require(Path.cwd().resolve() == root, "Run from the workspace root.")
    toolchain = tomllib.loads((root / "rust-toolchain.toml").read_text())["toolchain"]["channel"]
    lock = tomllib.loads((root / "Cargo.lock").read_text())
    require(lock["version"] == 4, "Inventory needs review: Cargo.lock format changed.")
    locked = {identity(package): package for package in lock["package"]}
    require(len(locked) == len(lock["package"]), "Duplicate Cargo.lock package identity.")
    metadata = json.loads(cargo("metadata", "--format-version=1", "--locked", "--offline", root=root))
    packages = {package["id"]: package for package in metadata["packages"]}
    nodes = {node["id"]: node for node in metadata["resolve"]["nodes"]}
    require(len(packages) == len(metadata["packages"])
            and len(nodes) == len(metadata["resolve"]["nodes"]) and set(nodes) == set(packages),
            "Incomplete or duplicated resolved Cargo dependency graph.")
    members = set(metadata["workspace_members"])
    require(len(members) == 3 and members <= set(packages)
            and {packages[item]["name"] for item in members} == set(WORKSPACE_EDGES),
            "Inventory needs review: expected exactly three workspace packages.")
    require({item for item, package in packages.items() if package["source"] is None} == members,
            "Inventory needs review: unexpected local package outside workspace.")
    require({identity(package) for package in packages.values()} == set(locked),
            "Inventory needs review: resolved package set differs from Cargo.lock.")
    inventory = []
    for package_id, package in sorted(packages.items(), key=lambda item: identity(item[1])):
        node = nodes[package_id]
        require(set(node["dependencies"]) == {edge["pkg"] for edge in node["deps"]},
                f"{label(package)}: inconsistent resolved edges.")
        require(set(node["dependencies"]) <= set(packages),
                f"{label(package)}: dependency missing from inventory.")
        checksum = locked[identity(package)].get("checksum")
        fetched_archive = None
        if package_id in members:
            check_workspace(package, node, packages, root, toolchain)
            notices = [{"path": "LICENSE", "sha256": sha256(root / "LICENSE")}]
        else:
            require(package["source"] == REGISTRY,
                    f"{label(package)}: unreviewed dependency source.")
            require(isinstance(checksum, str) and re.fullmatch(r"[a-f0-9]{64}", checksum),
                    f"{label(package)}: missing registry checksum.")
            directory = Path(package["manifest_path"]).resolve().parent
            require(directory.name == f"{package['name']}-{package['version']}"
                    and directory.parent.parent.name == "src"
                    and directory.parents[2].name == "registry",
                    f"{label(package)}: unexpected Cargo registry cache layout.")
            archive = (directory.parents[2] / "cache" / directory.parent.name
                       / (directory.name + ".crate"))
            require(archive.is_file(), f"{label(package)}: fetched crate archive is missing.")
            archive_digest = sha256(archive)
            require(archive_digest == checksum,
                    f"{label(package)}: fetched archive checksum differs from Cargo.lock.")
            fetched_archive = {"file": archive.name, "sha256": archive_digest}
            require(package["name"] not in NETWORK_CLIENTS,
                    f"{label(package)}: network client is outside the offline validator scope.")
            if package["name"] in {"hyper", "hyper-util"}:
                require(not any(feature == "client" or feature.startswith("client-") for feature in node["features"]),
                        f"{label(package)}: HTTP client feature is outside the health-server scope.")
            if package["name"] == "jsonschema":
                require(not {"resolve-http", "resolve-file"} & set(node["features"]),
                        "jsonschema: HTTP/filesystem resolution feature enabled.")
            notices = notice_files(package)
        inventory.append({
            "name": package["name"], "version": package["version"],
            "source": package["source"] or "local workspace", "checksum": checksum,
            "fetched_archive": fetched_archive,
            "declared_license": package["license"], "declared_license_file": package["license_file"],
            "notice_files": notices, "enabled_features": sorted(node["features"]),
            "dependencies": sorted_records([
                {"package": label(packages[edge["pkg"]]), "name": edge["name"],
                 "kinds": sorted_records(edge["dep_kinds"])} for edge in node["deps"]
            ]),
        })
    manifest_paths = [Path("Cargo.toml")] + [
        Path("crates") / name / "Cargo.toml" for name in sorted(WORKSPACE_EDGES)
    ]
    result = {
        "format_version": 1,
        "cargo_lock_sha256": sha256(root / "Cargo.lock"),
        "manifests": [{"path": path.as_posix(), "sha256": sha256(root / path)}
                      for path in manifest_paths],
        "metadata_configuration": "--format-version=1 --locked --offline; all target platforms",
        "packages": inventory,
        "third_party_cargo_packages": len(packages) - len(members),
        "notice_files_unavailable": [label(package) for package in inventory if not package["notice_files"]],
        "licence_metadata_unavailable": [label(package) for package in inventory
                                         if not package["declared_license"] and not package["declared_license_file"]],
    }
    if enforce_snapshot:
        snapshot = root / SNAPSHOT
        require(snapshot.is_file(), "Reviewed Cargo snapshot is missing; --candidate emits evidence only.")
        require(json.loads(snapshot.read_text()) == result,
                "Cargo inventory differs from reviewed docs/cargo-dependencies.json; review the candidate diff.")
    return result


def main():
    require(sys.argv[1:] in ([], ["--candidate"]), "Specify no arguments or exactly --candidate.")
    candidate = sys.argv[1:] == ["--candidate"]
    if candidate:
        print("Unapproved Cargo inventory candidate; review before committing the snapshot.", file=sys.stderr)
    print(json.dumps(cargo_inventory(enforce_snapshot=not candidate), indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
