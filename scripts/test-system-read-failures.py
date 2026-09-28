"""Prove that the retrieval proof rejects a wrong identity and process-memory storage."""
import json
import os
from pathlib import Path
import runpy
import sys
import tempfile
from database_harness import HarnessError
from test_system_read import FINAL, build_proof, require, run_binary, validate_output

ROOT = Path(__file__).resolve().parents[1]
HELPERS = runpy.run_path(str(ROOT / "scripts/test-validation-failures.py"))
SOURCE = Path("crates/glaux-server/src/system_http.rs")
PASSED = "System read group passed: "

IDENTITY = "    let id = system.id.to_string();\n"
WRONG_IDENTITY = "    let id = LocalId::generate().map_err(|_| Problem::internal())?.to_string();\n"

LOCATION = "    let location = canonical(&state.boundary, receipt.system_id)?;\n"
REMEMBER = "    PROCESS_MEMORY.lock().unwrap().push((receipt.system_id, bytes.to_vec()));\n"
STORAGE = ("    let system = state\n"
           "        .admission\n"
           "        .current_system(&mut connection, &context, id)\n"
           "        .await?;\n")
# Serve only what this process accepted: correct until the process restarts.
RECALL = """    let system = {
        let memory = PROCESS_MEMORY.lock().unwrap();
        let (_, bytes) = memory
            .iter()
            .find(|(key, _)| *key == id)
            .ok_or_else(Problem::not_found)?;
        let value: Value = serde_json::from_slice(bytes).unwrap();
        CurrentSystem {
            id,
            uid: value["properties"]["uid"].as_str().unwrap().parse().unwrap(),
            label: value["properties"]["name"].as_str().unwrap().to_owned(),
            parent: None,
            media_type: INPUT_MEDIA.to_owned(),
            bytes: bytes.clone(),
        }
    };
"""
MEMORY = ("\nstatic PROCESS_MEMORY: std::sync::Mutex<Vec<(LocalId, Vec<u8>)>> =\n"
          "    std::sync::Mutex::new(Vec::new());\n")


def wrong_identity(source):
    require(source.count(IDENTITY) == 1, "Retrieved-identity fault target changed")
    return source.replace(IDENTITY, WRONG_IDENTITY)


def process_memory(source):
    require(source.count(LOCATION) == 1 and source.count(STORAGE) == 1,
            "Process-memory fault targets changed")
    return source.replace(LOCATION, REMEMBER + LOCATION).replace(STORAGE, RECALL) + MEMORY


# Each fault must fail its own intended assertion after the earlier groups pass.
CONTROLS = (
    ("wrong-identity", wrong_identity,
     "right after creation: retrieved System differs from its independent expectation",
     "independent-wire-oracle-controls", "root-navigation-create-and-exact-retrieval"),
    ("process-memory", process_memory,
     "after server restart: retrieved System differs from its independent expectation",
     "root-navigation-create-and-exact-retrieval", "restart-retains-identity-and-meaning"),
)


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
        log = evidence / ("system-read-" + phase + ".log")
        log.write_text(output)
        row = {"phase": phase, "log": log.name, **facts}
        results.append(row)
        (evidence / "system-read-failure-controls.json").write_text(json.dumps(results, indent=2))
        print(json.dumps(row), flush=True)

    target = ROOT / "target"
    with tempfile.TemporaryDirectory(prefix="glaux-system-read-controls-", dir=runner_temp) as directory:
        baseline = Path(directory) / "baseline"
        HELPERS["copy_source"](files, ROOT, baseline)
        output = run_binary(*build_proof(baseline, target))
        validate_output(output)
        record("baseline", output, passed=True)
        try:
            for name, mutate, message, last_passed, first_failed in CONTROLS:
                faulty = Path(directory) / name
                HELPERS["copy_source"](files, baseline, faulty)
                path = faulty / SOURCE
                path.write_text(mutate(path.read_text()))
                binaries = build_proof(faulty, target)
                try:
                    output = run_binary(*binaries)
                except HarnessError as error:
                    output = str(error)
                    detected = (
                        "Docker exec failed (101)" in output
                        and message in output
                        and "panicked at" in output
                        and PASSED + last_passed in output
                        and PASSED + first_failed not in output
                        and FINAL not in output
                    )
                else:
                    detected = False
                record(name, output, detected=detected)
                require(detected, name + " escaped or failed for another reason: " + output)
        finally:
            require((ROOT / SOURCE).read_bytes() == original, "Real System HTTP source was modified")
            build_proof(ROOT, target)
        output = run_binary(target / "debug/glaux-server", target / "debug/examples/system-read-proof")
        validate_output(output)
        record("restored", output, passed=True)
    print("System read failure controls: 2 detected; 0 escaped.", flush=True)

if __name__ == "__main__":
    main()
