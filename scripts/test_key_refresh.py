"""Owned HTTPS issuer, independent signing and deterministic refresh observations."""

from contextlib import contextmanager
import http.server
import json
import os
from pathlib import Path
import socket
import ssl
import subprocess
import sys
import tempfile
import threading
import time

from test_authentication import b64, der_item, openssl, require


ROOT = Path(__file__).resolve().parents[1]
GROUPS = (
    "initial-cache-reuse",
    "rotation-and-bounded-unknown-keys",
    "expired-trust-outage-and-recovery",
    "hostile-hints-and-bounded-responses",
    "tls-and-http-transport-failures",
    "concurrency-cancellation-and-clocks",
    "middleware-and-clean-shutdown",
)
FINAL = "Required key-refresh proof passed: 7 groups."


def public_key(path, kid):
    raw = openssl(["rsa", "-in", str(path), "-RSAPublicKey_out", "-outform", "DER"])
    tag, sequence, end = der_item(raw, 0)
    require(tag == 48 and end == len(raw), "Expected exact public-key DER")
    nt, n, offset = der_item(sequence, 0)
    et, e, end = der_item(sequence, offset)
    require(nt == et == 2 and end == len(sequence), "Expected RSA n/e")
    return {"kty": "RSA", "kid": kid, "alg": "RS256", "use": "sig",
            "n": b64(n.lstrip(b"\x00")), "e": b64(e.lstrip(b"\x00"))}


def materials(owner):
    print("Key-refresh signer: " + openssl(["version"]).decode().strip(), flush=True)
    print("Key-refresh TLS fixture: " + ssl.OPENSSL_VERSION, flush=True)
    ca_key, ca = owner / "ca.key", owner / "ca.pem"
    leaf_key, leaf = owner / "leaf.key", owner / "leaf.pem"
    csr, extension = owner / "leaf.csr", owner / "leaf.ext"
    openssl(["req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "1",
             "-subj", "/CN=Glaux synthetic issuer CA", "-keyout", str(ca_key), "-out", str(ca),
             "-addext", "basicConstraints=critical,CA:TRUE",
             "-addext", "keyUsage=critical,keyCertSign,cRLSign"])
    openssl(["req", "-new", "-newkey", "rsa:2048", "-nodes", "-subj", "/CN=Glaux synthetic issuer",
             "-keyout", str(leaf_key), "-out", str(csr)])
    extension.write_text("basicConstraints=critical,CA:FALSE\nkeyUsage=critical,digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\nsubjectAltName=IP:127.0.0.1\n")
    openssl(["x509", "-req", "-in", str(csr), "-CA", str(ca), "-CAkey", str(ca_key),
             "-CAcreateserial", "-days", "1", "-out", str(leaf), "-extfile", str(extension)])
    keys, tokens = {}, {}
    for name, subject in (("a", "fixture-alice"), ("b", "fixture-bob")):
        key = owner / ("signing-" + name + ".key")
        openssl(["genpkey", "-algorithm", "RSA", "-pkeyopt", "rsa_keygen_bits:2048", "-out", str(key)])
        key.chmod(0o600)
        keys[name] = public_key(key, "key-" + name)
        claims = {"iss": "https://issuer.example.test", "aud": "https://api.example.test",
                  "sub": subject, "client_id": "fixture-client", "jti": "token-" + name,
                  "iat": 1699999990, "nbf": 1699999990, "exp": 1700000600, "scope": "read"}
        header = {"alg": "RS256", "typ": "at+jwt", "kid": "key-" + name}
        unsigned = (b64(json.dumps(header).encode()) + "." + b64(json.dumps(claims).encode())).encode()
        tokens[name] = unsigned.decode() + "." + b64(openssl(["dgst", "-sha256", "-sign", str(key)], unsigned))
        if name == "a":
            wrong_claims = {**claims, "iss": "https://unknown-issuer.example.test"}
            unsigned = (b64(json.dumps(header).encode()) + "." + b64(json.dumps(wrong_claims).encode())).encode()
            tokens["wrong-issuer"] = unsigned.decode() + "." + b64(openssl(["dgst", "-sha256", "-sign", str(key)], unsigned))
        key.unlink()
    ca_key.chmod(0o600)
    leaf_key.chmod(0o600)
    ca_key.unlink()
    return ca, leaf, leaf_key, keys, tokens


