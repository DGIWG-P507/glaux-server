#!/usr/bin/env python3
"""One hosted-only, frozen Phase 1 mutation pilot; not a permanent CI requirement.

Authority: planning evidence/04-test-strength-and-security-proposal.md.
Run only on the reviewed diagnostic PR, after the ordinary rust-suites checks.
No examples, listener proofs or database proofs are claimed by these lib tests.
Tool-reported catches still require review of the preserved assertion and diff.
"""

import hashlib
import io
import json
import os
from pathlib import Path
import platform
import re
import signal
import subprocess
import sys
import tarfile
import tempfile
import time
import urllib.request


VERSION = "27.1.0"
TOOL_COMMIT = "8ab1dc786a1f61a4e370416cc6c68b81a704e917"
ARCHIVE = "cargo-mutants-x86_64-unknown-linux-gnu.tar.gz"
URL = f"https://github.com/sourcefrog/cargo-mutants/releases/download/v{VERSION}/{ARCHIVE}"
ARCHIVE_SHA256 = "dfe6dc37d0342c891d2829b5a695aa57c2d0edecef7e7d0399a30cc6e206411e"
TOTAL_SECONDS = 900
PER_FUNCTION = 6
AUTH = "crates/glaux-server/src/authorization.rs"
MEDIA = "crates/glaux-server/src/http_boundary/media.rs"
AUTH_TEST = "authorization::tests::configured_policy_keeps_identity_actions_and_resource_pairs_distinct"
MEDIA_TESTS = (
    "http_boundary::media::tests::media_quality_uses_exact_grammar_and_specific_exclusions",
    "http_boundary::media::tests::media_parameters_preserve_quotes_values_and_weight_order",
)
TARGETS = {
    (AUTH, "PermissionSet::allows"): (AUTH_TEST,),
    (AUTH, "PermissionSet::allows_source"): (AUTH_TEST,),
    (MEDIA, "parse_quality"): MEDIA_TESTS,
    (MEDIA, "matches"): MEDIA_TESTS,
}
TESTS = (AUTH_TEST,) + MEDIA_TESTS
ROOT = Path(__file__).resolve().parents[1]


def save(path, value):
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def digest(data):
    return hashlib.sha256(data).hexdigest()


def source_hashes():
    names = subprocess.check_output(["git", "ls-files", "-z"], cwd=ROOT).split(b"\0")
    return {
        os.fsdecode(name): digest((ROOT / os.fsdecode(name)).read_bytes())
        for name in names if name
    }


def stop_session(child):
    """Stop only this subprocess session, including cargo's separate groups."""
    child.send_signal(signal.SIGINT)
    try:
        child.wait(timeout=5)
    except subprocess.TimeoutExpired:
        pass
    # cargo-mutants gives cargo its own process group, but not its own session.
    groups = set()
    for item in Path("/proc").iterdir():
        if not item.name.isdigit():
            continue
        try:
            fields = (item / "stat").read_text().rsplit(") ", 1)[1].split()
            if int(fields[3]) == child.pid:
                groups.add(int(fields[2]))
        except (FileNotFoundError, ProcessLookupError):
            continue
    for group in groups:
        try:
            os.killpg(group, signal.SIGKILL)
        except ProcessLookupError:
            pass
    child.wait(timeout=5)


