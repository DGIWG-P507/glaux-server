"""Hosted Chrome rendering proof; no installation and no external browser access.

This exercises the production discovery router, not database startup. The real
binary/database discovery proof owns that different claim.
"""

from html.parser import HTMLParser
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import select
import shutil
import signal
import socket
import subprocess
import sys
import tempfile
import threading
import time
from urllib.parse import urlencode, urlsplit
from urllib.request import ProxyHandler, build_opener


ROOT = Path(__file__).resolve().parents[1]
FINAL = "Discovery browser: all required checks passed."
CANARY = "http://glaux-browser-denial.invalid/controlled-egress-canary"
DENIED = "Glaux controlled browser egress denial"
# Independent contract answers, not extracted from the served OpenAPI document.
EXPECTED_PATHS = (
    "/", "/conformance", "/api", "/docs", "/docs/init.js", "/docs/swagger-ui-bundle.js",
    "/docs/swagger-ui.css", "/docs/LICENSE", "/docs/NOTICE",
    "/docs/swagger-ui-bundle.js.LICENSE.txt", "/schemas/discovery.json",
    "/examples/landing.json", "/examples/conformance.json", "/health/live", "/health/ready",
)
EXPECTED_OPERATIONS = tuple(sorted((method, path) for method in ("GET", "HEAD")
                                   for path in EXPECTED_PATHS))
EXPECTED_TITLE = "Glaux Server initial API"
EXPECTED_PARTIAL = "no resource families or conformance classes are advertised yet."
VOID = {"area", "base", "br", "col", "embed", "hr", "img", "input", "link",
        "meta", "param", "source", "track", "wbr"}


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


class VersionProbeError(RuntimeError):
    def __init__(self, record):
        super().__init__("Browser version probe failed: " + record["label"] + ": " + record["failure"])
        self.record = record


