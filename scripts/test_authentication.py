"""Independent OpenSSL signatures and raw HTTP assertions on an owned listener."""

import base64
import hashlib
import hmac
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile


ROOT = Path(__file__).resolve().parents[1]
GROUPS = (
    "independent-wire-oracle-controls",
    "verified-caller-context",
    "signature-profile-and-claim-rejections",
    "exact-validity-clock-and-scope",
    "header-framing-and-no-disclosure",
    "explicit-loopback-development-boundary",
    "bounded-generated-input-and-clean-shutdown",
)
FINAL = "Required authentication proof passed: 7 groups."


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def openssl(arguments, data=None):
    result = subprocess.run(["openssl", *arguments], input=data,
                            capture_output=True, timeout=30, check=False)
    require(result.returncode == 0, "Synthetic fixture OpenSSL operation failed")
    return result.stdout


def b64(value):
    return base64.urlsafe_b64encode(value).rstrip(b"=").decode("ascii")


def der_item(data, position):
    require(position + 2 <= len(data), "Truncated public RSA DER")
    tag, size = data[position], data[position + 1]
    start = position + 2
    if size & 128:
        count = size & 127
        require(0 < count <= 4 and start + count <= len(data), "Invalid DER length")
        size = int.from_bytes(data[start:start + count], "big")
        start += count
    end = start + size
    require(end <= len(data), "Truncated public RSA value")
    return tag, data[start:end], end


