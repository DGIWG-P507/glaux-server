"""A compiled audience-bypass fault must fail exact independent HTTP assertions."""

import json
import os
from pathlib import Path
import runpy
import sys
import tempfile

from test_authentication import (
    FINAL, build_proof, fixtures, require, run_binary, validate_output,
)


ROOT = Path(__file__).resolve().parents[1]
HELPERS = runpy.run_path(str(ROOT / "scripts/test-validation-failures.py"))
SOURCE = Path("crates/glaux-server/src/authentication/jwt.rs")


def main():
    require(Path.cwd().resolve() == ROOT and not sys.argv[1:],
            "Run from workspace root without target or selection overrides")
    runner_temp = Path(os.environ["RUNNER_TEMP"]).resolve()
    evidence = runner_temp / "glaux-ci-evidence"
    evidence.mkdir(exist_ok=True)
    files = HELPERS["source_files"]()
    original = (ROOT / SOURCE).read_bytes()
    results = []

    def record(phase, output, **facts):
        log = evidence / ("authentication-" + phase + ".log")
        log.write_text(output)
        row = {"phase": phase, "log": log.name, **facts}
        results.append(row)
        (evidence / "authentication-failure-controls.json").write_text(json.dumps(results, indent=2))
        print(json.dumps(row), flush=True)

    def passing(proof, fixture_path, phase):
        result = run_binary(proof, fixture_path)
        output = result.stdout + result.stderr
        record(phase, output, exit=result.returncode, passed=result.returncode == 0)
        require(result.returncode == 0, "Unmodified authentication proof failed: " + output)
        validate_output(output)

    target = ROOT / "target"
    with tempfile.TemporaryDirectory(prefix="glaux-auth-controls-", dir=runner_temp) as directory:
        owner = Path(directory)
        fixture_path = fixtures(owner)
        baseline = owner / "baseline"
        faulty = owner / "ignored-audience"
        HELPERS["copy_source"](files, ROOT, baseline)
        passing(build_proof(baseline, target), fixture_path, "baseline")
        HELPERS["copy_source"](files, baseline, faulty)
        path = faulty / SOURCE
        source = path.read_text()
        old = "if !audiences.iter().any(|value| *value == self.audience) {"
        new = "if !audiences.iter().any(|value| *value == self.audience) && token.is_empty() {"
        require(source.count(old) == 1, "Audience-comparison fault target changed")
        path.write_text(source.replace(old, new))
        try:
            proof = build_proof(faulty, target)
            result = run_binary(proof, fixture_path)
            output = result.stdout + result.stderr
            detected = (
                result.returncode == 101
                and "invalid access-token case accepted: wrong-audience" in output
                and "panicked at" in output
                and "Authentication group passed: verified-caller-context" in output
                and "Authentication group passed: signature-profile-and-claim-rejections" not in output
                and FINAL not in output
            )
            record("ignored-audience", output, exit=result.returncode, detected=detected)
            require(detected, "Audience bypass escaped or failed for another reason: " + output)
        finally:
            require((ROOT / SOURCE).read_bytes() == original, "Real authentication source changed")
            build_proof(ROOT, target)
        passing(target / "debug/examples/authentication-proof", fixture_path, "restored")
    print("Authentication failure control: 1 detected; 0 escaped.", flush=True)


if __name__ == "__main__":
    main()
