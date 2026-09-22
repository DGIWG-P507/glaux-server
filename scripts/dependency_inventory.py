"""Emit this task's dependency/licence evidence as JSON, never an approval scan.

Run from the workspace root on the approved hosted Linux runner after provisioning
the pinned Rust components and image. No pip package, user database, networked
container or persistent inventory store is used. Any failure prevents JSON output.
"""

from contextlib import redirect_stdout
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import subprocess
import sys
import tomllib

from database_harness import DisposablePostgis, PIN, ROOT, docker
from cargo_inventory import cargo_inventory


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def command(*args):
    result = subprocess.run(
        args, check=False, text=True, capture_output=True, timeout=90, cwd=ROOT,
    )
    print(result.stderr, end="", file=sys.stderr)
    if result.returncode:
        print(result.stdout, end="", file=sys.stderr)
        result.check_returncode()
    return result.stdout.strip()


def action_inventory():
    actions = []
    # Update this reviewed set with the workflow when a later task changes a pin.
    allowed = {
        "actions/checkout": "3d3c42e5aac5ba805825da76410c181273ba90b1",
        "actions/upload-artifact": "043fb46d1a93c77aae656e7c1c64a875d1fc6a0a",
    }
    workflows = sorted(path for path in (ROOT / ".github/workflows").iterdir()
                       if path.suffix in (".yaml", ".yml"))
    for workflow in workflows:
        for line in workflow.read_text().splitlines():
            if not re.match(r"\s*(?:-\s*)?uses:", line):
                continue
            match = re.fullmatch(
                r"\s*(?:-\s*)?uses:\s*([\w./-]+)@([a-f0-9]{40})(?:\s+#.*)?\s*", line,
            )
            require(match is not None, "Inventory needs review: action is not SHA pinned.")
            repository, revision = match.groups()
            require(allowed.get(repository) == revision,
                    "Inventory needs review: unexpected workflow action or changed pin.")
            actions.append({
                "repository": repository, "revision": revision,
                "workflow": str(workflow.relative_to(ROOT)),
                "project_license": "MIT",
                "license_source": f"https://github.com/{repository}/blob/{revision}/LICENSE",
                "bundled_component_notice": "Project licence is not an inventory of bundled npm licences.",
            })
    require({action["repository"] for action in actions} == set(allowed),
            "Expected checkout and inventory-upload actions were not both found.")
    return actions


def image_inventory():
    details = json.loads(docker("image", "inspect", PIN["image"]))[0]
    require(details["Os"] + "/" + details["Architecture"] == PIN["platform"],
            "Image platform differs from the approved pin.")
    # Reuse the harness's exact owned-target validation and fatal cleanup handling.
    # Its ordinary readiness/cleanup diagnostics must not contaminate stdout JSON.
    with redirect_stdout(sys.stderr), DisposablePostgis() as database:
        database.validate_target()
        fields = "${Package}\t${Version}\t${Architecture}\t${db:Status-Status}\t${source:Package}\t${source:Version}\n"
        raw = docker("exec", database.container_id, "dpkg-query", "-W", "-f=" + fields)
        packages = []
        for line in raw.splitlines():
            row = line.split("\t")
            require(len(row) == 6, "Malformed dpkg inventory row.")
            name, version, architecture, status, source, source_version = row
            if status != "installed":
                continue
            require(re.fullmatch(r"[a-z0-9][a-z0-9+.-]+", name) is not None,
                    "Unexpected Debian package name.")
            packages.append({
                "name": name, "version": version, "architecture": architecture,
                "source_package": source, "source_version": source_version,
            })
        require(packages, "No installed packages found in the pinned image.")
        names = sorted({package["name"] for package in packages})
        # Fixed shell program; input is validated package names, never shell code.
        # Hash actual available packaging copyright files, following their normal
        # documentation symlinks; do not guess licence expressions from prose.
        copyright_script = """while IFS= read -r package; do
    file="/usr/share/doc/$package/copyright"
    if [ -r "$file" ]; then
        digest=$(sha256sum "$file")
        resolved=$(readlink -f "$file")
        printf '%s\\t%s\\t%s\\t%s\\n' "$package" "$file" "$resolved" "${digest%% *}"
    else
        printf '%s\\t%s\\tMISSING\\tMISSING\\n' "$package" "$file"
    fi
done"""
        database.validate_target()
        raw_copyrights = docker(
            "exec", "-i", database.container_id, "sh", "-eu", "-c", copyright_script,
            input="\n".join(names) + "\n", timeout=60,
        )
        copyrights = {}
        for line in raw_copyrights.splitlines():
            name, path, resolved, digest = line.split("\t")
            require(name in names and name not in copyrights, "Unexpected copyright row.")
            present = digest != "MISSING"
            require(not present or re.fullmatch(r"[a-f0-9]{64}", digest) is not None,
                    "Invalid copyright-file hash.")
            copyrights[name] = {
                "path": path, "available": present,
                "resolved_path": resolved if present else None,
                "sha256": digest if present else None,
                "license_expression": None,
            }
        require(set(copyrights) == set(names), "Incomplete package copyright accounting.")
        for package in packages:
            package["copyright_file"] = copyrights[package["name"]]
        database.setup()
        extension = database.query("SELECT extversion FROM pg_extension WHERE extname='postgis';")
        require(extension == PIN["postgis_version"], "Inventory PostGIS version mismatch.")
    return {
        "pin": PIN, "docker_image_id": details["Id"],
        "repository_digests": details["RepoDigests"],
        "packages": sorted(packages, key=lambda package: (package["name"], package["architecture"])),
        "package_count": len(packages),
        "copyright_files_unavailable": [name for name in names if not copyrights[name]["available"]],
        "licence_scope": "Installed dpkg packages and available packaging copyright hashes; not inferred SPDX expressions or a complete image SBOM.",
    }


def main():
    require(Path.cwd().resolve() == ROOT, "Run from the workspace root.")
    toolchain = tomllib.loads((ROOT / "rust-toolchain.toml").read_text())["toolchain"]
    result = {
        "commit": command("git", "rev-parse", "HEAD"),
        "cargo_lock_sha256": hashlib.sha256((ROOT / "Cargo.lock").read_bytes()).hexdigest(),
        "rust_toolchain": toolchain,
        "versions": {
            "rustc": command("rustc", "--version", "--verbose"),
            "cargo": command("cargo", "--version", "--verbose"),
            "rustfmt": command("rustfmt", "--version"),
            "clippy": command("cargo", "clippy", "--version"),
            "python": sys.version,
            "docker": docker("version", "--format", "{{json .}}"),
        },
        "runner": {
            "system": platform.platform(), "image_os": os.environ.get("ImageOS"),
            "image_version": os.environ.get("ImageVersion"),
        },
        "cargo": cargo_inventory(), "actions": action_inventory(),
        "database_image": image_inventory(),
        "limitations": [
            "Cargo packages/features/notices match the separately reviewed committed snapshot; this inventory is evidence, not automatic dependency or legal approval.",
            "Missing copyright files are disclosed, not converted to a guessed licence.",
            "No CVE scan, legal approval, release SBOM or security certification is claimed.",
            "Hosted runner OS/tools and bundled action/toolchain components retain their own notices; this is not a complete transitive inventory of the runner.",
        ],
    }
    print(json.dumps(result, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