class State:
    def __init__(self, keys):
        self.keys = keys
        self.condition = threading.Condition()
        self.modes = {}
        self.counts = {}
        self.body_flushes = {}
        self.gates = {}
        self.bad_headers = False
        self.errors = []
        self.attacker = ""
        self.tls_failures = 0

    def set(self, target, mode):
        with self.condition:
            self.modes[target] = mode
            if mode.startswith("hold"):
                self.gates[target] = threading.Event()

    def release(self, target=None):
        with self.condition:
            for name, gate in self.gates.items():
                if target is None or target == name:
                    gate.set()


class OwnedServer(http.server.ThreadingHTTPServer):
    daemon_threads = False
    block_on_close = True

    def handle_error(self, request, client_address):
        self.state.errors.append("owned fixture handler failed")

    def get_request(self):
        connection, address = super().get_request()
        connection.settimeout(3)
        if getattr(self, "tls_context", None) is not None:
            try:
                connection = self.tls_context.wrap_socket(connection, server_side=True)
            except (ssl.SSLError, OSError):
                connection.close()
                with self.state.condition:
                    self.state.tls_failures += 1
                    self.state.condition.notify_all()
                raise
        return connection, address


class QuietHandler(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *_args):
        pass

    def send_bytes(self, code, body, media="application/json", extra=None, chunked=False):
        try:
            self.send_response(code)
            self.send_header("Content-Type", media)
            self.send_header("Connection", "close")
            self.send_header("Transfer-Encoding" if chunked else "Content-Length",
                             "chunked" if chunked else str(len(body)))
            for name, value in (extra or {}).items():
                self.send_header(name, value)
            self.end_headers()
            if chunked:
                for start in range(0, len(body), 4096):
                    part = body[start:start + 4096]
                    self.wfile.write(f"{len(part):x}\r\n".encode() + part + b"\r\n")
                self.wfile.write(b"0\r\n\r\n")
            else:
                self.wfile.write(body)
        except (BrokenPipeError, ConnectionResetError, ssl.SSLError):
            # Deliberate body bounds, aborts and deadlines close the client socket.
            pass