def fixtures(directory):
    """Only public configuration and synthetic signed tokens leave this directory."""
    version = openssl(["version"]).decode("ascii").strip()
    print("Authentication fixture signer: " + version, flush=True)
    key = directory / "synthetic-private.pem"
    wrong_key = directory / "synthetic-wrong-private.pem"
    for target in (key, wrong_key):
        openssl(["genpkey", "-algorithm", "RSA", "-pkeyopt", "rsa_keygen_bits:2048",
                 "-out", str(target)])
        target.chmod(0o600)
    public = openssl(["rsa", "-in", str(key), "-RSAPublicKey_out", "-outform", "DER"])
    tag, sequence, end = der_item(public, 0)
    require(tag == 48 and end == len(public), "Expected exact RSA public sequence")
    n_tag, modulus, offset = der_item(sequence, 0)
    e_tag, exponent, end = der_item(sequence, offset)
    require(n_tag == 2 and e_tag == 2 and end == len(sequence), "Expected public n/e")
    jwk = {"kty": "RSA", "kid": "fixture-key", "alg": "RS256", "use": "sig",
           "n": b64(modulus.lstrip(b"\x00")), "e": b64(exponent.lstrip(b"\x00"))}
    base = {"iss": "https://issuer.example.test", "aud": "https://api.example.test",
            "sub": "fixture-alice", "client_id": "fixture-client", "jti": "fixture-token",
            "iat": 1699999990, "nbf": 1699999990, "exp": 1700000600,
            "scope": "read write", "groups": ["group-a", "group-b"]}
    header = {"alg": "RS256", "typ": "at+jwt", "kid": "fixture-key"}
    tokens = {}

    def sign(name, claims=None, headers=None, payload_bytes=None, header_bytes=None, signing_key=key):
        payload = payload_bytes if payload_bytes is not None else json.dumps(
            base if claims is None else claims, separators=(",", ":")).encode()
        protected = header_bytes if header_bytes is not None else json.dumps(
            header if headers is None else headers, separators=(",", ":")).encode()
        signing = (b64(protected) + "." + b64(payload)).encode("ascii")
        signature = openssl(["dgst", "-sha256", "-sign", str(signing_key)], signing)
        tokens[name] = signing.decode("ascii") + "." + b64(signature)

    sign("valid")
    sign("valid-media-type", headers={**header, "typ": "application/at+jwt"})
    sign("valid-case-type", headers={**header, "typ": "AT+JWT"})
    sign("valid-b64-true", headers={**header, "b64": True})
    sign("valid-aud-array", claims={**base, "aud": ["other-api", base["aud"]]})
    sign("valid-extension", claims={**base, "unfamiliar": {"value": 17}})
    sign("valid-deduplicated", claims={**base, "scope": "read read write",
                                      "groups": ["group-a", "group-a", "group-b"]})
    without_nbf = base.copy()
    del without_nbf["nbf"]
    sign("valid-no-not-before", claims=without_nbf)
    sign("bad-signature", signing_key=wrong_key)
    for name, field, value in (
        ("wrong-issuer", "iss", "https://wrong.example.test"),
        ("wrong-audience", "aud", "https://wrong-api.example.test"),
        ("audience-empty", "aud", []), ("audience-mixed", "aud", [base["aud"], 17]),
        ("subject-type", "sub", 17), ("expiry-type", "exp", "1700000600"),
        ("expired", "exp", 1699999999), ("future-not-before", "nbf", 1700000001),
        ("future-issued", "iat", 1700000001), ("expiry-before-issued", "exp", 1699999980),
        ("missing-scope", "scope", "write"), ("scope-type", "scope", ["read"]),
        ("groups-type", "groups", "group-a"), ("groups-mixed", "groups", ["group-a", 17]),
        ("private-number-marker", "exp", {"$serde_json::private::Number": "1700000600"}),
        ("groups-limit", "groups", ["group-" + str(index) for index in range(65)]),
        ("scope-limit", "scope", "read " + " ".join("scope" + str(index) for index in range(64))),
    ):
        sign(name, claims={**base, field: value})
    for field in ("iss", "sub", "aud", "exp", "iat", "client_id", "jti"):
        claims = base.copy()
        del claims[field]
        sign("missing-" + field, claims=claims)
    for name, field, value in (
        ("id-token", "typ", "JWT"), ("unknown-algorithm", "alg", "RS512"),
        ("unknown-key", "kid", "unknown-key"),
        ("unsupported-critical", "crit", ["new-critical"]), ("unencoded-payload", "b64", False),
    ):
        sign(name, headers={**header, field: value})
    missing_type = header.copy()
    del missing_type["typ"]
    sign("missing-type", headers=missing_type)
    missing_kid = header.copy()
    del missing_kid["kid"]
    sign("missing-key-id", headers=missing_kid)
    for field, value in (("jku", "https://untrusted.example.test/keys"),
                         ("x5u", "https://untrusted.example.test/cert"),
                         ("jwk", jwk), ("x5c", ["synthetic-certificate"])):
        sign("request-key-" + field, headers={**header, field: value})
    compact = json.dumps(base, separators=(",", ":"))
    # Both duplicate values are otherwise valid: last-wins parsing cannot pass this check.
    sign("duplicate-claim", payload_bytes=(compact[:-1] + ',"aud":"https://api.example.test"}').encode())
    sign("duplicate-header", header_bytes=b'{"alg":"RS256","typ":"at+jwt","kid":"fixture-key","alg":"RS256"}')
    sign("fraction-exp-after", payload_bytes=compact.replace('"exp":1700000600', '"exp":1700000000.500000001').encode())
    sign("fraction-exp-equal", payload_bytes=compact.replace('"exp":1700000600', '"exp":1700000000.500000000').encode())
    sign("fraction-exp-before", payload_bytes=compact.replace('"exp":1700000600', '"exp":1700000000.499999999').encode())
    for name, instant in (("fraction-nbf-equal", "1700000000.500000000"),
                          ("fraction-nbf-after", "1700000000.500000001")):
        sign(name, payload_bytes=compact.replace('"nbf":1699999990', '"nbf":' + instant).encode())
    for name, instant in (("fraction-iat-equal", "1700000000.500000000"),
                          ("fraction-iat-after", "1700000000.500000001")):
        sign(name, payload_bytes=compact.replace('"iat":1699999990', '"iat":' + instant).encode())
    unsigned = b64(json.dumps({**header, "alg": "none"}).encode()) + "." + b64(compact.encode())
    tokens["unsigned"] = unsigned + "."
    hs = (b64(json.dumps({**header, "alg": "HS256"}).encode()) + "." + b64(compact.encode())).encode()
    tokens["symmetric-confusion"] = hs.decode() + "." + b64(hmac.new(public, hs, hashlib.sha256).digest())
    protected, payload, signature = tokens["valid"].split(".")
    tokens["padded-segment"] = protected + "=." + payload + "." + signature
    alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_"
    last = alphabet.index(signature[-1])
    require(last % 16 == 0, "Expected canonical two-character tail for 256-byte signature")
    tokens["noncanonical-signature"] = protected + "." + payload + "." + signature[:-1] + alphabet[last + 1]

    def padded(value, size):
        result = {**value, "pad": ""}
        count = size - len(json.dumps(result, separators=(",", ":")).encode())
        require(count >= 0, "Requested fixture size is too small")
        result["pad"] = "x" * count
        require(len(json.dumps(result, separators=(",", ":")).encode()) == size,
                "Independent fixture byte count differs")
        return result

    sign("exact-header-limit", headers=padded(header, 2048))
    sign("over-header-limit", headers=padded(header, 2049))
    sign("exact-token-limit", headers=padded(header, 2047), claims=padded(base, 9982))
    sign("over-token-limit", headers=padded(header, 2048), claims=padded(base, 9982))
    require(len(tokens["exact-token-limit"]) == 16384, "Exact token-bound fixture differs")
    require(len(tokens["over-token-limit"]) == 16385, "Over-token-bound fixture differs")
    path = directory / "public-fixtures.json"
    public_fixture = json.dumps({"jwk": jwk, "tokens": tokens})
    require(len(public_fixture.encode()) <= 131072, "Public fixture file exceeds reader bound")
    path.write_text(public_fixture, encoding="utf-8")
    # No example process needs either private key; erase them before starting it.
    key.unlink()
    wrong_key.unlink()
    return path


