"""Prove task #9's public Rust type boundaries on the approved hosted runner.

The valid probe must compile, link and run before any compile-fail evidence is
accepted. Each negative requires one specific structured compiler diagnostic;
missing tools, linking errors and arbitrary compile failures are not proof.
Only the script-owned TemporaryDirectory is written or cleaned by the probes.
"""

import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time


ROOT = Path(__file__).resolve().parents[1]
TIMEOUT_SECONDS = 180
IMPORTS = "use glaux_domain::identity::{LocalId, Uid, SourceAuthority, SourceIdentifier, SourceIdentity};\n"
LOCAL = '"01890a5d-ac96-7b34-8d23-86f948ca9d1a".parse::<LocalId>().unwrap()'
UID = '"urn:glaux:fixture:system:alpha".parse::<Uid>().unwrap()'
AUTHORITY = '"https://source.example/".parse::<SourceAuthority>().unwrap()'
IDENTIFIER = '"external-42".parse::<SourceIdentifier>().unwrap()'
BASELINE = IMPORTS + f"""
fn main() {{
    let id = {LOCAL};
    assert_eq!(id.to_string(), "01890a5d-ac96-7b34-8d23-86f948ca9d1a");
    let uid = {UID};
    assert_eq!(uid.as_str(), "urn:glaux:fixture:system:alpha");
    let authority = {AUTHORITY};
    assert_eq!(authority.as_str(), "https://source.example/");
    let identifier = {IDENTIFIER};
    assert_eq!(identifier.as_str(), "external-42");
    let _source = SourceIdentity::new(authority, identifier);
    let _generated = LocalId::generate();
}}
"""
CASES = [
    ("uid-is-not-local-id", "E0308", ("LocalId", "Uid"),
     f"let _: LocalId = {UID};"),
    ("source-identifier-is-not-authority", "E0308", ("SourceAuthority", "SourceIdentifier"),
     f"let _: SourceAuthority = {IDENTIFIER};"),
    ("local-id-is-not-time", "E0308", ("SystemTime", "LocalId"),
     f"let _: std::time::SystemTime = {LOCAL};"),
    ("local-id-exposes-no-timestamp", "E0599", ("timestamp", "LocalId"),
     f"let id = {LOCAL}; let _ = id.timestamp();"),
    ("local-id-is-not-authorization", "E0308", ("bool", "LocalId"),
     f"let _: bool = {LOCAL};"),
    ("local-id-exposes-no-authorization", "E0599", ("is_authorized", "LocalId"),
     f"let id = {LOCAL}; let _ = id.is_authorized();"),
]


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def run(command, *, cwd=ROOT):
    started = time.monotonic()
    try:
        result = subprocess.run(
            command, cwd=cwd, text=True, stdout=subprocess.PIPE,
            stderr=subprocess.PIPE, timeout=TIMEOUT_SECONDS, check=False,
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        raise RuntimeError(f"Required identity runner unavailable or timed out: {error}") from error
    return result, round(time.monotonic() - started, 3)


def json_lines(text):
    records = []
    for line in text.splitlines():
        if line.strip():
            records.append(json.loads(line))
    return records


def domain_artifact(evidence):
    command = ["cargo", "build", "--locked", "--offline", "-p", "glaux-domain",
               "--message-format=json"]
    result, elapsed = run(command)
    (evidence / "identity-cargo-build.jsonl").write_text(result.stdout)
    (evidence / "identity-cargo-build.stderr.log").write_text(result.stderr)
    require(result.returncode == 0, f"Domain build failed, not type-boundary evidence:\n{result.stderr}")
    messages = json_lines(result.stdout)
    finished = [message for message in messages if message.get("reason") == "build-finished"]
    require(len(finished) == 1 and finished[0].get("success") is True,
            "Cargo did not report one successful completed build.")
    artifacts = [message for message in messages
                 if message.get("reason") == "compiler-artifact"
                 and message.get("target", {}).get("name") == "glaux_domain"
                 and "lib" in message.get("target", {}).get("kind", [])]
    require(len(artifacts) == 1, "Expected exactly one glaux-domain library build artifact.")
    candidates = [Path(name).resolve() for name in artifacts[0]["filenames"]
                  if name.endswith(".rlib")]
    require(len(candidates) == 1 and candidates[0].is_file(),
            "Cargo must identify one existing domain rlib, not a glob-selected stale build.")
    artifact = candidates[0]
    dependencies = artifact.parent if artifact.parent.name == "deps" else artifact.parent / "deps"
    require(dependencies.is_dir(), "Built dependency search directory is absent.")
    return artifact, dependencies, {"command": command, "exit": result.returncode,
                                    "elapsed_seconds": elapsed, "artifact": str(artifact)}


def compile_probe(directory, artifact, dependencies, name, source, evidence):
    source_path = directory / (name + ".rs")
    executable = directory / name
    source_path.write_text(source)
    command = ["rustc", "--edition=2024", "--crate-name", name.replace("-", "_"),
               "--crate-type=bin", "--error-format=json", "-A", "unused-imports",
               "--extern", f"glaux_domain={artifact}",
               "-L", f"dependency={dependencies}", "-o", str(executable), str(source_path)]
    result, elapsed = run(command)
    (evidence / (name + ".stderr.jsonl")).write_text(result.stderr)
    (evidence / (name + ".stdout.log")).write_text(result.stdout)
    diagnostics = json_lines(result.stderr)
    require(all(item.get("$message_type") == "diagnostic" for item in diagnostics),
            "Unexpected rustc diagnostic protocol, not type-boundary evidence.")
    row = {"case": name, "command": command, "exit": result.returncode,
           "elapsed_seconds": elapsed, "diagnostics": diagnostics}
    return result, row, executable


def intended_rejection(result, row, expected_code, expected_terms):
    # rustc also emits an uncoded summary ('aborting due to ...'); it must not
    # replace the actual coded type error. Other coded failures are rejected.
    coded_errors = [item for item in row["diagnostics"]
                    if item.get("level") == "error" and item.get("code") is not None]
    if result.returncode != 1 or len(coded_errors) != 1:
        return False
    diagnostic = coded_errors[0]
    if diagnostic["code"].get("code") != expected_code:
        return False
    context = json.dumps({"message": diagnostic.get("message"),
                          "spans": diagnostic.get("spans"),
                          "children": diagnostic.get("children")})
    return all(term in context for term in expected_terms)


def main():
    require(not sys.argv[1:], "No probe-selection override is accepted.")
    require(Path.cwd().resolve() == ROOT, "Run from the workspace root.")
    require(sys.platform == "linux" and os.environ.get("GITHUB_ACTIONS") == "true"
            and os.environ.get("RUNNER_ENVIRONMENT") == "github-hosted",
            "Identity boundary probes run only on the approved GitHub-hosted Linux runner.")
    runner_temp = Path(os.environ["RUNNER_TEMP"]).resolve()
    require(runner_temp.is_dir() and not runner_temp.is_relative_to(ROOT),
            "RUNNER_TEMP must be an existing directory outside the checkout.")
    evidence = runner_temp / "glaux-ci-evidence"
    evidence.mkdir(exist_ok=True)
    artifact, dependencies, build = domain_artifact(evidence)
    outcomes = [{"phase": "build", **build}]

    def record(row):
        outcomes.append(row)
        (evidence / "identity-boundary.json").write_text(json.dumps(outcomes, indent=2))
        summary = {key: value for key, value in row.items() if key != "diagnostics"}
        print(json.dumps(summary), flush=True)

    with tempfile.TemporaryDirectory(prefix="glaux-identity-boundary-", dir=runner_temp) as name:
        directory = Path(name)
        result, baseline, executable = compile_probe(
            directory, artifact, dependencies, "valid-public-types", BASELINE, evidence,
        )
        require(result.returncode == 0 and executable.is_file(),
                f"Valid public-type probe did not compile/link:\n{result.stderr}")
        executed, elapsed = run([str(executable)], cwd=directory)
        record({"phase": "baseline", **baseline, "execution_exit": executed.returncode,
                "execution_seconds": elapsed})
        require(executed.returncode == 0,
                f"Valid public-type probe did not execute:\n{executed.stderr}")
        # Deliberately feed real successful compiler output into every negative
        # classifier. A classifier accepting 'anything happened' would fail here.
        refused = [not intended_rejection(result, baseline, code, terms)
                   for _, code, terms, _ in CASES]
        record({"phase": "sensitivity", "valid_compile_refused_as_negative": refused})
        require(all(refused), "Type-boundary classifier accepted a successful valid probe.")
        for case, code, terms, statement in CASES:
            source = IMPORTS + "fn main() { " + statement + " }\n"
            result, row, _ = compile_probe(
                directory, artifact, dependencies, case, source, evidence,
            )
            detected = intended_rejection(result, row, code, terms)
            record({"phase": "negative", **row, "expected_code": code,
                    "expected_terms": terms, "detected": detected})
            require(detected, f"Missing intended public type boundary {case}:\n{result.stderr}")
    print("Identity public type boundaries: 6 rejected; valid baseline passed; success sensitivity passed.",
          flush=True)


if __name__ == "__main__":
    main()