class Issuer(QuietHandler):
    def do_GET(self):
        target = self.path.strip("/")
        state = self.server.state
        with state.condition:
            state.counts[target] = state.counts.get(target, 0) + 1
            state.bad_headers |= "Authorization" in self.headers or "Cookie" in self.headers
            mode = state.modes.get(target, "a")
            gate = state.gates.get(target)
            state.condition.notify_all()
        if mode.startswith("hold") and mode != "hold-body":
            require(gate is not None, "hold fixture lacks explicit gate")
            require(gate.wait(5), "Issuer gate was not explicitly released")
            mode = mode.removeprefix("hold-")
        if mode == "hold-body":
            # Headers and the first body chunk arrive, then the client must
            # enforce its whole-fetch deadline while this body remains open.
            body = json.dumps({"keys": [state.keys["a"]]}).encode()
            prefix, suffix = body[:len(body) // 2], body[len(body) // 2:]
            try:
                self.send_response(200)
                self.send_header("Content-Type", "application/jwk-set+json")
                self.send_header("Transfer-Encoding", "chunked")
                self.send_header("Connection", "close")
                self.end_headers()
                self.wfile.write(f"{len(prefix):x}\r\n".encode() + prefix + b"\r\n")
                self.wfile.flush()
                with state.condition:
                    state.body_flushes[target] = state.body_flushes.get(target, 0) + 1
                    state.condition.notify_all()
                require(gate is not None and gate.wait(5), "Body gate was not explicitly released")
                self.wfile.write(f"{len(suffix):x}\r\n".encode() + suffix + b"\r\n0\r\n\r\n")
            except (BrokenPipeError, ConnectionResetError, ssl.SSLError):
                pass
            return
        a, b = state.keys["a"], state.keys["b"]
        body = json.dumps({"keys": [a] if mode != "b" else [b]}).encode()
        if mode == "ab":
            body = json.dumps({"keys": [a, b]}).encode()
        elif mode == "empty":
            body = b'{"keys":[]}'
        elif mode == "malformed":
            body = b'{"keys":['
        elif mode == "duplicate":
            body = json.dumps({"keys": [a, a]}).encode()
        elif mode == "duplicate-member":
            body = b'{"keys":' + json.dumps([a]).encode() + b',"keys":' + json.dumps([b]).encode() + b'}'
        elif mode == "private":
            body = json.dumps({"keys": [{**b, "d": "SyntheticPrivateKeyCanary"}]}).encode()
        elif mode == "mixed-invalid":
            body = json.dumps({"keys": [b, {"kty": "RSA", "kid": "broken"}]}).encode()
        elif mode == "oversized" or mode == "oversized-chunked":
            body += b" " * 65537
        elif mode == "outage":
            self.send_bytes(503, b'{"error":"SyntheticIssuerPrivateCanary"}')
            return
        elif mode == "redirect":
            self.send_bytes(302, b"", extra={"Location": state.attacker})
            return
        elif mode == "not-modified":
            self.send_bytes(304, b"")
            return
        self.send_bytes(200, body, "text/plain" if mode == "wrong-media" else "application/jwk-set+json",
                        chunked=mode == "oversized-chunked")


class Control(QuietHandler):
    def do_GET(self):
        parts = self.path.strip("/").split("/")
        state = self.server.state
        if parts[0] == "set" and len(parts) == 3:
            state.set(parts[1], parts[2])
        elif parts[0] == "release" and len(parts) == 2:
            state.release(parts[1])
        elif parts[0] in ("wait", "wait-body") and len(parts) == 3:
            target, expected = parts[1], int(parts[2])
            deadline = time.monotonic() + 3
            with state.condition:
                counts = state.counts if parts[0] == "wait" else state.body_flushes
                while counts.get(target, 0) < expected:
                    left = deadline - time.monotonic()
                    if left <= 0:
                        self.send_bytes(500, b'{"barrier":false}')
                        return
                    state.condition.wait(left)
        elif parts[0] == "wait-tls" and len(parts) == 2:
            deadline = time.monotonic() + 3
            with state.condition:
                while state.tls_failures < int(parts[1]):
                    left = deadline - time.monotonic()
                    if left <= 0:
                        self.send_bytes(500, b'{"tls_barrier":false}')
                        return
                    state.condition.wait(left)
        elif parts != ["counts"]:
            self.send_bytes(404, b"{}")
            return
        with state.condition:
            result = {"counts": dict(state.counts), "bad_headers": state.bad_headers,
                      "tls_failures": state.tls_failures, "body_flushes": dict(state.body_flushes)}
        self.send_bytes(200, json.dumps(result).encode())


@contextmanager
def issuer_fixture(owner):
    ca, leaf, leaf_key, keys, tokens = materials(owner)
    state = State(keys)
    servers, threads = [], []
    try:
        for handler in (Issuer, Control):
            server = OwnedServer(("127.0.0.1", 0), handler)
            servers.append(server)
            server.state = state
            if handler is Issuer:
                context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
                context.minimum_version = ssl.TLSVersion.TLSv1_2
                context.load_cert_chain(str(leaf), str(leaf_key))
                server.tls_context = context
            thread = threading.Thread(target=server.serve_forever, kwargs={"poll_interval": 0.05})
            thread.start()
            threads.append(thread)
        leaf_key.unlink()
        base = "https://127.0.0.1:" + str(servers[0].server_port)
        state.attacker = base + "/attacker"
        # These token edits change only hostile headers; no accepted case relies on them.
        payload, signature = tokens["a"].split(".")[1:]
        for hint, value in (("jku", state.attacker), ("x5u", state.attacker),
                            ("jwk", keys["b"]), ("x5c", ["SyntheticUntrustedCertificate"])):
            header = {"alg": "RS256", "typ": "at+jwt", "kid": "key-a", hint: value}
            tokens[hint] = b64(json.dumps(header).encode()) + "." + payload + "." + signature
        header = {"alg": "RS256", "typ": "JWT", "kid": "key-a"}
        tokens["bad-profile"] = b64(json.dumps(header).encode()) + "." + payload + "." + signature
        tokens["malformed"] = "invalid.compact.token.with.extra.segment"
        tokens["bad-signature"] = tokens["a"].rsplit(".", 1)[0] + "." + tokens["b"].rsplit(".", 1)[1]
        for index in range(32):
            header = {"alg": "RS256", "typ": "at+jwt", "kid": "unknown-" + str(index)}
            tokens["unknown-" + str(index)] = b64(json.dumps(header).encode()) + "." + payload + "." + signature
        fixture = owner / "public-fixtures.json"
        fixture.write_text(json.dumps({"base": base, "wrong_host": base.replace("127.0.0.1", "localhost"),
                                       "control": "127.0.0.1:" + str(servers[1].server_port),
                                       "ca": ca.read_text(), "keys": keys, "tokens": tokens}))
        yield fixture
    finally:
        state.release()
        addresses = [server.server_address for server in servers]
        # shutdown() requires a running serve_forever loop; setup can fail
        # after binding a server but before its thread is started.
        for server in servers[:len(threads)]:
            server.shutdown()
        for server in servers:
            server.server_close()
        for thread in threads:
            thread.join(timeout=4)
            require(not thread.is_alive(), "Owned issuer listener survived cleanup")
        for address in addresses:
            try:
                connection = socket.create_connection(address, timeout=0.1)
            except OSError:
                continue
            connection.close()
            raise RuntimeError("Owned issuer address still accepts connections")
        require(not state.errors, "Owned fixture handler failed")
        require(not state.bad_headers, "Issuer received a bearer credential or cookie")


def build_proof(source_root, target_directory):
    result = subprocess.run(["cargo", "build", "--locked", "--offline", "-p", "glaux-server",
                             "--example", "key-refresh-proof", "--target-dir", str(target_directory)],
                            cwd=source_root, capture_output=True, text=True, timeout=180, check=False)
    print(result.stdout + result.stderr, end="", flush=True)
    proof = target_directory / "debug/examples/key-refresh-proof"
    require(result.returncode == 0 and proof.is_file(), "Required key-refresh proof did not build")
    return proof


def run_binary(proof, owner):
    with issuer_fixture(owner) as fixture:
        return subprocess.run([str(proof), str(fixture)], capture_output=True, text=True,
                              timeout=60, check=False)


def validate_output(output):
    prefix = "Key refresh group passed: "
    actual = [line for line in output.splitlines() if line.startswith(prefix)]
    require(actual == [prefix + name for name in GROUPS], "Required key-refresh groups missing/duplicated/reordered")
    require(output.splitlines().count(FINAL) == 1, "Required key-refresh final marker missing/duplicated")


def main():
    require(Path.cwd().resolve() == ROOT and not sys.argv[1:], "Run from root without target/selection overrides")
    proof = build_proof(ROOT, ROOT / "target")
    with tempfile.TemporaryDirectory(prefix="glaux-key-fixture-", dir=os.environ["RUNNER_TEMP"]) as directory:
        result = run_binary(proof, Path(directory))
        output = result.stdout + result.stderr
        print(output, end="", flush=True)
        require(result.returncode == 0, "Required key-refresh proof failed")
        validate_output(output)
    print("Key refresh: 7 groups passed; 0 failed; 0 skipped", flush=True)
    print("Key refresh: all required checks passed.", flush=True)


if __name__ == "__main__":
    main()
