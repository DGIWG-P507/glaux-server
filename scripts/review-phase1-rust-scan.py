"""Temporary, approved Rust-only Phase 1 diagnostic; DO NOT MERGE.

Runs once on the disposable public-repository PR runner. No dependency updates,
Security uploads, settings changes or laptop execution. Exit 2 means incomplete;
exit 1 means findings or extraction diagnostics require review. Exit 0 is only a
completed default-suite run without reported candidates or recorded diagnostics,
not a security certification or proof that all Rust semantics were modeled.
"""

import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import resource
import shutil
import signal
import subprocess
import sys
import tempfile
import time
import tomllib
import zipfile


ROOT = Path(__file__).resolve().parents[1]
BASE = "27955c1b9260cd811ad6bc08f85feab43ad65028"
ALLOWED_DIFF = {".github/workflows/build.yml", "scripts/review-phase1-rust-scan.py"}
PIN = {
    "version": "2.27.1",
    "url": "https://github.com/github/codeql-action/releases/download/"
           "codeql-bundle-v2.27.1/codeql-bundle-linux64.tar.gz",
    "sha256": "1d380f79896ededc654c7b21fafb3360136f1aeb678ad4df4df9af3910c6b815",
    "release": "https://github.com/github/codeql-action/releases/tag/codeql-bundle-v2.27.1",
    "digest_source": "https://api.github.com/repos/github/codeql-action/releases/tags/codeql-bundle-v2.27.1",
    "license": "GitHub CodeQL Terms and Conditions (CLI); MIT (queries); bundled notices apply",
    "license_source": "https://github.com/github/codeql-cli-binaries/blob/v2.27.1/LICENSE.md",
    "query_source": "https://github.com/github/codeql/tree/6e9f9e38390175c41b99070a423c875f450759ca",
    "query_pack": "codeql/rust-queries@0.1.43",
    "extractor_option_source": "https://github.com/github/codeql/blob/"
                               "6e9f9e38390175c41b99070a423c875f450759ca/rust/extractor/src/config.rs",
}


def now():
    return datetime.now(timezone.utc).isoformat()


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def write_json(path, value):
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def snapshot(tracked):
    return {name: digest(ROOT / name) if (ROOT / name).is_file() else None for name in tracked}


class StageFailure(Exception):
    pass


def strip_ansi(text):
    # Logging color is presentation, not configuration or diagnostic severity.
    return re.sub(r"\x1b\[[0-?]*[ -/]*[@-~]", "", text)


def is_diagnostic(line):
    return bool(re.search(r"(?:^|\s)(?:WARN(?:ING)?|ERROR|FATAL)(?:\s|:)", strip_ansi(line), re.I))


def extractor_config_checks(text, options, command):
    text = strip_ansi(text)
    text = "\n".join(re.sub(r"^(?:\[[^\]]*\]\s*)+", "", line) for line in text.splitlines())
    start = text.find("INFO configuration: {")
    end = text.find("\n}", start)
    if start < 0 or end < 0:
        raise StageFailure("actual extractor configuration block unavailable")
    config = text[start:end + 2]
    checks = {"all_targets": bool(re.search(r"cargo_all_targets:\s*true,", config)),
              "default_features": bool(re.search(r'cargo_features:\s*\[\s*"default",?\s*\]', config))}
    for key in ("SYSROOT", "SYSROOT_SRC", "PROC_MACRO_SERVER"):
        checks[key.lower()] = bool(re.search(key.lower() + r":\s*Some\(\s*" +
                                            re.escape(json.dumps(options[key])) + r",?\s*\)", config))
    match = re.search(r"build_script_command:\s*(\[.*?\]),", config, re.S)
    checks["build_script_command"] = bool(match and json.loads(
        re.sub(r",\s*\]", "]", match.group(1))) == command)
    return config, checks