def version_probe(arguments, evidence, label, timeout):
    """A single setup attempt, bounded independently of browser rendering."""
    started = time.monotonic()
    captured = {"stdout": bytearray(), "stderr": bytearray()}
    process = None
    failure = None
    cleanup = "not started"
    try:
        process = subprocess.Popen(arguments, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                   bufsize=0, start_new_session=True)
        streams = {process.stdout: "stdout", process.stderr: "stderr"}
        deadline = started + timeout
        while streams and failure is None:
            remaining = deadline - time.monotonic()
            ready = select.select(list(streams), [], [], max(0, remaining))[0]
            if remaining <= 0 or not ready:
                failure = "timeout"
                break
            for stream in ready:
                chunk = os.read(stream.fileno(), 4096)
                if not chunk:
                    del streams[stream]
                else:
                    captured[streams[stream]].extend(chunk)
                    if len(captured[streams[stream]]) > 8192:
                        failure = "output budget exceeded"
                        break
        if failure is None:
            process.communicate(timeout=max(0, deadline - time.monotonic()))
            cleanup = "reaped"
    except subprocess.TimeoutExpired:
        failure = "timeout"
    except OSError as error:
        failure = "execution error: " + type(error).__name__
    finally:
        if process is not None and cleanup != "reaped":
            try:
                try:
                    os.killpg(process.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                output, error = process.communicate(timeout=5)
                # After killing the owned group, retain only a bounded diagnostic
                # tail from any bytes left in its pipes, never an unbounded log.
                captured["stdout"].extend(output[-4096:])
                captured["stderr"].extend(error[-4096:])
                cleanup = "killed and reaped"
            except (OSError, subprocess.TimeoutExpired) as error:
                failure = "cleanup error: " + type(error).__name__
                cleanup = "failed"
                process.stdout.close()
                process.stderr.close()
    output = bytes(captured["stdout"]).decode("utf-8", errors="replace").strip()
    error = bytes(captured["stderr"]).decode("utf-8", errors="replace")
    if failure is None and process.returncode != 0:
        failure = "nonzero exit"
    if failure is None and not output:
        failure = "empty version"
    record = {"label": label, "timeout_seconds": timeout,
              "elapsed_seconds": round(time.monotonic() - started, 3),
              "returncode": None if process is None else process.returncode,
              "failure": failure, "cleanup": cleanup,
              "stdout_tail": output[-4096:], "stderr_tail": error[-4096:]}
    (evidence / ("discovery-browser-version-" + label + ".json")).write_text(json.dumps(record, indent=2))
    print("Discovery browser version probe: " + json.dumps({key: record[key] for key in
          ("label", "elapsed_seconds", "failure", "cleanup")}), flush=True)
    if failure is not None:
        raise VersionProbeError(record)
    return output


def version_probe_controls(evidence):
    prefix = [sys.executable, "-c"]
    version = version_probe(prefix + ["print('Synthetic Browser 1.2.3')"], evidence, "control-success", 5)
    require(version == "Synthetic Browser 1.2.3", "Version helper changed its expected output")
    cases = (
        ("control-nonzero", "import sys; print('not a passing version'); sys.exit(7)", 5, "nonzero exit", 7),
        ("control-empty", "pass", 5, "empty version", 0),
        ("control-timeout", "import time; print('sleeping probe started', flush=True); time.sleep(60)",
         2, "timeout", -signal.SIGKILL),
    )
    for label, source, timeout, expected, code in cases:
        try:
            version_probe(prefix + [source], evidence, label, timeout)
        except VersionProbeError as error:
            require(error.record["failure"] == expected and error.record["returncode"] == code,
                    "Version helper control failed for an unintended reason")
            if expected == "timeout":
                require(error.record["cleanup"] == "killed and reaped"
                        and error.record["stdout_tail"] == "sleeping probe started"
                        and 2 <= error.record["elapsed_seconds"] < 8,
                        "Sleeping version probe did not establish its bounded timeout/cleanup")
        else:
            raise RuntimeError("Version helper admitted a failing setup probe")
    print("Discovery browser version controls: 1 success; 3 failures detected; 0 escaped.", flush=True)


class Rendered(HTMLParser):
    """Read actual DOM text/classes without the renderer's JavaScript model."""

    def __init__(self):
        super().__init__(convert_charrefs=True)
        self.frames = []
        self.methods = []
        self.paths = []
        self.titles = []
        self.text = []
        self.ready = False
        self.errors = False
        self.try_it_out = False

    def handle_starttag(self, tag, attributes):
        attributes = dict(attributes)
        classes = set(attributes.get("class", "").split())
        self.ready |= attributes.get("data-glaux-rendered") == "true"
        self.errors |= bool(classes & {"errors-wrapper", "render-errors"})
        self.try_it_out |= bool(classes & {"try-out__btn", "execute"})
        if tag not in VOID:
            self.frames.append((tag, classes, []))

    def handle_startendtag(self, tag, attributes):
        self.handle_starttag(tag, attributes)
        if tag not in VOID:
            self.handle_endtag(tag)

    def handle_data(self, data):
        self.text.append(data)
        for _, _, parts in self.frames:
            parts.append(data)

    def handle_endtag(self, tag):
        if not any(frame[0] == tag for frame in self.frames):
            return
        while self.frames:
            current, classes, parts = self.frames.pop()
            value = " ".join("".join(parts).split())
            if "opblock-summary-method" in classes:
                self.methods.append(value)
            if "opblock-summary-path" in classes:
                self.paths.append(value)
            # Pinned Swagger UI 5.33 renders its API title as h1; the outer
            # documentation page's own h1 has no title class and cannot satisfy it.
            if current == "h1" and "title" in classes:
                self.titles.append(value)
            if current == tag:
                break


def assert_rendered(document):
    dom = Rendered()
    dom.feed(document)
    require(dom.ready, "Swagger renderer did not report completed rendering")
    require(len(dom.titles) == 1 and (dom.titles[0] == EXPECTED_TITLE
            or dom.titles[0].startswith(EXPECTED_TITLE + " ")),
            "Expected API title was not rendered")
    require(EXPECTED_PARTIAL in " ".join(dom.text).lower(),
            "Partial implementation qualification was not rendered")
    require(len(dom.methods) == len(dom.paths)
            and tuple(sorted(zip(dom.methods, dom.paths))) == EXPECTED_OPERATIONS,
            "Rendered operations differ from independently expected discovery contract")
    require(not dom.errors, "Renderer displayed an error")
    require(not dom.try_it_out, "Documentation enabled a Try it out/execute control")
    require("GlauxMaliciousReplacement" not in " ".join(dom.text),
            "Query supplied replacement API was rendered")


def oracle_controls():
    sample = ('<main data-glaux-rendered="true"><h1 class="title">Glaux Server initial API'
              '<span><small><pre class="version"> 0.1.0 </pre></small>'
              '<small><pre>OAS 3.1</pre></small></span></h1>'
              '<p>No resource families or conformance classes are advertised yet.</p>'
              + ''.join('<span class="opblock-summary-method">' + method + '</span>'
                        '<span class="opblock-summary-path">' + path + '</span>'
                        for method, path in EXPECTED_OPERATIONS) + '</main>')
    assert_rendered(sample)
    changes = (
        sample.replace('data-glaux-rendered="true"', ''),
        sample.replace("/conformance", "/systems"),
        sample.replace("Glaux Server", "GlauxMaliciousReplacement"),
        sample.replace("No resource families", "All resource families"),
        sample.replace("</main>", '<button class="try-out__btn">Try it out</button></main>'),
        sample.replace("</main>", '<div class="errors-wrapper">error</div></main>'),
    )
    for changed in changes:
        try:
            assert_rendered(changed)
        except RuntimeError:
            continue
        raise RuntimeError("Known-bad rendered DOM escaped the independent oracle")
    print("Discovery browser oracle controls: 6 detected; 0 escaped.", flush=True)


class DenyProxy(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def setup(self):
        super().setup()
        self.connection.settimeout(5)

    def deny(self):
        attempt = {"method": self.command, "target": self.path,
                   "decision": "blocked; no upstream connection", "client_disconnected": False}
        with self.server.record_lock:
            require(len(self.server.attempts) < 200, "Browser proxy attempt budget exceeded")
            self.server.attempts.append(attempt)
        body = ("<!doctype html><title>" + DENIED + "</title><p>" + DENIED + "</p>").encode()
        try:
            self.send_response(502)
            self.send_header("Content-Type", "text/html; charset=utf-8")
            self.send_header("Content-Length", str(len(body)))
            self.send_header("Connection", "close")
            self.end_headers()
            if self.command != "HEAD":
                self.wfile.write(body)
        except (BrokenPipeError, ConnectionResetError):
            # Chrome may exit after rejecting a CONNECT response. The attempt
            # was denied before this write; no upstream socket exists to leak.
            with self.server.record_lock:
                attempt["client_disconnected"] = True
        finally:
            self.close_connection = True

    do_GET = deny
    do_HEAD = deny
    do_POST = deny
    do_CONNECT = deny
    do_PUT = deny
    do_DELETE = deny
    do_OPTIONS = deny
    do_PATCH = deny
    do_TRACE = deny

    def log_message(self, *_):
        pass


class DenyServer(ThreadingHTTPServer):
    def handle_error(self, *_):
        # An exception in a request thread must not silently permit a pass.
        with self.record_lock:
            self.errors.append(type(sys.exc_info()[1]).__name__)


def chrome_dom(chrome, url, profile, proxy, evidence, label):
    arguments = [
        chrome, "--headless", "--no-sandbox", "--disable-dev-shm-usage",
        "--disable-background-networking", "--disable-component-update",
        "--disable-domain-reliability", "--disable-sync", "--disable-quic",
        "--disable-client-side-phishing-detection", "--metrics-recording-only",
        "--no-first-run", "--no-default-browser-check", "--safebrowsing-disable-auto-update",
        "--disable-features=MediaRouter,OptimizationHints,OptimizationGuideModelDownloading,OptimizationHintsFetching,AutofillServerCommunication",
        "--host-resolver-rules=MAP * ~NOTFOUND, EXCLUDE 127.0.0.1, EXCLUDE localhost",
        f"--proxy-server=http://127.0.0.1:{proxy.server_port}",
        "--proxy-bypass-list=<-loopback>;127.0.0.1;localhost",
        f"--user-data-dir={profile}", "--disable-extensions", "--password-store=basic",
        "--virtual-time-budget=5000", "--timeout=15000", "--dump-dom", url,
    ]
    process = subprocess.Popen(arguments, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                               text=True, start_new_session=True)
    try:
        output, error = process.communicate(timeout=20)
        require(process.returncode == 0, f"Hosted Chrome failed for {label}: {error[-3000:]}")
        require(len(output) < 2_000_000 and len(error) < 2_000_000,
                "Browser output exceeded its evidence budget")
        (evidence / ("discovery-browser-" + label + ".html")).write_text(output)
        (evidence / ("discovery-browser-" + label + ".stderr")).write_text(error)
        return output
    finally:
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        process.wait(timeout=5)


def stop_fixture(process):
    if process.poll() is None:
        os.killpg(process.pid, signal.SIGINT)
    try:
        output, error = process.communicate(timeout=10)
    except subprocess.TimeoutExpired:
        os.killpg(process.pid, signal.SIGKILL)
        process.communicate(timeout=5)
        raise RuntimeError("Browser fixture did not stop within its cleanup deadline") from None
    require(process.returncode == 0 and "Discovery browser stopped." in output,
            "Browser fixture cleanup failed: " + output[-2000:] + error[-2000:])
    return output


def main():
    require(Path.cwd().resolve() == ROOT and not sys.argv[1:],
            "Run from workspace root without target or selection overrides")
    require(os.environ.get("GITHUB_ACTIONS") == "true" and os.environ.get("RUNNER_OS") == "Linux",
            "Browser proof is restricted to the approved GitHub-hosted Linux job")
    chrome = shutil.which("google-chrome")
    require(chrome is not None, "Hosted Google Chrome is unavailable; no installation or skip permitted")
    runner_temp = Path(os.environ["RUNNER_TEMP"]).resolve()
    evidence = runner_temp / "glaux-ci-evidence"
    evidence.mkdir(exist_ok=True)
    version_probe_controls(evidence)
    version = version_probe([chrome, "--version"], evidence, "chrome", 30)
    runner = {key: os.environ.get(key, "not supplied") for key in
              ("ImageOS", "ImageVersion", "RUNNER_ARCH", "RUNNER_OS")}
    print("Discovery browser runtime: " + json.dumps({"chrome": version, "runner": runner}), flush=True)
    oracle_controls()
    build = subprocess.run(["cargo", "build", "--locked", "--offline", "-p", "glaux-server", "--example",
                            "discovery-browser-fixture"], capture_output=True, text=True, timeout=30)
    require(build.returncode == 0, "Browser fixture failed to build: " + build.stdout + build.stderr)
    proxy = DenyServer(("127.0.0.1", 0), DenyProxy)
    # server_close waits for bounded request threads before their errors/results
    # are inspected, so a late handler failure cannot escape the final check.
    proxy.daemon_threads = False
    proxy.attempts = []
    proxy.errors = []
    proxy.record_lock = threading.Lock()
    thread = threading.Thread(target=proxy.serve_forever, daemon=True)
    thread.start()
    process = None
    fixture_output = ""
    root = None
    canary_attempts = []
    try:
        with tempfile.TemporaryDirectory(prefix="glaux-discovery-browser-", dir=runner_temp) as directory:
            directory = Path(directory)
            canary = chrome_dom(chrome, CANARY, directory / "canary", proxy, evidence, "egress-canary")
            require(DENIED in canary and any(row["method"] == "GET" and row["target"] == CANARY
                                           for row in proxy.attempts),
                    "External-request canary did not reach the deny-only proxy")
            print("Discovery browser egress canary: request rejected by owned deny-only proxy.", flush=True)
            with proxy.record_lock:
                canary_attempts = list(proxy.attempts)
                proxy.attempts.clear()
            process = subprocess.Popen([str(ROOT / "target/debug/examples/discovery-browser-fixture")],
                                       stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
                                       start_new_session=True)
            ready, _, _ = select.select([process.stdout], [], [], 10)
            require(ready, "Browser fixture did not publish its bound listener")
            first = process.stdout.readline().strip()
            prefix = "Discovery browser ready: "
            require(first.startswith(prefix), "Browser fixture readiness record is absent")
            root = first.removeprefix(prefix)
            parsed = urlsplit(root)
            require(parsed.scheme == "http" and parsed.hostname == "127.0.0.1"
                    and parsed.port and parsed.path == "", "Browser fixture was not owned loopback")
            opener = build_opener(ProxyHandler({}))
            with opener.open(root + "/", timeout=5) as response:
                landing = json.loads(response.read(256_000))
            docs = [link["href"] for link in landing["links"] if link["rel"] == "service-doc"]
            require(len(docs) == 1 and docs[0].startswith(root + "/"),
                    "Root did not disclose exactly one local documentation link")
            url = docs[0]
            attacks = {
                "normal": url,
                "same-origin-query": url + "?" + urlencode({
                    "url": root + "/__replacement-spec", "configUrl": root + "/__replacement-config"}),
                "external-query": url + "?" + urlencode({
                    "url": "http://glaux-browser-denial.invalid/GlauxMaliciousReplacement",
                    "configUrl": "http://glaux-browser-denial.invalid/replacement-config",
                    "validatorUrl": "http://glaux-browser-denial.invalid/replacement-validator"}),
            }
            for label, target in attacks.items():
                document = chrome_dom(chrome, target, directory / label, proxy, evidence, label)
                assert_rendered(document)
                print("Discovery browser rendered: " + label, flush=True)
    finally:
        try:
            if process is not None:
                fixture_output = stop_fixture(process)
                (evidence / "discovery-browser-fixture.log").write_text(fixture_output)
        finally:
            proxy.shutdown()
            proxy.server_close()
            thread.join(timeout=5)
            require(not thread.is_alive(), "Deny-only proxy did not stop")
            with proxy.record_lock:
                blocked_attempts = list(proxy.attempts)
                proxy_errors = list(proxy.errors)
            (evidence / "discovery-browser-proxy.json").write_text(json.dumps({
                "canary_phase_blocked_attempts": canary_attempts,
                "rendering_phase_blocked_attempts": blocked_attempts,
                "unexpected_proxy_errors": proxy_errors,
                "attribution": "Attempts are recorded without classifying browser or page origin.",
            }, indent=2))
    require(not proxy_errors, "Deny-only proxy encountered an unexpected error: " + repr(proxy_errors))
    # Browser processes can attempt background services despite the disabling
    # flags. Every proxy request is denied, without a hostname exception list.
    # These test-controlled targets are unambiguously forbidden query overrides.
    require(not any("glaux-browser-denial.invalid" in row["target"] for row in blocked_attempts),
            "Browser attempted a query-supplied external specification/configuration/validator")
    requests = [json.loads(line.removeprefix("Discovery browser request: "))
                for line in fixture_output.splitlines() if line.startswith("Discovery browser request: ")]
    require(requests and all(row["method"] == "GET" for row in requests),
            "Browser fixture recorded an unexpected method")
    require(not any("__replacement" in row["uri"].split("?", 1)[0] for row in requests),
            "Documentation fetched a query-supplied replacement specification/configuration")
    allowed = {"/", "/docs", "/api", "/docs/init.js", "/docs/swagger-ui-bundle.js",
               "/docs/swagger-ui.css", "/favicon.ico"}
    require(all(urlsplit(row["uri"]).path in allowed for row in requests),
            "Documentation fetched an unexpected local resource: " + repr(requests))
    required = allowed - {"/favicon.ico"}
    require(required <= {urlsplit(row["uri"]).path for row in requests},
            "Browser did not load the complete owned renderer/specification resource set")
    with socket.socket() as probe:
        probe.settimeout(1)
        require(probe.connect_ex(("127.0.0.1", urlsplit(root).port)) != 0,
                "Owned browser listener remains open after cleanup")
    (evidence / "discovery-browser.json").write_text(json.dumps({
        "chrome": version, "runner": runner, "fixture": "production router; no database startup claim",
        "oracle_controls_detected": 6, "rendered_variants": list(attacks),
        "external_canary_blocked": True, "rendering_phase_blocked_attempts": blocked_attempts,
        "external_attempt_attribution": "Not established; all proxy requests are denied.",
        "requests": requests, "listener_stopped": True,
    }, indent=2))
    print(FINAL, flush=True)


if __name__ == "__main__":
    main()
