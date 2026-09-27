"""Detect an unfinished conformance declaration in a compiled disposable server."""

import json
import os
from pathlib import Path
import runpy
import sys
import tempfile

from database_harness import HarnessError
from test_discovery import FINAL, build_proof, require, run_binary, validate_output


ROOT = Path(__file__).resolve().parents[1]
HELPERS = runpy.run_path(str(ROOT / "scripts/test-validation-failures.py"))
SOURCE = Path("crates/glaux-server/src/discovery.rs")


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
        log = evidence / ("discovery-" + phase + ".log")
        log.write_text(output)
        row = {"phase": phase, "log": log.name, **facts}
        results.append(row)
        (evidence / "discovery-failure-controls.json").write_text(json.dumps(results, indent=2))
        print(json.dumps(row), flush=True)

    target = ROOT / "target"
    with tempfile.TemporaryDirectory(prefix="glaux-discovery-controls-", dir=runner_temp) as directory:
        baseline = Path(directory) / "baseline"
        faulty = Path(directory) / "unfinished-class"
        HELPERS["copy_source"](files, ROOT, baseline)
        output = run_binary(*build_proof(baseline, target))
        validate_output(output)
        record("baseline", output, passed=True)
        HELPERS["copy_source"](files, baseline, faulty)
        path = faulty / SOURCE
        source = path.read_text()
        old = "const DECLARED_CLASSES: &[&str] = &[]; // DISCOVERY_DECLARATION"
        require(source.count(old) == 1, "Discovery declaration fault target changed")
        path.write_text(source.replace(old,
            'const DECLARED_CLASSES: &[&str] = &["http://www.opengis.net/spec/ogcapi-connectedsystems-1/1.0/conf/api-common"]; // DISCOVERY_DECLARATION'))
        try:
            binaries = build_proof(faulty, target)
            try:
                output = run_binary(*binaries)
            except HarnessError as error:
                output = str(error)
                detected = (
                    "Docker exec failed (101)" in output
                    and "unfinished conformance class advertised" in output
                    and "panicked at" in output
                    and "Discovery group passed: independent-wire-oracle-controls" in output
                    and "Discovery group passed: root-navigation-and-honest-declarations" not in output
                    and FINAL not in output
                )
            else:
                detected = False
            record("unfinished-class", output, detected=detected)
            require(detected, "Unfinished declaration escaped or failed for another reason: " + output)
        finally:
            require((ROOT / SOURCE).read_bytes() == original, "Real discovery source was modified")
            build_proof(ROOT, target)
        output = run_binary(target / "debug/glaux-server",
                            target / "debug/examples/discovery-proof",
                            target / "debug/examples/discovery-schema-proof")
        validate_output(output)
        record("restored", output, passed=True)
    print("Discovery failure control: 1 detected; 0 escaped.", flush=True)


if __name__ == "__main__":
    main()