class Campaign:
    def __init__(self, evidence, work, budget):
        self.evidence, self.work = evidence, work
        self.started = time.monotonic()
        self.deadline = self.started + budget
        self.env = os.environ.copy()
        for name in ("GITHUB_TOKEN", "GH_TOKEN", "GITHUB_AUTH_TOKEN", "CODEQL_REGISTRIES_AUTH"):
            self.env.pop(name, None)
        self.summary = {
            "started": now(), "baseline": BASE, "pin": PIN, "budget_seconds": budget,
            "extraction_cap_seconds": 300, "analysis_cap_seconds": 900,
            "threads": 2, "ram_mb": 4096, "commands": [], "status": "not_run",
            "host": {"platform": platform.platform(), "python": platform.python_version(),
                     "logical_cpus": os.cpu_count(),
                     "physical_memory_bytes": os.sysconf("SC_PAGE_SIZE") * os.sysconf("SC_PHYS_PAGES")},
            "limitations": [
                "Rust default suite only; prior mutation/audit/Python/Actions evidence is reused.",
                "No project-toolchain downgrade, all-features change, fixes, retry loop or Security upload.",
                "Pinned CodeQL metadata uses its own fixed Rust 1.97.0 and may provision it on the disposable runner; "
                "the explicit project build command, sysroot and procedural-macro server remain Rust 1.98.1.",
                "Source archive presence does not prove complete semantic extraction.",
                "Warnings remain review limitations, never a clean security result.",
            ],
        }
        self.save()

    def save(self):
        write_json(self.evidence / "rust-followup-summary.json", self.summary)

    def check_parser(self):
        options = {"SYSROOT": "/fixture/toolchain", "SYSROOT_SRC": "/fixture/library",
                   "PROC_MACRO_SERVER": "/fixture/proc-macro-server"}
        command = ["cargo", "+1.98.1", "check", "--locked", "--offline"]
        fixture = '''INFO configuration: {
cargo_all_targets: true,
cargo_features: ["default",],
sysroot: Some("/fixture/toolchain",),
sysroot_src: Some("/fixture/library",),
proc_macro_server: Some("/fixture/proc-macro-server",),
build_script_command: ["cargo", "+1.98.1", "check", "--locked", "--offline",],
}'''
        plain = "\n".join("[2026-10-03 00:00:00] [build-stdout] " + line for line in fixture.splitlines())
        colored = plain.replace("INFO", "\x1b[32mINFO\x1b[0m")
        cases = [("uncolored_configuration", plain, True),
                 ("colored_configuration", colored, True),
                 ("colored_wrong_setting", colored.replace("cargo_all_targets: true", "cargo_all_targets: false"), False),
                 ("absent_configuration", "[build-stdout] no configuration emitted", False)]
        outcomes = []
        for name, text, expected in cases:
            try:
                _, checks = extractor_config_checks(text, options, command)
                accepted = all(checks.values())
            except StageFailure:
                accepted = False
            outcomes.append({"case": name, "expected_acceptance": expected,
                             "actual_acceptance": accepted, "passed": accepted == expected})
        for name, line, expected in (
                ("colored_warning_detected", "[build-stdout] \x1b[33mWARN\x1b[0m generated source warning", True),
                ("ordinary_info_not_warning", "[build-stdout] INFO normal extraction --warnings=show", False)):
            actual = is_diagnostic(line)
            outcomes.append({"case": name, "expected_detection": expected,
                             "actual_detection": actual, "passed": actual == expected})
        write_json(self.evidence / "configuration-parser-controls.json", outcomes)
        if not all(item["passed"] for item in outcomes):
            raise StageFailure("configuration-parser sensitivity control failed")

    def run(self, name, args, *, cwd=None, cap=120):
        remaining = self.deadline - time.monotonic()
        record = {"name": name, "argv": [str(item) for item in args], "started": now(),
                  "cwd": str(cwd or self.work), "status": "not_run", "cap_seconds": cap}
        self.summary["commands"].append(record)
        self.save()
        if remaining <= 2:
            record["reason"] = "campaign budget exhausted"
            self.save()
            raise StageFailure(record["reason"])
        effective = min(cap, remaining - 1)
        stdout = self.evidence / (name + ".stdout.log")
        stderr = self.evidence / (name + ".stderr.log")
        record.update(stdout=stdout.name, stderr=stderr.name, effective_timeout_seconds=effective)
        print(f"Rust diagnostic: {name}", flush=True)
        started = time.monotonic()
        usage_before = resource.getrusage(resource.RUSAGE_CHILDREN)
        try:
            with stdout.open("wb") as out, stderr.open("wb") as err:
                process = subprocess.Popen(args, cwd=cwd or self.work, env=self.env,
                                           stdout=out, stderr=err, start_new_session=True)
                try:
                    code = process.wait(timeout=effective)
                    record.update(status="completed", exit_code=code)
                except subprocess.TimeoutExpired:
                    os.killpg(process.pid, signal.SIGKILL)
                    process.wait()
                    record.update(status="timed_out", exit_code=process.returncode)
        except OSError as error:
            record.update(status="setup_failed", error=str(error))
        usage_after = resource.getrusage(resource.RUSAGE_CHILDREN)
        record.update(finished=now(), elapsed_seconds=round(time.monotonic() - started, 3),
                      child_user_cpu_seconds=usage_after.ru_utime - usage_before.ru_utime,
                      child_system_cpu_seconds=usage_after.ru_stime - usage_before.ru_stime,
                      cumulative_children_maxrss_kib=usage_after.ru_maxrss)
        self.save()
        if record["status"] != "completed" or record.get("exit_code") != 0:
            raise StageFailure(f"{name}: {record['status']}, exit={record.get('exit_code')}")
        return stdout

    def prepare(self):
        archive = self.work / "codeql.tar.gz"
        self.run("codeql-download", ["curl", "--fail", "--location", "--silent", "--show-error",
                 "--proto", "=https", "--tlsv1.2", "--connect-timeout", "20", "--max-time", "180",
                 "--output", str(archive), PIN["url"]], cap=185)
        self.summary["download"] = {"sha256": digest(archive), "bytes": archive.stat().st_size}
        self.save()
        if self.summary["download"]["sha256"] != PIN["sha256"]:
            raise StageFailure("CodeQL archive checksum mismatch")
        directory = self.work / "bundle"
        directory.mkdir()
        self.run("codeql-unpack", ["tar", "--extract", "--gzip", "--file", str(archive),
                 "--directory", str(directory), "--no-same-owner", "--no-same-permissions"], cap=90)
        binary = directory / "codeql" / "codeql"
        output = self.run("codeql-version", [str(binary), "version", "--format=json"], cap=15)
        version = json.loads(output.read_text())
        if version.get("version") != PIN["version"]:
            raise StageFailure("unexpected CodeQL version")
        self.summary["codeql_version"] = version
        self.run("codeql-languages", [str(binary), "resolve", "languages", "--format=json"], cap=15)
        self.run("rust-resolved-extractor", [str(binary), "resolve", "extractor", "--language=rust",
                 "--format=betterjson"], cap=15)
        shutil.copyfile(directory / "codeql" / "rust" / "codeql-extractor.yml",
                        self.evidence / "codeql-extractor.yml")
        suites = list(directory.rglob("codeql-suites/rust-code-scanning.qls"))
        if len(suites) != 1:
            raise StageFailure("bundled default Rust suite is not unique")
        suite = suites[0]
        manifest = suite.parent.parent / "qlpack.yml"
        text = manifest.read_text()
        if not re.search(r"^version:\s*0\.1\.43\s*$", text, re.M) or not re.search(
                r"^name:\s*codeql/rust-queries\s*$", text, re.M):
            raise StageFailure("unexpected bundled Rust query pack")
        shutil.copyfile(manifest, self.evidence / "rust-qlpack.yml")
        shutil.copyfile(suite, self.evidence / "rust-code-scanning.qls")
        self.summary["suite"] = {"path": str(suite.relative_to(directory)), "sha256": digest(suite)}
        self.run("rust-default-query-inventory", [str(binary), "resolve", "queries", str(suite),
                 "--format=json"], cap=30)
        self.save()
        return directory, binary, suite

    def configure_extractor(self):
        channel = self.summary["project_toolchain"]
        output = self.run("project-sysroot", ["rustc", "+" + channel, "--print", "sysroot"],
                          cwd=ROOT, cap=15)
        sysroot = Path(output.read_text().strip()).resolve(strict=True)
        source = sysroot / "lib" / "rustlib" / "src" / "rust" / "library"
        if not source.is_dir():
            self.run("matching-rust-src", ["rustup", "component", "add", "rust-src",
                     "--toolchain", channel], cwd=ROOT, cap=90)
        if not source.is_dir():
            raise StageFailure("matching Rust standard-library source unavailable")
        self.run("project-components", ["rustup", "component", "list", "--installed",
                 "--toolchain", channel], cwd=ROOT, cap=15)
        servers = [sysroot / folder / "rust-analyzer-proc-macro-srv" for folder in ("libexec", "lib")]
        server = next((path for path in servers if path.is_file()), None)
        if server is None:
            raise StageFailure("matching compiler procedural-macro server unavailable; no extra tool installed")
        command = ["cargo", "+" + channel, "check", "--workspace", "--all-targets", "--locked",
                   "--offline", "--message-format=json", "--target-dir", str(ROOT / "target")]
        # These are pinned-source diagnostic controls, not advertised public CLI
        # options. Readback below must show them, or the attempt remains incomplete.
        options = {"BUILD_SCRIPT_COMMAND": json.dumps(command), "SYSROOT": str(sysroot),
                   "SYSROOT_SRC": str(source), "PROC_MACRO_SERVER": str(server),
                   "CARGO_ALL_TARGETS": "true"}
        self.summary["extractor_environment_keys_before"] = sorted(
            key for key in self.env if key.startswith("CODEQL_EXTRACTOR_RUST_"))
        if self.summary["extractor_environment_keys_before"]:
            raise StageFailure("unexpected inherited Rust extractor options; refusing hidden configuration")
        for key, value in options.items():
            self.env["CODEQL_EXTRACTOR_RUST_OPTION_" + key] = value
        self.summary["extractor_configuration"] = {
            "environment": options, "public_option": "rust.cargo_features=default",
            "build_command": command, "proc_macro_server_sha256": digest(server),
            "limit": "Pinned-source adaptation; success and semantic warnings must be inspected, not presumed supported.",
        }
        self.save()
        return options, command

    def verify_extractor_config(self, output, options, command):
        config, checks = extractor_config_checks(output.read_text(), options, command)
        (self.evidence / "actual-extractor-configuration.log").write_text(config + "\n", encoding="utf-8")
        self.summary["actual_extractor_configuration_checks"] = checks
        self.save()
        if not all(checks.values()):
            raise StageFailure("extractor did not confirm every approved override; no fallback accepted")

    def scan(self, tracked):
        directory, binary, suite = self.prepare()
        options, command = self.configure_extractor()
        database = self.work / "database-rust"
        logs = self.evidence / "codeql-rust-logs"
        logs.mkdir()
        create = [str(binary), "database", "create", str(database), "--language=rust",
                  "--source-root=" + str(ROOT), "--build-mode=none", "--threads=2", "--ram=4096",
                  "--extractor-option=rust.cargo_features=default", "--logdir=" + str(logs)]
        output = self.run("rust-extract", create, cap=300)
        self.verify_extractor_config(output, options, command)
        metadata = database / "codeql-database.yml"
        if metadata.is_file():
            shutil.copyfile(metadata, self.evidence / metadata.name)
        archive = database / "src.zip"
        with zipfile.ZipFile(archive) as source:
            archived = sorted(name for name in source.namelist() if not name.endswith("/"))
        expected = [name for name in tracked if name.endswith(".rs")]
        matched = [name for name in expected if any(item == name or item.endswith("/" + name)
                                                   for item in archived)]
        missing = sorted(set(expected) - set(matched))
        write_json(self.evidence / "rust-source-presence.json", {
            "tracked_candidates": expected, "matched_source_archive": matched,
            "unmatched_tracked_candidates": missing, "source_archive_entries": archived,
            "limit": "File presence is not proof every Rust path, macro or target was modeled.",
        })
        self.summary.update(archived_tracked_files=len(matched), unmatched_tracked_files=len(missing))
        if not matched:
            raise StageFailure("no tracked Rust files matched the source archive")
        sarif = self.evidence / "rust.sarif"
        self.run("rust-analyze", [str(binary), "database", "analyze", str(database), str(suite),
                 "--format=sarif-latest", "--output=" + str(sarif),
                 "--sarif-category=phase1-rust-followup", "--threads=2", "--ram=4096",
                 "--logdir=" + str(logs)], cap=900)
        report = json.loads(sarif.read_text())
        runs = report.get("runs", [])
        if report.get("version") != "2.1.0" or not runs:
            raise StageFailure("missing or unexpected SARIF runs")
        notifications, count, rules = [], 0, []
        for run in runs:
            inventory = run.get("tool", {}).get("driver", {}).get("rules", [])
            invocations = run.get("invocations", [])
            if not inventory or not invocations or any(
                    invocation.get("executionSuccessful") is not True for invocation in invocations):
                raise StageFailure("SARIF lacks query inventory or successful invocation evidence")
            rules.extend(inventory)
            count += len(run.get("results", []))
            for invocation in invocations:
                notifications.extend(invocation.get("toolExecutionNotifications", []))
                notifications.extend(invocation.get("toolConfigurationNotifications", []))
        problems = [item for item in notifications if item.get("level") in ("error", "warning")]
        write_json(self.evidence / "rust-sarif-rule-inventory.json", rules)
        self.summary.update(reported_results=count, diagnostic_notifications=len(problems),
                            sarif=sarif.name, sarif_sha256=digest(sarif),
                            status="completed_pending_diagnostic_review")

    def collect_diagnostics(self):
        messages = []
        for path in sorted(self.evidence.rglob("*.log")):
            for number, line in enumerate(path.read_text(errors="replace").splitlines(), 1):
                if is_diagnostic(line):
                    messages.append({"file": str(path.relative_to(self.evidence)),
                                     "line": number, "text": line})
        write_json(self.evidence / "rust-diagnostic-lines.json", messages)
        self.summary["diagnostic_log_line_count"] = len(messages)
        self.summary["diagnostic_line_limit"] = (
            "Conservative text inventory, including duplicates; requires human/assistant triage, not a defect count.")
        if self.summary["status"] == "completed_pending_diagnostic_review":
            self.summary["status"] = (
                "reported_candidates" if self.summary["reported_results"] else
                "completed_with_coverage_or_diagnostic_questions" if messages or
                self.summary["unmatched_tracked_files"] or self.summary["diagnostic_notifications"] else
                "completed_no_reported_candidates")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--approved-ci", action="store_true")
    parser.add_argument("--budget-seconds", type=int, default=1200)
    args = parser.parse_args()
    if not (args.approved_ci and os.environ.get("GITHUB_ACTIONS") == "true" and
            os.environ.get("GITHUB_REPOSITORY") == "DGIWG-P507/glaux-server" and
            os.environ.get("GITHUB_EVENT_NAME") == "pull_request" and
            os.environ.get("GITHUB_HEAD_REF") == "review/phase1-rust-scan-followup" and
            platform.system() == "Linux" and platform.machine() == "x86_64" and
            30 <= args.budget_seconds <= 1200):
        raise SystemExit("Refusing: approved diagnostic PR on hosted Linux with bounded budget required.")
    runner_temp = Path(os.environ["RUNNER_TEMP"]).resolve(strict=True)
    evidence = runner_temp / "glaux-ci-evidence" / "phase1-rust-followup"
    evidence.mkdir(parents=True, exist_ok=False)
    work = Path(tempfile.mkdtemp(prefix="glaux-phase1-rust-followup-", dir=runner_temp))
    campaign = Campaign(evidence, work, args.budget_seconds)
    tracked, before = [], {}
    try:
        campaign.check_parser()
        output = campaign.run("source-head", ["git", "rev-parse", "HEAD"], cwd=ROOT, cap=10)
        campaign.summary["head"] = output.read_text().strip()
        if campaign.summary["head"] != os.environ.get("TESTED_HEAD"):
            raise StageFailure("checkout differs from exact reviewed workflow head")
        campaign.run("fetch-reviewed-baseline", ["git", "fetch", "--no-tags", "--depth=1",
                     "https://github.com/DGIWG-P507/glaux-server.git", BASE], cwd=ROOT, cap=45)
        output = campaign.run("baseline-diff", ["git", "diff", "--name-only", BASE, "HEAD"],
                              cwd=ROOT, cap=15)
        if not set(output.read_text().splitlines()).issubset(ALLOWED_DIFF):
            raise StageFailure("task head changes files outside the two approved diagnostic files")
        campaign.run("clean-tracked-source", ["git", "diff", "--exit-code", "HEAD"], cwd=ROOT, cap=15)
        output = campaign.run("tracked-files", ["git", "ls-files"], cwd=ROOT, cap=10)
        tracked = output.read_text().splitlines()
        before = snapshot(tracked)
        write_json(evidence / "tracked-before.json", before)
        campaign.summary["lockfile_before_sha256"] = before["Cargo.lock"]
        with (ROOT / "rust-toolchain.toml").open("rb") as stream:
            campaign.summary["project_toolchain"] = tomllib.load(stream)["toolchain"]["channel"]
        campaign.run("project-rustc-version", ["rustc", "--version", "--verbose"], cwd=ROOT, cap=15)
        campaign.run("project-cargo-version", ["cargo", "--version", "--verbose"], cwd=ROOT, cap=15)
        campaign.run("toolchains-before", ["rustup", "toolchain", "list"], cwd=ROOT, cap=15)
        campaign.save()
        campaign.scan(tracked)
    except (StageFailure, OSError, ValueError, KeyError, TypeError, zipfile.BadZipFile) as error:
        campaign.summary.update(status="incomplete", reason=str(error))
    finally:
        for name in ("codeql-database.yml", "diagnostic"):
            path = work / "database-rust" / name
            if path.is_dir():
                shutil.copytree(path, evidence / name, dirs_exist_ok=True)
            elif path.is_file():
                shutil.copyfile(path, evidence / name)
        if tracked:
            try:
                campaign.run("toolchains-after", ["rustup", "toolchain", "list"], cwd=ROOT, cap=15)
            except StageFailure as error:
                campaign.summary.update(status="incomplete", toolchain_readback_error=str(error))
            after = snapshot(tracked)
            write_json(evidence / "tracked-after.json", after)
            changed = [name for name in tracked if before.get(name) != after[name]]
            campaign.summary.update(changed_tracked_files=changed,
                                    lockfile_after_sha256=after.get("Cargo.lock"))
            if changed:
                campaign.summary.update(status="incomplete", reason="tracked source changed during diagnostics")
        campaign.collect_diagnostics()
        status = campaign.summary["status"]
        campaign.summary.update(finished=now(), elapsed_seconds=round(time.monotonic() - campaign.started, 3),
                                exit_code=2 if status in ("not_run", "incomplete") else
                                0 if status == "completed_no_reported_candidates" else 1)
        campaign.save()
        print(json.dumps({"evidence": str(evidence), "status": status,
                          "exit_code": campaign.summary["exit_code"]}, indent=2), flush=True)
    return campaign.summary["exit_code"]


if __name__ == "__main__":
    sys.exit(main())