class Pilot:
    def __init__(self, evidence, scratch):
        self.evidence = evidence
        self.scratch = scratch
        self.deadline = time.monotonic() + TOTAL_SECONDS
        self.commands = []
        self.env = os.environ.copy()
        # CLI options and this run, not inherited mutation filters or a shared
        # target directory, determine the diagnostic. No source is mutated in place.
        for key in list(self.env):
            if key.startswith("CARGO_MUTANTS_") or key == "CARGO_TARGET_DIR":
                del self.env[key]
        self.env.update(CARGO_TERM_COLOR="never", RUST_BACKTRACE="0", TMPDIR=str(scratch))

    def run(self, name, argv, seconds, separate_stderr=False):
        remaining = self.deadline - time.monotonic()
        if remaining <= 0:
            raise TimeoutError("15-minute pilot budget exhausted before " + name)
        timeout = min(seconds, remaining)
        record = {"name": name, "argv": argv, "timeout_seconds": timeout}
        self.commands.append(record)
        save(self.evidence / "commands.json", self.commands)
        print("Phase 1 mutation pilot:", name, flush=True)
        start = time.monotonic()
        with (self.evidence / (name + ".log")).open("wb") as stdout:
            with (self.evidence / (name + ".stderr.log")).open("wb") as stderr:
                child = subprocess.Popen(
                    argv, cwd=ROOT, env=self.env, stdin=subprocess.DEVNULL,
                    stdout=stdout, stderr=stderr if separate_stderr else subprocess.STDOUT,
                    start_new_session=True,
                )
                try:
                    record["returncode"] = child.wait(timeout=timeout)
                    record["timed_out"] = False
                except subprocess.TimeoutExpired:
                    record["timed_out"] = True
                    stop_session(child)
                    record["returncode"] = child.returncode
        record["elapsed_seconds"] = time.monotonic() - start
        save(self.evidence / "commands.json", self.commands)
        return record

    def checked(self, name, argv, seconds=60, separate_stderr=False):
        result = self.run(name, argv, seconds, separate_stderr)
        if result["timed_out"] or result["returncode"] != 0:
            raise RuntimeError(name + " failed or timed out; see preserved logs")
        return (self.evidence / (name + ".log")).read_text(encoding="utf-8")

    def install(self):
        # Exact release asset; no floating install, dependency resolution or retry.
        start = time.monotonic()
        with urllib.request.urlopen(URL, timeout=30) as response:
            archive = bytearray()
            while True:
                if time.monotonic() - start > 60:
                    raise TimeoutError("bounded tool download exceeded 60 seconds")
                chunk = response.read(65536)
                if not chunk:
                    break
                archive.extend(chunk)
                if len(archive) > 4 * 1024 * 1024:
                    raise RuntimeError("release archive unexpectedly exceeds 4 MiB")
        if digest(archive) != ARCHIVE_SHA256:
            raise RuntimeError("cargo-mutants release archive digest mismatch")
        (self.evidence / ARCHIVE).write_bytes(archive)
        # Do not extract paths supplied by the archive. Copy one regular member.
        executable = self.scratch / "cargo-mutants"
        with tarfile.open(fileobj=io.BytesIO(archive), mode="r:gz") as bundle:
            members = [m for m in bundle.getmembers() if Path(m.name).name == "cargo-mutants"]
            if len(members) != 1 or not members[0].isfile() or members[0].size > 64 * 1024 * 1024:
                raise RuntimeError("unexpected executable layout in pinned release")
            with bundle.extractfile(members[0]) as binary:
                executable.write_bytes(binary.read())
        executable.chmod(0o700)
        version = self.checked("tool-version", [str(executable), "mutants", "--version"])
        if version.strip() != "cargo-mutants " + VERSION:
            raise RuntimeError("unexpected executable version")
        save(self.evidence / "tool-pin.json", {
            "version": VERSION, "source_commit": TOOL_COMMIT, "license": "MIT",
            "url": URL, "sha256": ARCHIVE_SHA256,
            "executable_sha256": digest(executable.read_bytes()),
            "cli_and_json_source": f"https://github.com/sourcefrog/cargo-mutants/tree/{TOOL_COMMIT}/src",
        })
        return str(executable)


def target(candidate):
    function = candidate.get("function") or {}
    key = (candidate["file"], function.get("function_name"))
    return key if key in TARGETS else None


def priority(candidate):
    # Deterministic, before outcomes: comparisons/Boolean boundaries, negation,
    # fixed returns, then other generated operators. Source position breaks ties.
    genre = candidate["genre"]
    comparison = re.search(r"replace (?:==|!=|<=|>=|<|>|&&|\|\|) with ", candidate["name"])
    rank = 0 if genre == "BinaryOperator" and comparison else {
        "UnaryOperator": 1, "FnValue": 2,
    }.get(genre, 3)
    start = candidate["span"]["start"]
    return rank, start["line"], start["column"], candidate["name"]


