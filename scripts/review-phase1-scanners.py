"""Temporary Phase 1 diagnostic: approved hosted runner only; never merge this pilot.

Produces ordinary artifacts, NOT GitHub Security uploads. No dependency fixes,
repository settings calls, added Actions, operational credentials or local use.
The controlling proposal is planning Implementation-Reviews/Phase-1/evidence/04.
Exit 0 means completed with no reported candidates, NOT security certification;
1 means candidates/diagnostics need review; 2 means incomplete/tool/setup failure.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import signal
import subprocess
import sys
import tempfile
import time
import zipfile
from datetime import datetime, timezone


BASE = "27955c1b9260cd811ad6bc08f85feab43ad65028"
ROOT = Path(__file__).resolve().parents[1]
PINS = {
    "codeql": {
        "version": "2.27.1",
        "url": "https://github.com/github/codeql-action/releases/download/"
               "codeql-bundle-v2.27.1/codeql-bundle-linux64.tar.gz",
        "sha256": "1d380f79896ededc654c7b21fafb3360136f1aeb678ad4df4df9af3910c6b815",
        "release": "https://github.com/github/codeql-action/releases/tag/codeql-bundle-v2.27.1",
        "digest_source": "https://api.github.com/repos/github/codeql-action/releases/tags/codeql-bundle-v2.27.1",
        "license": "GitHub CodeQL Terms and Conditions (CLI); MIT (query source); bundled notices apply",
        "license_source": "https://github.com/github/codeql-cli-binaries/blob/v2.27.1/LICENSE.md",
        "query_source": "https://github.com/github/codeql/tree/6e9f9e38390175c41b99070a423c875f450759ca",
        "query_license_source": "https://github.com/github/codeql/blob/6e9f9e38390175c41b99070a423c875f450759ca/LICENSE",
        "packs": {"rust": "0.1.43", "python": "1.8.11", "actions": "0.6.36"},
    },
    "cargo_audit": {
        "version": "0.22.2",
        "url": "https://github.com/rustsec/rustsec/releases/download/cargo-audit/v0.22.2/"
               "cargo-audit-x86_64-unknown-linux-gnu-v0.22.2.tgz",
        "sha256": "ab28a1bdb54db4d5d8ad5981cf1f959410370b3d28250dbd35f6a44248620e39",
        "release": "https://github.com/rustsec/rustsec/releases/tag/cargo-audit/v0.22.2",
        "digest_source": "https://api.github.com/repos/rustsec/rustsec/releases/tags/cargo-audit%2Fv0.22.2",
        "source": "https://github.com/rustsec/rustsec/tree/281452c35cf0870969042374110f099a411bc185/cargo-audit",
        "license": "Apache-2.0 OR MIT",
        "license_source": "https://github.com/rustsec/rustsec/blob/281452c35cf0870969042374110f099a411bc185/cargo-audit/LICENSE-MIT",
    },
    "rustsec": {
        "url": "https://github.com/RustSec/advisory-db.git",
        "pin_policy": "Fetch once; record actual commit and commit/retrieval dates, then audit with --no-fetch.",
        "license": "CC0-1.0 (advisory data)",
        "license_source": "https://github.com/RustSec/advisory-db/blob/main/LICENSE.txt",
    },
}


def now():
    return datetime.now(timezone.utc).isoformat()


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def write_json(path, value):
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")


class StageFailure(Exception):
    pass


class Campaign:
    def __init__(self, evidence, work, budget):
        self.evidence, self.work = evidence, work
        self.deadline = time.monotonic() + budget
        self.env = os.environ.copy()
        # Explicit languages and local query packs need no GitHub credentials.
        for name in ("GITHUB_TOKEN", "GH_TOKEN", "CODEQL_REGISTRIES_AUTH", "GITHUB_AUTH_TOKEN"):
            self.env.pop(name, None)
        self.summary = {
            "started": now(), "baseline": BASE, "budget_seconds": budget,
            "pins": PINS, "commands": [],
            "scans": {name: {"status": "not_run"} for name in ("cargo_audit", "rust", "python", "actions")},
            "native_github_security_settings": "unknown; previous unauthenticated reads returned 401",
            "limitations": [
                "Default query suites and known Rust advisories are not a complete security review.",
                "No container, renderer, toolchain, exploitability or standards-conformance claim.",
                "No native Security upload, automatic fix, dependency update or settings change.",
                "CodeQL source archives/diagnostics require review before interpreting empty findings.",
                "RustSec advisory data is captured at run time, not an assertion about Dependabot alerts.",
                "Yanked-package registry lookup is excluded; RustSec advisories/warnings are retained.",
            ],
        }
        self.save()

    def save(self):
        write_json(self.evidence / "scanner-summary.json", self.summary)

    def run(self, name, args, *, cwd=None, cap=120, allow=(0,)):
        remaining = self.deadline - time.monotonic()
        record = {"name": name, "argv": [str(arg) for arg in args], "started": now(),
                  "cwd": str(cwd or self.work), "status": "not_run"}
        self.summary["commands"].append(record)
        self.save()
        if remaining <= 2:
            record["reason"] = "campaign time budget exhausted"
            self.save()
            raise StageFailure(record["reason"])
        stdout = self.evidence / (name + ".stdout.log")
        stderr = self.evidence / (name + ".stderr.log")
        record.update(stdout=stdout.name, stderr=stderr.name)
        print(f"diagnostic: {name}", flush=True)
        started = time.monotonic()
        try:
            with stdout.open("wb") as out, stderr.open("wb") as err:
                process = subprocess.Popen(args, cwd=cwd or self.work, env=self.env,
                                           stdout=out, stderr=err, start_new_session=True)
                try:
                    code = process.wait(timeout=min(cap, remaining - 1))
                    record.update(status="completed", exit_code=code)
                except subprocess.TimeoutExpired:
                    os.killpg(process.pid, signal.SIGKILL)
                    process.wait()
                    record.update(status="timed_out", exit_code=process.returncode)
        except OSError as error:
            record.update(status="setup_failed", error=str(error))
        record.update(finished=now(), elapsed_seconds=round(time.monotonic() - started, 3))
        self.save()
        if record["status"] != "completed" or record["exit_code"] not in allow:
            raise StageFailure(f"{name}: {record['status']}, exit={record.get('exit_code')}")
        return record["exit_code"], stdout

    def download(self, name):
        pin = PINS[name]
        archive = self.work / (name + ".tar.gz")
        self.run(name + "-download", ["curl", "--fail", "--location", "--silent", "--show-error",
                 "--proto", "=https", "--tlsv1.2", "--connect-timeout", "20", "--max-time", "180",
                 "--output", str(archive), pin["url"]], cap=185)
        actual = digest(archive)
        self.summary.setdefault("downloads", {})[name] = {"sha256": actual, "bytes": archive.stat().st_size}
        self.save()
        if actual != pin["sha256"]:
            raise StageFailure(f"{name}: released archive checksum mismatch")
        destination = self.work / name
        destination.mkdir()
        # Only the checksum-verified official archive is extracted, into this new owned directory.
        self.run(name + "-extract", ["tar", "--extract", "--gzip", "--file", str(archive),
                 "--directory", str(destination), "--no-same-owner", "--no-same-permissions"], cap=90)
        return destination

    def audit(self, lockfile):
        result = self.summary["scans"]["cargo_audit"]
        try:
            directory = self.download("cargo_audit")
            matches = [path for path in directory.rglob("cargo-audit") if path.is_file()]
            if len(matches) != 1:
                raise StageFailure("cargo-audit executable not uniquely present in pinned archive")
            binary = matches[0]
            _, output = self.run("cargo-audit-version", [str(binary), "--version"], cap=10)
            if output.read_text().strip() != "cargo-audit " + PINS["cargo_audit"]["version"]:
                raise StageFailure("unexpected cargo-audit executable version")
            database = self.work / "rustsec-advisory-db"
            self.run("rustsec-fetch", ["git", "clone", "--depth", "1", PINS["rustsec"]["url"],
                     str(database)], cap=75)
            _, output = self.run("rustsec-pin", ["git", "-C", str(database), "show", "-s",
                                               "--format=%H%n%cI", "HEAD"], cap=10)
            revision, committed = output.read_text().strip().splitlines()
            if not re.fullmatch(r"[0-9a-f]{40}", revision):
                raise StageFailure("malformed advisory database revision")
            result.update(database_commit=revision, database_commit_date=committed,
                          database_retrieved=now(), lockfile_sha256=digest(lockfile))
            self.save()
            code, output = self.run("cargo-audit-results", [str(binary), "audit", "--file", str(lockfile),
                "--db", str(database), "--no-fetch", "--no-yanked", "--json"], cap=120, allow=(0, 1))
            report = json.loads(output.read_text())
            vulnerabilities = report["vulnerabilities"]
            if not isinstance(vulnerabilities["list"], list) or not isinstance(report["warnings"], dict):
                raise StageFailure("cargo-audit JSON shape differs from expected report")
            count = len(vulnerabilities["list"])
            warnings = sum(len(items) for items in report["warnings"].values())
            if vulnerabilities["count"] != count or (code == 1 and count + warnings == 0):
                raise StageFailure("cargo-audit return code/report count disagreement")
            result.update(status="reported_candidates" if count + warnings else "completed_no_reported_candidates",
                          vulnerability_count=count, warning_count=warnings, exit_code=code,
                          applicability="requires review; advisory match is not exploitability proof")
            write_json(self.evidence / "cargo-audit-results.json", report)
        except (StageFailure, OSError, ValueError, KeyError, TypeError) as error:
            result.update(status="incomplete", reason=str(error))
        self.save()

    def codeql(self, tracked):
        try:
            directory = self.download("codeql")
            binary = directory / "codeql" / "codeql"
            _, output = self.run("codeql-version", [str(binary), "version", "--format=json"], cap=15)
            version = json.loads(output.read_text())
            if version.get("version") != PINS["codeql"]["version"]:
                raise StageFailure("unexpected CodeQL executable version")
            self.summary["codeql_version"] = version
            self.run("codeql-languages", [str(binary), "resolve", "languages", "--format=json"], cap=15)
        except (StageFailure, OSError, ValueError) as error:
            for language in PINS["codeql"]["packs"]:
                self.summary["scans"][language].update(status="not_run", reason=f"CodeQL setup: {error}")
            self.save()
            return
        for language, pack_version in PINS["codeql"]["packs"].items():
            result = self.summary["scans"][language]
            try:
                suites = list(directory.rglob(f"codeql-suites/{language}-code-scanning.qls"))
                if len(suites) != 1:
                    raise StageFailure(f"expected one bundled default {language} suite, found {len(suites)}")
                suite = suites[0]
                manifest = suite.parent.parent / "qlpack.yml"
                manifest_text = manifest.read_text()
                if not re.search(rf"^version:\s*{re.escape(pack_version)}\s*$", manifest_text, re.M):
                    raise StageFailure(f"{language} bundled query-pack version mismatch")
                if not re.search(rf"^name:\s*codeql/{language}-queries\s*$", manifest_text, re.M):
                    raise StageFailure(f"{language} bundled query-pack name mismatch")
                result.update(query_pack=f"codeql/{language}-queries@{pack_version}",
                              suite_sha256=digest(suite), suite=str(suite.relative_to(directory)))
                self.run(language + "-queries", [str(binary), "resolve", "queries", str(suite),
                         "--format=json"], cap=30)
                database = self.work / ("database-" + language)
                logs = self.evidence / ("codeql-" + language + "-logs")
                logs.mkdir()
                create = [str(binary), "database", "create", str(database), "--language=" + language,
                          "--source-root=" + str(ROOT), "--threads=2", "--ram=4096",
                          "--logdir=" + str(logs)]
                # Rust 'none' still executes build.rs and compiles procedural macros.
                if language == "rust":
                    create.append("--build-mode=none")
                self.run(language + "-extract", create, cap=300 if language == "rust" else 150)
                archives = list(database.glob("src.zip"))
                if len(archives) != 1:
                    raise StageFailure(f"{language}: source archive unavailable; coverage not established")
                with zipfile.ZipFile(archives[0]) as archive:
                    archived = sorted(name for name in archive.namelist() if not name.endswith("/"))
                expected = [name for name in tracked if
                            (language == "rust" and name.endswith(".rs")) or
                            (language == "python" and name.endswith(".py")) or
                            (language == "actions" and name.startswith(".github/workflows/") and
                             name.endswith((".yml", ".yaml")))]
                # Source archive names include absolute-source prefixes; retain exact names too.
                matched = [name for name in expected if any(item == name or item.endswith("/" + name)
                                                            for item in archived)]
                missing = sorted(set(expected) - set(matched))
                write_json(self.evidence / (language + "-source-coverage.json"), {
                    "tracked_candidates": expected, "matched_source_archive": matched,
                    "unmatched_tracked_candidates": missing, "source_archive_entries": archived,
                    "limit": "Archive presence is extraction evidence, not proof every path/query was modeled.",
                })
                result.update(archived_tracked_files=len(matched), unmatched_tracked_files=len(missing))
                if not matched:
                    raise StageFailure(f"{language}: no tracked language source matched extracted archive")
                sarif = self.evidence / (language + ".sarif")
                self.run(language + "-analyze", [str(binary), "database", "analyze", str(database),
                         str(suite), "--format=sarif-latest", "--output=" + str(sarif),
                         "--sarif-category=phase1-diagnostic/" + language, "--threads=2", "--ram=4096",
                         "--logdir=" + str(logs)], cap=180)
                report = json.loads(sarif.read_text())
                runs = report.get("runs", [])
                if report.get("version") != "2.1.0" or not runs:
                    raise StageFailure(f"{language}: missing/unexpected SARIF runs")
                notifications = []
                count = 0
                for run in runs:
                    if not run.get("tool", {}).get("driver", {}).get("rules"):
                        raise StageFailure(f"{language}: SARIF contains no query-rule inventory")
                    count += len(run.get("results", []))
                    for invocation in run.get("invocations", []):
                        if invocation.get("executionSuccessful") is False:
                            raise StageFailure(f"{language}: SARIF reports unsuccessful execution")
                        notifications.extend(invocation.get("toolExecutionNotifications", []))
                        notifications.extend(invocation.get("toolConfigurationNotifications", []))
                problems = [item for item in notifications if item.get("level") in ("error", "warning")]
                result.update(reported_results=count, diagnostic_notifications=len(problems),
                              sarif=sarif.name, sarif_sha256=digest(sarif),
                              coverage_review_required=bool(missing),
                              status="reported_candidates" if count else
                                     "completed_with_coverage_or_diagnostic_questions" if missing or problems else
                                     "completed_no_reported_candidates")
            except (StageFailure, OSError, ValueError, KeyError, TypeError, zipfile.BadZipFile) as error:
                result.update(status="incomplete", reason=str(error))
            self.save()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--approved-ci", action="store_true")
    parser.add_argument("--budget-seconds", type=int, default=1200)
    args = parser.parse_args()
    if not (args.approved_ci and os.environ.get("GITHUB_ACTIONS") == "true" and
            os.environ.get("GITHUB_REPOSITORY") == "DGIWG-P507/glaux-server" and
            os.environ.get("GITHUB_EVENT_NAME") == "pull_request" and platform.system() == "Linux" and
            platform.machine() == "x86_64" and 30 <= args.budget_seconds <= 1200):
        raise SystemExit("Refusing: approved public-repository pull-request Linux runner and bounded budget required.")
    runner_temp = Path(os.environ["RUNNER_TEMP"]).resolve(strict=True)
    evidence = runner_temp / "glaux-ci-evidence" / "phase1-scanners"
    evidence.mkdir(parents=True, exist_ok=False)
    work = Path(tempfile.mkdtemp(prefix="glaux-phase1-scanners-", dir=runner_temp))
    campaign = Campaign(evidence, work, args.budget_seconds)
    lockfile = ROOT / "Cargo.lock"
    before = digest(lockfile)
    try:
        _, output = campaign.run("source-head", ["git", "rev-parse", "HEAD"], cwd=ROOT, cap=10)
        campaign.summary["head"] = output.read_text().strip()
        # Checkout is intentionally shallow. Fetch only the reviewed public
        # baseline object; do not move HEAD or require persisted credentials.
        campaign.run("fetch-reviewed-baseline", ["git", "fetch", "--no-tags", "--depth=1",
                     "https://github.com/DGIWG-P507/glaux-server.git", BASE], cwd=ROOT, cap=45)
        campaign.run("unchanged-production", ["git", "diff", "--exit-code", BASE, "--", "Cargo.lock",
                     "Cargo.toml", "rust-toolchain.toml", "crates", "corpus", "fuzz"], cwd=ROOT, cap=15)
        _, output = campaign.run("tracked-files", ["git", "ls-files"], cwd=ROOT, cap=10)
        tracked = output.read_text().splitlines()
        campaign.summary["lockfile_before_sha256"] = before
        campaign.save()
        # Advisory findings do not suppress the independent source scans.
        campaign.audit(lockfile)
        campaign.codeql(tracked)
    except (StageFailure, OSError, ValueError) as error:
        campaign.summary["campaign_error"] = str(error)
    finally:
        campaign.summary["lockfile_after_sha256"] = digest(lockfile) if lockfile.exists() else None
        if campaign.summary["lockfile_after_sha256"] != before:
            campaign.summary["campaign_error"] = "Cargo.lock changed or disappeared during diagnostics"
        campaign.summary["finished"] = now()
        statuses = [scan["status"] for scan in campaign.summary["scans"].values()]
        incomplete = "campaign_error" in campaign.summary or any(status in ("not_run", "incomplete") for status in statuses)
        needs_review = any(status != "completed_no_reported_candidates" for status in statuses)
        campaign.summary["exit_code"] = 2 if incomplete else 1 if needs_review else 0
        campaign.save()
        print(json.dumps({"evidence": str(evidence), "scans": campaign.summary["scans"],
                          "exit_code": campaign.summary["exit_code"]}, indent=2), flush=True)
    return campaign.summary["exit_code"]


if __name__ == "__main__":
    sys.exit(main())
