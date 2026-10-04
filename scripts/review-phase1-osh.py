"""Temporary authorized Phase 1 Step 5 entry point; DO NOT MERGE."""

import hashlib
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import time
import traceback

from review_phase1_osh_cases import GROUPS, controls, run
from review_phase1_osh_setup import Runtime


ROOT = Path(__file__).resolve().parents[1]


def tracked_hashes():
    listing = subprocess.check_output(["git", "ls-files", "-z"], cwd=ROOT, timeout=10)
    result = {}
    for encoded in listing.split(b"\0"):
        if encoded:
            name = encoded.decode("utf-8")
            result[name] = hashlib.sha256((ROOT / name).read_bytes()).hexdigest()
    return result


def main():
    if (sys.argv[1:] or Path.cwd().resolve() != ROOT or sys.platform != "linux"
            or os.environ.get("GITHUB_ACTIONS") != "true"
            or os.environ.get("RUNNER_ENVIRONMENT") != "github-hosted"):
        raise RuntimeError("Run only without overrides in the authorized GitHub-hosted Linux checkout")
    started = time.time()
    lane_start = int(os.environ["GLAUX_OSH_LANE_STARTED"])
    # The first setup attempt used 116.735 seconds; this sole retry must keep
    # both diagnostics within the same 1,200-second aggregate allowance.
    budget = min(1083, lane_start + 1800 - 180 - started)
    output = Path(os.environ["RUNNER_TEMP"]) / "glaux-ci-evidence" / "phase1-osh"
    output.mkdir(parents=True, exist_ok=False)
    summary = {
        "status": "started", "head": os.environ["TESTED_HEAD"],
        "started_unix": started, "allowed_seconds": budget,
        "limit_seconds": 1200, "prior_attempt_seconds": 116.735,
        "attempt": "sole-diagnosed-setup-retry", "request_cap": 60, "retry_automated": False,
        "normalizations": ["JSON member order", "per-server base/local ID mapping, with exact per-server identity checks", "Sensor and Platform: exact sosa CURIE/full http URI pair only; Glaux preserves submitted spelling"],
        "limits": ["selected fields, not complete schema or Annex A validation", "no auth parity", "ordinary restart, not crash/backup recovery", "no production or peer modification"],
        "groups": [{"peer": peer, "number": i, "name": name, "status": "unrun"}
                   for peer in ("glaux", "osh") for i, name in enumerate(GROUPS, 1)],
    }
    def write():
        (output / "summary.json").write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
    def expired(_signal, _frame):
        raise TimeoutError("Total diagnostic execution deadline reached; cleanup reserve begins")
    write()
    exit_code = 1
    original_sources = None
    try:
        if budget < 180:
            raise RuntimeError("Insufficient remaining lane time; comparison not executed")
        signal.signal(signal.SIGALRM, expired)
        signal.setitimer(signal.ITIMER_REAL, budget - 120)
        original_sources = tracked_hashes()
        (output / "glaux-sources-before.json").write_text(json.dumps(original_sources, indent=2) + "\n", encoding="utf-8")
        checked = controls()
        (output / "checker-controls.json").write_text(json.dumps(checked, indent=2) + "\n", encoding="utf-8")
        summary["checker_controls"] = "passed"
        write()
        with Runtime(output, time.monotonic() + budget - 120) as runtime:
            rows = run(runtime, output)
            expected_counts = {1: 1, 2: 2, 3: 3, 4: 2, 5: 2, 6: 4, 7: 2, 8: 3}
            for group in summary["groups"]:
                members = [r for r in rows if r["peer"] == group["peer"] and r["group"] == group["number"]]
                group["cases"] = len(members)
                complete = (len(members) == expected_counts[group["number"]]
                            and len({r["case"] for r in members}) == len(members)
                            and all(r["status"] == "accounted" for r in members))
                group["status"] = "accounted" if complete else "incomplete-or-unresolved"
            summary["status"] = "accounted" if all(g["status"] == "accounted" for g in summary["groups"]) else "incomplete-or-unresolved"
        if summary["status"] == "accounted":
            exit_code = 0
    except BaseException as error:
        summary["status"] = "setup-or-execution-failed"
        summary["error"] = f"{type(error).__name__}: {error}"
        (output / "exception.txt").write_text(traceback.format_exc(), encoding="utf-8")
        # Retain completed case observations even if a later operation/cleanup failed.
        case_file = output / "case-results.json"
        if case_file.exists():
            summary["partial_case_evidence"] = "case-results.json"
        print(summary["error"], flush=True)
    finally:
        signal.setitimer(signal.ITIMER_REAL, 0)
        if original_sources is not None:
            try:
                unchanged = tracked_hashes() == original_sources
                summary["original_glaux_sources_and_corpus_unchanged"] = unchanged
                if not unchanged:
                    summary["status"] = "source-drift"
                    exit_code = 1
            except Exception as error:
                summary["source_recheck_error"] = str(error)
                summary["status"] = "source-recheck-failed"
                exit_code = 1
        summary["elapsed_seconds"] = round(time.time() - started, 3)
        if summary["elapsed_seconds"] > budget:
            summary["status"] = "deadline-exceeded"
            exit_code = 1
        write()
        records = []
        for path in sorted(output.rglob("*")):
            if path.is_file() and path.name != "evidence-manifest.json":
                data = path.read_bytes()
                records.append({"path": path.relative_to(output).as_posix(), "bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()})
        (output / "evidence-manifest.json").write_text(json.dumps(records, indent=2) + "\n", encoding="utf-8")
        print(json.dumps(summary, indent=2), flush=True)
    return exit_code


if __name__ == "__main__":
    sys.exit(main())
