"""A wrong configured origin must fail independent HTTP assertions, not compilation."""

import json
import os
from pathlib import Path
import runpy
import sys
import tempfile

from test_http_boundary import FINAL, build_proof, require, run_binary, validate_output


ROOT = Path(__file__).resolve().parents[1]
HELPERS = runpy.run_path(str(ROOT / "scripts/test-validation-failures.py"))
SOURCE = Path("crates/glaux-server/src/http_boundary.rs")


def main():
    require(Path.cwd().resolve() == ROOT and not sys.argv[1:],
            "Run from the workspace root without target or selection overrides")
    runner_temp = Path(os.environ["RUNNER_TEMP"]).resolve()
    evidence = runner_temp / "glaux-ci-evidence"
    evidence.mkdir(exist_ok=True)
    files = HELPERS["source_files"]()
    original = (ROOT / SOURCE).read_bytes()
    results = []

    def record(phase, output, **facts):
        log = evidence / ("http-boundary-" + phase + ".log")
        log.write_text(output)
        row = {"phase": phase, "log": log.name, **facts}
        results.append(row)
        (evidence / "http-boundary-failure-controls.json").write_text(json.dumps(results, indent=2))
        print(json.dumps(row), flush=True)

    def passing(proof, phase):
        result = run_binary(proof)
        output = result.stdout + result.stderr
        record(phase, output, exit=result.returncode, passed=result.returncode == 0)
        require(result.returncode == 0, "Unmodified HTTP proof failed: " + output)
        validate_output(output)

    target = ROOT / "target"
    with tempfile.TemporaryDirectory(prefix="glaux-http-controls-", dir=runner_temp) as directory:
        baseline = Path(directory) / "baseline"
        faulty = Path(directory) / "wrong-configured-origin"
        HELPERS["copy_source"](files, ROOT, baseline)
        passing(build_proof(baseline, target), "baseline")
        HELPERS["copy_source"](files, baseline, faulty)
        path = faulty / SOURCE
        source = path.read_text()
        old = "let mut result = self.public_api_root.clone().ok_or_else(Problem::internal)?;"
        require(source.count(old) == 1, "Configured-origin fault target changed")
        path.write_text(source.replace(old, old + '\n        result.clear();\n        result.push_str("https://attacker.invalid/");'))
        try:
            proof = build_proof(faulty, target)
            result = run_binary(proof)
            output = result.stdout + result.stderr
            detected = (
                result.returncode == 101
                and "configured origin changed or escaping lost" in output
                and "panicked at" in output
                and "HTTP boundary group passed: bounded-bodies-headers-paths-and-timeouts" in output
                and "HTTP boundary group passed: configured-origin-link-isolation" not in output
                and FINAL not in output
            )
            record("wrong-configured-origin", output, exit=result.returncode, detected=detected)
            require(detected, "Wrong origin escaped or failed for another reason: " + output)
        finally:
            require((ROOT / SOURCE).read_bytes() == original, "Real HTTP source was modified")
            build_proof(ROOT, target)
        passing(target / "debug/examples/http-boundary-proof", "restored")
    print("HTTP boundary failure control: 1 detected; 0 escaped.", flush=True)


if __name__ == "__main__":
    main()
