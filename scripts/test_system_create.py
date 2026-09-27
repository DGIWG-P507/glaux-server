"""Real System POST through the actual binary and owned network-isolated database."""
from pathlib import Path
import json
import os
import subprocess
import sys
import tempfile
import time
from database_harness import DisposablePostgis, HarnessError, docker
from test_authentication import b64, der_item, openssl

ROOT = Path(__file__).resolve().parents[1]
GROUPS = (
    "independent-wire-oracle-controls",
    "empty201-canonical-location-and-atomic-records",
    "malformed-media-and-no-partial-writes",
    "verified-callers-source-scope-and-safe-denials",
    "optional-retry-and-forged-context",
    "precommit-failure-and-owned-cleanup",
)
FINAL = "Required System create proof passed: 6 groups."

def require(condition, message):
    if not condition:
        raise HarnessError(message)

def build_proof(source_root, target_directory):
    result = subprocess.run(
        ["cargo", "build", "--locked", "--offline", "-p", "glaux-server",
         "--bin", "glaux-server", "--example", "system-create-proof",
         "--target-dir", str(target_directory)],
        cwd=source_root, capture_output=True, text=True, timeout=180, check=False,
    )
    print(result.stdout + result.stderr, end="", flush=True)
    server = target_directory / "debug/glaux-server"
    proof = target_directory / "debug/examples/system-create-proof"
    require(result.returncode == 0 and server.is_file() and proof.is_file(),
            "Required System create server/proof did not build")
    return server, proof

def signed_fixture(directory):
    """Reuse the independent signer, not the server's JWT implementation."""
    print("System creation fixture signer: " + openssl(["version"]).decode().strip(), flush=True)
    key = directory / "synthetic-private.pem"
    openssl(["genpkey", "-algorithm", "RSA", "-pkeyopt", "rsa_keygen_bits:2048", "-out", str(key)])
    key.chmod(0o600)
    public = openssl(["rsa", "-in", str(key), "-RSAPublicKey_out", "-outform", "DER"])
    tag, sequence, end = der_item(public, 0)
    require(tag == 48 and end == len(public), "Expected exact RSA public sequence")
    n_tag, modulus, offset = der_item(sequence, 0)
    e_tag, exponent, end = der_item(sequence, offset)
    require(n_tag == 2 and e_tag == 2 and end == len(sequence), "Expected public n/e")
    jwk = {"kty": "RSA", "kid": "create-key", "alg": "RS256", "use": "sig",
           "n": b64(modulus.lstrip(b"\x00")), "e": b64(exponent.lstrip(b"\x00"))}
    now = int(time.time())
    claims = {"iss": "https://issuer.example.test", "aud": "https://api.example.test",
              "sub": "jwt-writer", "client_id": "create-fixture", "jti": "create-token",
              "iat": now - 30, "nbf": now - 30, "exp": now + 600,
              "scope": "write", "groups": ["group-a"]}
    header = {"alg": "RS256", "typ": "at+jwt", "kid": "create-key"}
    signing = (b64(json.dumps(header, separators=(",", ":")).encode()) + "." +
               b64(json.dumps(claims, separators=(",", ":")).encode())).encode()
    signature = openssl(["dgst", "-sha256", "-sign", str(key)], signing)
    key.unlink()
    token = signing.decode() + "." + b64(signature)
    wrong = bytearray(signature)
    wrong[0] ^= 1
    fixture = directory / "public-create-fixtures.json"
    fixture.write_text(json.dumps({"jwk": jwk, "token": token,
                                  "bad_token": signing.decode() + "." + b64(wrong)}))
    require(fixture.stat().st_size < 16384, "Synthetic public fixture exceeds reader bound")
    return fixture


def run_binary(server, proof):
    with tempfile.TemporaryDirectory(prefix="glaux-create-jwt-", dir=os.environ["RUNNER_TEMP"]) as directory, DisposablePostgis() as db:
        fixture = signed_fixture(Path(directory))
        db.setup()
        for binary, name in ((server, "glaux-system-create-server"),
                             (proof, "glaux-system-create-proof")):
            db.validate_target()
            docker("cp", str(binary), db.container_id + ":/tmp/" + name)
        db.validate_target()
        docker("cp", str(fixture), db.container_id + ":/tmp/glaux-system-create-fixtures.json")
        db.validate_target()
        return docker("exec", "--user", "postgres", db.container_id,
                      "/tmp/glaux-system-create-proof", timeout=120)

def validate_output(output):
    prefix = "System create group passed: "
    actual = [line for line in output.splitlines() if line.startswith(prefix)]
    require(actual == [prefix + name for name in GROUPS],
            "Required System create groups missing, duplicated or reordered")
    require(output.splitlines().count(FINAL) == 1,
            "System create final marker missing/duplicated")

def main():
    require(Path.cwd().resolve() == ROOT and not sys.argv[1:],
            "Run from workspace root without target or selection overrides")
    output = run_binary(*build_proof(ROOT, ROOT / "target"))
    print(output, flush=True)
    validate_output(output)
    print("System create: 6 groups passed; 0 failed; 0 skipped", flush=True)
    print("System creation: all required checks passed.", flush=True)

if __name__ == "__main__":
    main()