def freeze(candidates, evidence):
    names = [item["name"] for item in candidates]
    if len(names) != len(set(names)):
        raise RuntimeError("duplicate generated names; cannot select unambiguously")
    selected = []
    missing = []
    for key in TARGETS:
        group = sorted((item for item in candidates if target(item) == key), key=priority)
        selected.extend(group[:PER_FUNCTION])
        if not group:
            missing.append(list(key))
    if len(selected) > 24:
        raise RuntimeError("pilot scope exceeded")
    selected_names = {item["name"] for item in selected}
    frozen = {
        "selection_rule": "per function: comparison/Boolean operators, negation, fixed returns, other operators; then source line/column/name; at most six",
        "selected": selected,
        "excluded": [
            {"candidate": item, "reason": "outside four functions" if target(item) is None else "per-function cap"}
            for item in candidates if item["name"] not in selected_names
        ],
        "targets_without_candidates": missing,
        "frozen_before_outcomes": True,
    }
    save(evidence / "selection-frozen.json", frozen)
    if missing or not selected:
        raise RuntimeError("target yielded no candidates; recorded, no replacement scope selected")
    return selected


def assertions(log, tests):
    """Locate actual target-test assertion text, not just a nonzero process."""
    found = []
    for match in re.finditer(r"thread '([^']+)'(?: \([^\n)]+\))? panicked at ([^\n]+)\n([^\n]*(?:\n(?!thread |test |failures:)[^\n]*){0,7})", log):
        if match[1] in tests and "assertion" in match[3]:
            found.append({"test": match[1], "location": match[2], "excerpt": match[0]})
    return found


def account(evidence, selected, command):
    output = evidence / "tool-output" / "mutants.out"
    data = json.loads((output / "outcomes.json").read_text())
    if data["cargo_mutants_version"] != VERSION:
        raise RuntimeError("outcome schema/version mismatch")
    baseline = [item for item in data["outcomes"] if item["scenario"] == "Baseline"]
    rows = {}
    selected_by_name = {item["name"]: item for item in selected}
    for outcome in data["outcomes"]:
        if outcome["scenario"] == "Baseline":
            continue
        mutant = outcome["scenario"]["Mutant"]
        name = mutant["name"]
        if name not in selected_by_name or name in rows:
            raise RuntimeError("unexpected or duplicate executed mutation")
        log_path = (output / outcome["log_path"]).resolve()
        if not log_path.is_relative_to(output.resolve()):
            raise RuntimeError("outcome log outside owned output directory")
        log = log_path.read_text(encoding="utf-8")
        summary = outcome["summary"]
        evidence_rows = assertions(log, TARGETS[target(selected_by_name[name])])
        phases = outcome["phase_results"]
        built = any(p["phase"] == "Build" and p["process_status"] == "Success" for p in phases)
        test_failed = any(p["phase"] == "Test" and p["process_status"] == {"Failure": 101} for p in phases)
        category = {
            "CaughtMutant": "caught-with-assertion-evidence" if built and test_failed and evidence_rows else "caught-unconfirmed",
            "MissedMutant": "survivor-unresolved",
            "Unviable": "unviable-not-detection",
            "Timeout": "timeout-not-detection",
        }.get(summary, "unclassified-not-detection")
        rows[name] = {
            "category": category, "tool_summary": summary,
            "assertions": evidence_rows, "log": str(log_path.relative_to(evidence)),
            "diff": outcome["diff_path"], "phase_results": phases,
            "manual_review": "check assertion/diff causal connection; survivors need behavioral/equivalent/unresolved classification; no automatic equivalence claim",
        }
    for name in selected_by_name:
        rows.setdefault(name, {"category": "unrun-not-detection"})
    baseline_ok = len(baseline) == 1 and baseline[0]["summary"] == "Success"
    if baseline_ok:
        log = (output / baseline[0]["log_path"]).read_text(encoding="utf-8")
        baseline_ok = all(log.count("test " + name + " ... ok") == 1 for name in TESTS)
    save(evidence / "outcome-accounting.json", {
        "baseline_passed_and_required_tests_executed": baseline_ok,
        "command": command, "selected": len(selected), "outcomes": rows,
        "interpretation": "No score or conformance claim. Tool catches plus assertion evidence require human/assistant source review before acceptance.",
    })
    return baseline_ok and not command["timed_out"] and command["returncode"] == 0 and all(
        row["category"] == "caught-with-assertion-evidence" for row in rows.values()
    )