def build_proof(source_root, target_directory):
    result = subprocess.run(
        ["cargo", "build", "--locked", "--offline", "-p", "glaux-server",
         "--example", "authentication-proof", "--target-dir", str(target_directory)],
        cwd=source_root, capture_output=True, text=True, timeout=180, check=False,
    )
    print(result.stdout + result.stderr, end="", flush=True)
    proof = target_directory / "debug/examples/authentication-proof"
    require(result.returncode == 0 and proof.is_file(), "Required authentication proof did not build")
    return proof


def run_binary(proof, fixture_path):
    return subprocess.run([str(proof), str(fixture_path)], capture_output=True,
                          text=True, timeout=40, check=False)


def validate_output(output):
    prefix = "Authentication group passed: "
    actual = [line for line in output.splitlines() if line.startswith(prefix)]
    require(actual == [prefix + name for name in GROUPS], "Required authentication groups missing/duplicated/reordered")
    require(output.splitlines().count(FINAL) == 1, "Authentication final marker missing/duplicated")


def main():
    require(Path.cwd().resolve() == ROOT and not sys.argv[1:],
            "Run from workspace root without target or selection overrides")
    proof = build_proof(ROOT, ROOT / "target")
    with tempfile.TemporaryDirectory(prefix="glaux-auth-fixtures-", dir=os.environ["RUNNER_TEMP"]) as directory:
        result = run_binary(proof, fixtures(Path(directory)))
        output = result.stdout + result.stderr
        print(output, end="", flush=True)
        require(result.returncode == 0, "Required authentication proof failed")
        validate_output(output)
    print("Authentication: 7 groups passed; 0 failed; 0 skipped", flush=True)
    print("Authentication: all required checks passed.", flush=True)


if __name__ == "__main__":
    main()