def main():
    if os.environ.get("GITHUB_ACTIONS") != "true" or platform.system() != "Linux" or platform.machine() != "x86_64":
        raise RuntimeError("this diagnostic is authorized only on hosted Linux x86_64 CI")
    runner_temp = Path(os.environ["RUNNER_TEMP"]).resolve(strict=True)
    evidence = runner_temp / "glaux-ci-evidence" / "phase1-mutants"
    evidence.mkdir(parents=True, exist_ok=False)
    before = source_hashes()
    save(evidence / "source-before.json", before)
    save(evidence / "run.json", {
        "source_head": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
        "run_id": os.environ.get("GITHUB_RUN_ID"), "attempt": os.environ.get("GITHUB_RUN_ATTEMPT"),
        "budget_seconds": TOTAL_SECONDS, "maximum_per_function": PER_FUNCTION,
        "rust_toolchain": (ROOT / "rust-toolchain.toml").read_text(),
        "scope": [list(key) for key in TARGETS], "tests_required": TESTS,
    })
    passed = False
    try:
        with tempfile.TemporaryDirectory(prefix="glaux-phase1-mutants-", dir=runner_temp) as scratch:
            pilot = Pilot(evidence, Path(scratch))
            tool = pilot.install()
            common = [tool, "mutants", "--no-config", "--no-shuffle", "--colors=never", "--level=error", "--package=glaux-server"]
            for filename in (AUTH, MEDIA):
                common.extend(["--file", filename])
            candidate_text = pilot.checked("generated-candidates", common + ["--list", "--json"], 60, True)
            selected = freeze(json.loads(candidate_text), evidence)
            # Escape only Rust regex metacharacters, not Python's additional
            # escaped spaces/hyphens; the names are literal exact-match inputs.
            escaped = [re.sub(r"([\\.^$|?*+()\[\]{}])", r"\\\1", item["name"]) for item in selected]
            regex = "^(?:" + "|".join(escaped) + ")$"
            chosen = common + ["--re", regex]
            relisted = json.loads(pilot.checked("selected-candidates", chosen + ["--list", "--json"], 60, True))
            if sorted(relisted, key=lambda x: x["name"]) != sorted(selected, key=lambda x: x["name"]):
                raise RuntimeError("exact-name selection did not reproduce frozen candidates")
            baseline = pilot.checked("named-baseline", [
                "cargo", "test", "--locked", "--offline", "--package", "glaux-server", "--lib",
                "--", "--nocapture", "--test-threads=1",
            ], 120)
            if not all(baseline.count("test " + name + " ... ok") == 1 for name in TESTS):
                raise RuntimeError("a required baseline test did not execute exactly once")
            command = pilot.run("mutation-run", chosen + [
                "--output", str(evidence / "tool-output"), "--jobs=1", "--copy-target=true",
                "--cap-lints=false", "--baseline=run", "--build-timeout=120", "--timeout=30",
                "--cargo-arg=--locked", "--cargo-arg=--offline", "--cargo-arg=--lib",
                "--cargo-test-arg=--", "--cargo-test-arg=--nocapture", "--cargo-test-arg=--test-threads=1",
            ], TOTAL_SECONDS)
            passed = account(evidence, selected, command)
    except Exception as error:
        save(evidence / "failure.json", {"type": type(error).__name__, "message": str(error), "result": "incomplete-not-passed"})
        print("Phase 1 mutation pilot incomplete:", error, flush=True)
    finally:
        after = source_hashes()
        save(evidence / "source-after.json", after)
        if before != after:
            save(evidence / "source-integrity-failure.json", {"changed": sorted(set(before) ^ set(after) | {name for name in before.keys() & after.keys() if before[name] != after[name]})})
            passed = False
        save(evidence / "result.json", {
            "execution_and_assertion_checks_passed": passed,
            "source_unchanged": before == after,
            "review_still_required": True,
            "no_examples_or_external_proofs_executed_by_this_pilot": True,
        })
    print("Phase 1 mutation evidence:", evidence, flush=True)
    return 0 if passed else 1


if __name__ == "__main__":
    sys.exit(main())
