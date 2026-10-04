"""Temporary, GitHub-hosted-only Step 5 environment; never production code.

Source-backed setup: OSH 235c0eab / AbstractTestApiBase, HttpServer and
HttpServerConfig, SensorHub.main, MVDatabaseConfig, BigId and VarInt. The
independent HTTP transport imports no server models; setup is not a verdict.
"""
import base64
import datetime
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import signal
import subprocess
import tarfile
import tempfile
import time
import urllib.parse
import urllib.request
import zipfile

from database_harness import DisposablePostgis, HarnessError, ROOT, TARGET

OSH_PIN = "235c0eabf24b6d6137b499b4402943d2794b70e6"
JDK_URL = ("https://github.com/adoptium/temurin17-binaries/releases/download/"
           "jdk-17.0.16%2B8/OpenJDK17U-jdk_x64_linux_hotspot_17.0.16_8.tar.gz")
JDK_SHA = "166774efcf0f722f2ee18eba0039de2d685b350ee14d7b69e6f83437dafd2af1"
GRADLE_URL = "https://services.gradle.org/distributions/gradle-8.10.2-bin.zip"
GRADLE_SHA = "31c55713e40233a8303827ceb42ca48a47267a0ad4bab9177123121e71524c26"
ASSETS = ROOT / "scripts/review-phase1-osh"
INSIDE = "/tmp/glaux-phase1-comparison"
LOG_CAP = 8 * 1024 * 1024
RAW_CAP = 1024 * 1024
# The same identity guard protects real shutdown and the non-signalling probe.
# /proc/PID/{exe,fd} inspection uses the serving UID, not container root without
# CAP_SYS_PTRACE. No capability, namespace or target-validation guard is relaxed.
OWNED_PROCESS_GUARD = (
    'pid=$(cat "$1"); case "$pid" in ""|*[!0-9]*) exit 2;; esac; '
    'test "$(readlink -f /proc/$pid/exe)" = "$2" || exit 3; '
    'case "$3" in probe) exit 0;; stop) kill -TERM "$pid";; *) exit 4;; esac'
)


def require(condition, message):
    if not condition:
        raise HarnessError(message)


def sha(path):
    with Path(path).open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def utc():
    return datetime.datetime.now(datetime.timezone.utc).isoformat()


def json_file(path, value):
    Path(path).write_text(json.dumps(value, indent=2) + "\n", encoding="utf-8")


def _varint(value):
    answer = bytearray()
    while value > 127:
        answer.append((value & 127) | 128)
        value >>= 7
    answer.append(value)
    return bytes(answer)


def absent_osh_id():
    # Pinned SensorHub.main defaults to IdEncodersBase32, not DES. BigId.toString32
    # writes scope varint then BigIdLong's unsigned varlong in base32hex.
    # The owned fresh DB uses scope 2 / SEQUENTIAL; two creations cannot allocate
    # 2^62. This is not a Glaux UUID sent to a peer whose encoding differs.
    return base64.b32hexencode(_varint(2) + _varint(2**62)).decode().lower().rstrip("=")


def safe_url(base, reference):
    require(isinstance(reference, str) and len(reference) <= 4096,
            "invalid or oversized HTTP reference")
    require(not any(ord(c) < 33 or ord(c) == 127 for c in reference),
            "whitespace/control in HTTP reference")
    require("\\" not in reference, "backslash in HTTP reference")
    # Reject both literal and encoded traversal BEFORE urljoin normalizes it.
    decoded = urllib.parse.unquote(reference)
    require("%" not in decoded and "\\" not in decoded,
            "nested percent encoding or encoded backslash")
    require(not any(p in (".", "..") for p in urllib.parse.urlsplit(decoded).path.split("/")),
            "traversal in HTTP reference")
    result = urllib.parse.urlsplit(urllib.parse.urljoin(base.rstrip("/") + "/", reference))
    origin = urllib.parse.urlsplit(base)
    require(not result.username and not result.password and not result.fragment,
            "userinfo/fragment not an approved runtime destination")
    require((result.scheme, result.netloc) == (origin.scheme, origin.netloc),
            "response-directed external destination refused")
    boundary = origin.path.rstrip("/")
    require(not boundary or result.path == boundary or result.path.startswith(boundary + "/"),
            "destination outside approved API path")
    require(result.path.startswith("/"), "HTTP target must be absolute path")
    return urllib.parse.urlunsplit(result)


def parse_wire(raw, method):
    require(0 < len(raw) <= RAW_CAP and b"\r\n\r\n" in raw,
            "empty, oversized or incomplete HTTP capture")
    head, entity = raw.split(b"\r\n\r\n", 1)
    require(len(head) <= 65536, "HTTP header cap")
    lines = head.decode("iso-8859-1").split("\r\n")
    match = re.fullmatch(r"HTTP/1\.[01] ([1-5][0-9]{2})(?: .*)?", lines[0])
    require(match is not None, "invalid HTTP status line")
    status = int(match.group(1))
    require(status >= 200, "interim HTTP response not a complete final response")
    headers = {}
    for line in lines[1:]:
        require(":" in line and not line[:1].isspace(), "invalid HTTP header")
        name, value = line.split(":", 1)
        require(re.fullmatch(r"[!#$%&'*+.^_`|~0-9A-Za-z-]+", name), "invalid header name")
        headers.setdefault(name.lower(), []).append(value.strip())
    transfer = headers.get("transfer-encoding", [])
    lengths = headers.get("content-length", [])
    require(not (transfer and lengths), "ambiguous HTTP length framing")
    require(len(lengths) <= 1, "duplicate Content-Length")
    if lengths:
        require(re.fullmatch(r"[0-9]+", lengths[0]), "invalid Content-Length")
    # HEAD may carry a hypothetical length. Keep *all* received bytes so the
    # checker catches illegal payloads rather than transport silently losing them.
    if method == "HEAD":
        return status, headers, entity
    if transfer:
        require(len(transfer) == 1 and transfer[0].lower() == "chunked",
                "unsupported transfer coding; raw reply retained")
        body = bytearray()
        offset = 0
        while True:
            end = entity.find(b"\r\n", offset)
            require(end >= 0, "incomplete chunk header")
            token = entity[offset:end].split(b";", 1)[0]
            require(re.fullmatch(b"[0-9a-fA-F]+", token), "invalid chunk size")
            size = int(token, 16)
            offset = end + 2
            if size == 0:
                # Accept syntactically framed trailers but never use them to
                # overwrite the original response metadata.
                tail = entity[offset:]
                require(tail == b"\r\n" or (tail.endswith(b"\r\n\r\n") and
                        all(b":" in line for line in tail[:-4].split(b"\r\n"))),
                        "incomplete or invalid chunk trailer")
                break
            require(size <= RAW_CAP and offset + size + 2 <= len(entity), "incomplete chunk")
            body.extend(entity[offset:offset + size])
            require(entity[offset + size:offset + size + 2] == b"\r\n", "chunk terminator")
            offset += size + 2
        entity = bytes(body)
    elif lengths:
        require(len(entity) == int(lengths[0]), "truncated/extra HTTP response bytes")
    require(not headers.get("content-encoding"), "unexpected content encoding")
    return status, headers, entity


def parser_controls():
    """Prove framing sensitivity without either implementation or model types."""
    records = []
    valid = b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\n\r\nabc"
    require(parse_wire(valid, "GET")[2] == b"abc", "valid Content-Length control")
    records.append({"control": "valid-length", "outcome": "accepted exact bytes"})
    chunked = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n1\r\na\r\n2\r\nbc\r\n0\r\nX-Note: ok\r\n\r\n"
    require(parse_wire(chunked, "GET")[2] == b"abc", "valid chunks/trailer control")
    records.append({"control": "valid-chunks-trailer", "outcome": "accepted exact decoded bytes"})
    illegal_head = b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\n\r\nabc"
    require(parse_wire(illegal_head, "HEAD")[2] == b"abc", "HEAD payload silently discarded")
    require(parse_wire(illegal_head[:-3], "HEAD")[2] == b"", "valid hypothetical HEAD length")
    records.append({"control": "head-body-preservation", "outcome": "illegal body retained for checker; empty body retained"})
    bad = {
        "length-truncated": valid[:-1],
        "length-extra": valid + b"d",
        "chunk-truncated": chunked[:-1],
        "chunk-size-invalid": chunked.replace(b"1\r\na", b"g\r\na", 1),
        "chunk-terminator-invalid": chunked.replace(b"a\r\n2", b"a\rX2", 1),
        "trailer-invalid": chunked.replace(b"X-Note: ok", b"not-a-header"),
        "duplicate-length": valid.replace(b"Content-Length: 3", b"Content-Length: 3\r\nContent-Length: 3"),
        "transfer-length-ambiguous": chunked.replace(b"Transfer-Encoding:", b"Content-Length: 3\r\nTransfer-Encoding:"),
        "interim-not-final": valid.replace(b"200 OK", b"100 Continue"),
    }
    for name, wire in bad.items():
        try:
            parse_wire(wire, "GET")
        except HarnessError as error:
            records.append({"control": name, "outcome": "rejected", "reason": str(error)})
        else:
            raise HarnessError("wire parser failed sensitivity control: " + name)
    # Destination controls are also executable before any network request.
    for reference in ("http://example.org/systems", "../systems", "%2e%2e/systems",
                      "http://user@127.0.0.1:18888/sensorhub/api", "/outside", "systems#fragment"):
        try:
            safe_url("http://127.0.0.1:18888/sensorhub/api", reference)
        except HarnessError as error:
            records.append({"control": "destination-" + reference, "outcome": "rejected", "reason": str(error)})
        else:
            raise HarnessError("destination guard failed sensitivity control: " + reference)
    return records


class Runtime:
    bases = {"glaux": "http://127.0.0.1:18080", "osh": "http://127.0.0.1:18888/sensorhub/api"}
    missing_ids = {"glaux": "0190f5c2-7b5a-7cc3-98c4-dc0c0c22ffff", "osh": absent_osh_id()}

    def __init__(self, evidence_dir, deadline):
        require(os.environ.get("GITHUB_ACTIONS") == "true" and os.name == "posix",
                "diagnostic is authorized only on the GitHub-hosted Linux lane")
        self.evidence = Path(evidence_dir).resolve()
        self.evidence.mkdir(parents=True, exist_ok=True)
        self.deadline = deadline
        require(0 < deadline - time.monotonic() <= 1200, "invalid total diagnostic deadline")
        self.commands = []
        self.requests = 0
        self.serial = 0
        self.processes = {}
        self.starts = {"glaux": 0, "osh": 0}
        self.db = None
        self.temporary = None
        self.peer_before = None
        self.cleanup_deadline = None
        self.setup = {"started": utc(), "source": OSH_PIN, "bases": self.bases,
                      "missing_ids": self.missing_ids, "runtime_isolation": "network none; no binds/ports",
                      "writability": "not established until successful creation and retained H2 file"}

    def remaining(self, cap=60, cleanup=False):
        available = self.deadline - time.monotonic()
        if cleanup:
            available = (self.cleanup_deadline or self.deadline) - time.monotonic()
            require(available > 0, "cleanup deadline reached")
            return min(cap, available)
        require(available > 20, "diagnostic deadline reached; preserving cleanup margin")
        return min(cap, available - 20)

    def save(self):
        json_file(self.evidence / "runtime-setup.json", self.setup)
        json_file(self.evidence / "commands.json", self.commands)

    def environment(self):
        # Build/runtime preparations must not inherit GitHub, cloud, proxy or
        # workstation secrets. Downloads use anonymous public repositories.
        keep = ("PATH", "HOME", "LANG", "LC_ALL", "TMPDIR", "RUNNER_TEMP")
        return {k: os.environ[k] for k in keep if k in os.environ}

    def command(self, args, name, cwd=None, env=None, cap=60, cleanup=False):
        limit = self.remaining(cap, cleanup)
        path = self.evidence / (f"command-{len(self.commands):03d}-{name}.log")
        entry = {"command": [str(a) for a in args], "cwd": str(cwd or ROOT),
                 "started": utc(), "timeout_seconds": limit, "log": path.name}
        self.commands.append(entry)
        self.save()
        start = time.monotonic()
        process = None
        primary = None
        faults = []
        fault = None
        with path.open("wb") as log:
            try:
                process = subprocess.Popen(args, cwd=cwd or ROOT, env=env or self.environment(),
                                           stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT,
                                           start_new_session=True)
                while process.poll() is None:
                    if time.monotonic() - start > limit:
                        fault = "command deadline"
                    elif path.stat().st_size > LOG_CAP:
                        fault = "command log cap (incomplete evidence)"
                    if fault:
                        break
                    time.sleep(0.1)
            except BaseException as error:
                primary = error
                fault = f"interrupted: {type(error).__name__}: {error}"
            finally:
                if process is not None:
                    if process.poll() is None or fault:
                        try:
                            os.killpg(process.pid, signal.SIGKILL)
                        except ProcessLookupError:
                            pass
                        except BaseException as error:
                            faults.append(error)
                    try:
                        process.wait(timeout=10)
                    except BaseException as error:
                        faults.append(error)
        code = process.returncode if process is not None else None
        entry.update({"exit_code": code, "elapsed_seconds": round(time.monotonic() - start, 3),
                      "finished": utc(), "sha256": sha(path), "bytes": path.stat().st_size,
                      "failure": fault, "cleanup_errors": [str(error) for error in faults]})
        self.save()
        if faults:
            raise BaseExceptionGroup("command and owned process cleanup failed", ([primary] if primary else []) + faults)
        if primary:
            raise primary
        require(not fault and path.stat().st_size <= LOG_CAP and code == 0,
                f"setup/runtime command failed: {name}, exit {code}, {fault}; see {path.name}")
        return path.read_text(encoding="utf-8", errors="replace").strip()

    def docker(self, *args, cap=30, cleanup=False, name="docker"):
        require(self.db is not None, "owned container is unavailable")
        self.db.validate_target()
        return self.command(["docker", "--host", "unix:///var/run/docker.sock", *args], name,
                            cap=cap, cleanup=cleanup)

    def copy(self, source, destination):
        require(destination == INSIDE or destination.startswith(INSIDE + "/"),
                "copy target outside owned container directory")
        self.docker("cp", str(source), self.db.container_id + ":" + destination, cap=60, name="copy")

    def download(self, url, destination, digest, cap):
        start = time.monotonic()
        opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
        total = 0
        with opener.open(url, timeout=self.remaining(30)) as response, destination.open("xb") as output:
            final = response.geturl()
            require(urllib.parse.urlsplit(final).scheme == "https", "download redirected away from TLS")
            while True:
                self.remaining()
                require(time.monotonic() - start < 180, "download deadline")
                block = response.read(65536)
                if not block:
                    break
                total += len(block)
                require(total <= cap, "download byte cap")
                output.write(block)
        actual = sha(destination)
        self.setup.setdefault("downloads", []).append({"url": url, "resolved_url": final,
                    "sha256": actual, "expected_sha256": digest, "bytes": total, "finished": utc()})
        self.save()
        require(actual == digest, "pinned download checksum mismatch")

    def tracked(self, directory, name):
        names = self.command(["git", "ls-files"], name, cwd=directory)
        return {p: sha(directory / p) for p in names.splitlines()}

    def prepare(self):
        json_file(self.evidence / "wire-parser-controls.json", parser_controls())
        self.temporary = tempfile.TemporaryDirectory(prefix="glaux-phase1-osh-", dir=os.environ["RUNNER_TEMP"])
        self.work = Path(self.temporary.name).resolve()
        archive = self.work / "jdk.tar.gz"
        self.download(JDK_URL, archive, JDK_SHA, 300 * 1024 * 1024)
        with tarfile.open(archive) as bundle:
            bundle.extractall(self.work, filter="data")
        self.jdk = self.work / "jdk-17.0.16+8"
        require((self.jdk / "bin/java").is_file(), "pinned JDK layout absent")
        gradle_zip = self.work / "gradle.zip"
        self.download(GRADLE_URL, gradle_zip, GRADLE_SHA, 180 * 1024 * 1024)
        with zipfile.ZipFile(gradle_zip) as bundle:
            for item in bundle.infolist():
                target = (self.work / item.filename).resolve()
                require(target.is_relative_to(self.work) and not item.filename.startswith("/"),
                        "unsafe Gradle archive path")
                require((item.external_attr >> 16) & 0o170000 != 0o120000, "Gradle archive symlink")
            bundle.extractall(self.work)
        self.gradle = self.work / "gradle-8.10.2/bin/gradle"
        self.gradle.chmod(0o755)
        self.env = self.environment() | {"JAVA_HOME": str(self.jdk),
                    "PATH": str(self.jdk / "bin") + ":" + self.environment()["PATH"],
                    "GRADLE_USER_HOME": str(self.work / "gradle-home")}
        self.command([str(self.jdk / "bin/java"), "-version"], "java-version", env=self.env)
        self.command([str(self.gradle), "--version"], "gradle-version", env=self.env)
        self.peer = self.work / "osh"
        self.peer.mkdir()
        for args in (["init"], ["remote", "add", "origin", "https://github.com/opensensorhub/osh-core.git"],
                     ["fetch", "--depth", "1", "origin", OSH_PIN], ["checkout", "--detach", "FETCH_HEAD"]):
            self.command(["git", *args], "peer-source", cwd=self.peer, cap=120)
        require(self.command(["git", "rev-parse", "HEAD"], "peer-pin", cwd=self.peer) == OSH_PIN,
                "peer source pin mismatch")
        self.peer_before = self.tracked(self.peer, "peer-source-before")
        json_file(self.evidence / "peer-source-before.json", self.peer_before)
        libs = self.work / "lib"
        libs.mkdir()
        self.env["GLAUX_OSH_LIBS"] = str(libs)
        self.command([str(self.gradle), "--no-daemon", "--max-workers=2", "--console=plain",
                      "-Dorg.gradle.jvmargs=-Xmx512m", "--init-script", str(ASSETS / "collect-runtime.gradle"),
                      "phase1CollectRuntime"], "peer-production-build", cwd=self.peer, env=self.env, cap=720)
        jars = list(libs.glob("*.jar"))
        require(jars, "no production jars collected")
        inventory = {p.name: {"bytes": p.stat().st_size, "sha256": sha(p)} for p in jars}
        json_file(self.evidence / "runtime-jars.json", inventory)
        shutil.copyfile(self.work / "resolved-components.json", self.evidence / "resolved-components.json")
        classes = {"org/sensorhub/impl/SensorHub.class",
                   "org/sensorhub/impl/service/consys/ConSysApiService.class",
                   "org/sensorhub/impl/datastore/h2/MVObsSystemDatabase.class"}
        for jar in jars:
            with zipfile.ZipFile(jar) as bundle:
                classes.difference_update(bundle.namelist())
        require(not classes, "minimal production runtime is missing its entry point/modules")
        # Keep source and notices with disposable binaries; never publish jar bundles.
        notices = self.work / "upstream-notices"
        notices.mkdir()
        for path in self.peer.rglob("*"):
            if path.is_file() and path.name.lower().startswith(("license", "notice", "copying")):
                dest = notices / path.relative_to(self.peer)
                dest.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(path, dest)
        transport = self.work / "transport"
        transport.mkdir()
        self.command([str(self.jdk / "bin/javac"), "--release", "17", "-d", str(transport),
                      str(ASSETS / "RawHttp.java")], "compile-raw-transport", env=self.env)
        server = ROOT / "target/debug/glaux-server"
        require(server.is_file(), "existing lane did not build the Glaux production binary")
        self.setup["glaux_binary"] = {"sha256": sha(server), "path": "target/debug/glaux-server"}
        self.setup["peer_tracked_files"] = len(self.peer_before)
        self.setup["runtime_jar_count"] = len(jars)
        self.save()
        self.db = DisposablePostgis()
        self.db.__enter__()
        self.db.setup()
        self.docker("exec", self.db.container_id, "mkdir", "-m", "700", INSIDE, name="owned-directory")
        self.copy(self.jdk, INSIDE + "/jdk")
        self.copy(libs, INSIDE + "/lib")
        self.copy(notices, INSIDE + "/upstream-notices")
        self.copy(transport, INSIDE + "/transport")
        self.copy(server, INSIDE + "/glaux-server")
        self.copy(ASSETS / "jetty-loopback.xml", INSIDE + "/jetty-loopback.xml")
        self.copy(ASSETS / "logback.xml", INSIDE + "/logback.xml")
        self.write_configuration()
        self.docker("exec", self.db.container_id, "chown", "-R", "postgres:postgres", INSIDE,
                    name="owned-file-permissions")
        self.docker("exec", "--user", "postgres", "--env",
                    "GLAUX_DATABASE_URL=postgres://postgres@localhost/" + TARGET +
                    "?host=/var/run/postgresql&sslmode=disable", self.db.container_id,
                    INSIDE + "/glaux-server", "migrate", name="glaux-migrate", cap=60)
        self.db.probe("""
CREATE ROLE glaux_compare_app LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT;
GRANT USAGE ON SCHEMA public TO glaux_compare_app;
GRANT SELECT ON public._sqlx_migrations TO glaux_compare_app;
GRANT SELECT,INSERT ON public.resource_identity,public.system_identity,public.source_identity,
public.system_parent,public.source_artifact,public.system_revision,public.server_audit,
public.outgoing_work,public.system_write_head,public.system_create_retry TO glaux_compare_app;
GRANT UPDATE(digest,system_id,revision_id,artifact_id,audit_id,event_id,retained_at,expires_at)
ON public.system_create_retry TO glaux_compare_app;
GRANT SELECT,UPDATE ON public.system_parent_write_guard TO glaux_compare_app;
""")
        require(self.db.query("SELECT count(*) FROM public.resource_identity;") == "0",
                "Glaux fixture store not empty")
        require(self.db.query("SELECT rolsuper OR rolcreatedb OR rolcreaterole FROM pg_roles "
                              "WHERE rolname='glaux_compare_app';") == "f", "serving role is privileged")
        self.start("glaux")
        self.start("osh")
        self.setup["ready"] = utc()
        self.save()

    def write_configuration(self):
        glaux = {"listener": "127.0.0.1:18080", "database": {"url_env": "GLAUX_COMPARE_DATABASE"},
                 "health_timeout_ms": 500, "authentication": "development",
                 "development": {"subject": "phase1-comparison", "groups": ["comparison"]},
                 "http": {"public_api_root": self.bases["glaux"]}, "discovery": True,
                 "policy": {"grants": [{"issuer": "urn:glaux:development", "group": "comparison",
                     "source": "urn:glaux:review:phase1:step5", "actions": ["create", "read"]}],
                     "denial_audit": {"max_records": 100, "max_per_window": 100, "window_seconds": 10}},
                 "system_creation": {"source": "urn:glaux:review:phase1:step5", "retry_retention_seconds": 3600}}
        peer = [{"objClass": "org.sensorhub.impl.service.HttpServerConfig", "id": "HTTP_SERVER_0",
                 "moduleClass": "org.sensorhub.impl.service.HttpServer", "name": "Comparison HTTP",
                 "httpPort": 18888, "httpsPort": 0, "servletsRootUrl": "/sensorhub",
                 "staticDocsRootUrl": None, "authMethod": "NONE", "enableCORS": False,
                 "xmlConfigFile": INSIDE + "/jetty-loopback.xml", "autoStart": True},
                {"objClass": "org.sensorhub.impl.datastore.h2.MVObsSystemDatabaseConfig",
                 "id": "comparison-db", "moduleClass": "org.sensorhub.impl.datastore.h2.MVObsSystemDatabase",
                 "name": "Comparison persistent H2", "storagePath": INSIDE + "/observations.dat",
                 "databaseNum": 2, "readOnly": False, "memoryCacheSize": 16384,
                 "autoCommitBufferSize": 1024, "autoCommitPeriod": 1,
                 "idProviderType": "SEQUENTIAL", "autoStart": True},
                {"objClass": "org.sensorhub.impl.service.consys.ConSysApiServiceConfig",
                 "id": "comparison-consys", "moduleClass": "org.sensorhub.impl.service.consys.ConSysApiService",
                 "name": "Comparison Connected Systems", "databaseID": "comparison-db",
                 "endPoint": "/api", "enableTransactional": True, "threadPoolSize": 2,
                 "security": {"objClass": "org.sensorhub.api.security.SecurityConfig",
                              "enableAccessControl": False, "requireAuth": False}, "autoStart": True}]
        for name, value in (("glaux-config.json", glaux), ("osh-config.json", peer)):
            path = self.work / name
            json_file(path, value)
            shutil.copyfile(path, self.evidence / name)
            self.copy(path, INSIDE + "/" + name)
        self.docker("exec", self.db.container_id, "test", "!", "-e", INSIDE + "/observations.dat",
                    name="fresh-h2-target")
        self.setup["configuration"] = {"osh_cache_kib": 16384, "osh_heap_mib": 128,
              "osh_security": "NONE inside network-disabled fixture only",
              "glaux_auth": "explicit development caller; NO identity/authentication request header",
              "osh_persistence": INSIDE + "/observations.dat", "osh_id_scope": 2,
              "osh_id_provider": "SEQUENTIAL; default nonsecure Base32hex encoder"}

    def start(self, peer):
        require(peer in self.bases and peer not in self.processes, "invalid/already running peer")
        require(self.starts[peer] < 2, "only one ordinary restart is authorized")
        self.db.validate_target()
        self.starts[peer] += 1
        if peer == "glaux":
            executable = INSIDE + "/glaux-server"
            command = [executable, "serve", INSIDE + "/glaux-config.json"]
        else:
            executable = INSIDE + "/jdk/bin/java"
            command = [executable, "-Xmx128m", "-XX:ActiveProcessorCount=2", "-Djava.net.preferIPv4Stack=true",
                       "-Dlogback.configurationFile=" + INSIDE + "/logback.xml",
                       "-cp", INSIDE + "/lib/*", "org.sensorhub.impl.SensorHub",
                       INSIDE + "/osh-config.json", INSIDE + "/module-data"]
        pidfile = INSIDE + "/" + peer + ".pid"
        args = ["docker", "--host", "unix:///var/run/docker.sock", "exec", "--user", "postgres",
                "--workdir", INSIDE, "--env", "GLAUX_COMPARE_DATABASE=postgres://glaux_compare_app@localhost/"
                + TARGET + "?host=/var/run/postgresql&sslmode=disable", self.db.container_id,
                "/bin/sh", "-c", 'echo $$ > "$1"; shift; exec "$@"', "owned-comparison", pidfile, *command]
        path = self.evidence / (f"{peer}-start-{self.starts[peer]}.log")
        log = path.open("wb")
        process = subprocess.Popen(args, env=self.environment(), stdin=subprocess.DEVNULL,
                                   stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
        self.processes[peer] = {"process": process, "log": log, "path": path,
                                "executable": executable, "pidfile": pidfile}
        self.setup.setdefault("processes", []).append({"peer": peer, "start": self.starts[peer],
                                                       "command": args, "log": path.name, "started": utc()})
        self.save()
        deadline = time.monotonic() + self.remaining(60)
        last = "no response"
        while time.monotonic() < deadline:
            require(process.poll() is None and path.stat().st_size <= LOG_CAP,
                    f"{peer} stopped or exceeded log cap during startup; see {path.name}")
            try:
                response = self.request(peer, "GET", self.bases[peer].rstrip("/") + "/", {"Accept": "application/json"},
                                        readiness=True)
                if response["status"] == 200:
                    self.verify_listener(peer)
                    return
                last = f"HTTP {response['status']}"
            except HarnessError as error:
                last = str(error)
            time.sleep(0.25)
        raise HarnessError(f"{peer} readiness timed out: {last}")

    def verify_listener(self, peer):
        port = urllib.parse.urlsplit(self.bases[peer]).port
        raw = self.docker("exec", "--user", "postgres", self.db.container_id, "cat", "/proc/net/tcp", "/proc/net/tcp6",
                          name="listener-inspection")
        matched = []
        for line in raw.splitlines():
            columns = line.split()
            if len(columns) > 9 and columns[1].endswith(f":{port:04X}") and columns[3] == "0A":
                require(columns[1].split(":")[0] == "0100007F", "peer bound non-loopback/wrong-family listener")
                matched.append(columns[9])
        require(len(matched) == 1, "missing/duplicate approved peer listener")
        item = self.processes[peer]
        pid = self.docker("exec", "--user", "postgres", self.db.container_id, "cat", item["pidfile"], name="owned-process-id")
        require(re.fullmatch(r"[1-9][0-9]*", pid), "invalid owned process id")
        actual = self.docker("exec", "--user", "postgres", self.db.container_id, "readlink", "-f", f"/proc/{pid}/exe",
                            name="owned-executable")
        require(actual == item["executable"], "serving process executable mismatch")
        fds = self.docker("exec", "--user", "postgres", self.db.container_id, "/bin/sh", "-c",
                         'for fd in /proc/"$1"/fd/*; do readlink "$fd" || true; done',
                         "owned-fds", pid, name="listener-process-ownership")
        require(f"socket:[{matched[0]}]" in fds.splitlines(), "listener not held by owned process")
        item["pid"] = pid
        if self.starts[peer] == 1:
            self.ownership_controls(peer)
        self.setup.setdefault("verified_listeners", []).append({"peer": peer, "pid": pid,
                    "address": f"127.0.0.1:{port}", "inode": matched[0], "start": self.starts[peer]})
        self.save()

    def ownership_controls(self, peer):
        """Use the real PID, same UID and exact shutdown guard, without signalling."""
        item = self.processes[peer]
        prefix = ("exec", "--user", "postgres", self.db.container_id, "/bin/sh", "-c",
                  OWNED_PROCESS_GUARD, "owned-control", item["pidfile"])
        self.docker(*prefix, item["executable"], "probe", name="owned-guard-valid-control")
        positive_log = self.commands[-1]["log"]
        try:
            self.docker(*prefix, INSIDE + "/not-the-serving-executable", "probe",
                        name="owned-guard-wrong-executable-control")
        except HarnessError:
            # A failure to launch/read the process is not sensitivity proof.
            # Only the shared guard's dedicated mismatch exit is acceptable.
            require(self.commands[-1]["exit_code"] == 3 and not self.commands[-1]["failure"],
                    "wrong-executable control failed for a setup/deadline reason")
            negative_log = self.commands[-1]["log"]
            self.commands[-1]["expected_control_outcome"] = "wrong executable rejected, no signal sent"
        else:
            raise HarnessError("owned process guard accepted a wrong executable")
        require(item["process"].poll() is None, "non-signalling guard control stopped the serving process")
        self.docker(*prefix, item["executable"], "probe", name="owned-guard-still-valid-control")
        self.setup.setdefault("ownership_controls", []).append({
            "peer": peer, "uid": "postgres (same as serving process)",
            "valid_executable": "accepted", "wrong_executable": "rejected with guard exit 3",
            "no_signal_sent": True, "correct_identity_rechecked": True,
            "positive_log": positive_log, "negative_log": negative_log,
        })
        self.save()

    def request(self, peer, method, path_or_url, headers=None, body=b"", readiness=False):
        require(peer in self.bases and method in ("GET", "HEAD", "POST"), "unapproved request")
        require(isinstance(body, bytes) and len(body) <= 32768, "request body cap")
        self.remaining()
        for item in self.processes.values():
            require(item["process"].poll() is None and item["path"].stat().st_size <= LOG_CAP,
                    "serving process exited or log cap reached")
        if not readiness:
            self.requests += 1
            require(self.requests <= 60, "approved non-readiness request cap reached")
        url = safe_url(self.bases[peer], path_or_url)
        parsed = urllib.parse.urlsplit(url)
        values = {"Host": parsed.netloc, "Connection": "close", "Accept-Encoding": "identity"}
        for key, value in (headers or {}).items():
            require(isinstance(key, str) and re.fullmatch(r"[!#$%&'*+.^_`|~0-9A-Za-z-]+", key),
                    "invalid request header name")
            require(isinstance(value, str) and not any(ord(c) < 32 or ord(c) == 127 for c in value),
                    "invalid request header value")
            require(key.lower() not in ("host", "connection", "authorization", "proxy-authorization",
                                        "content-length", "transfer-encoding") and
                    not key.lower().startswith("x-glaux"), "reserved/authentication header refused")
            values[key] = value
        if method == "POST":
            values["Content-Length"] = str(len(body))
        target = parsed.path + ("?" + parsed.query if parsed.query else "")
        request = (f"{method} {target} HTTP/1.1\r\n" +
                   "".join(f"{k}: {v}\r\n" for k, v in values.items()) + "\r\n").encode("ascii") + body
        self.serial += 1
        stem = f"{'ready' if readiness else 'request'}-{self.serial:03d}-{peer}-{method.lower()}"
        (self.evidence / (stem + ".request.bin")).write_bytes(request)
        record = {"peer": peer, "url": url, "method": method, "readiness": readiness,
                  "started": utc(), "request": stem + ".request.bin", "complete": False}
        json_file(self.evidence / (stem + ".json"), record)
        output = self.docker("exec", "--user", "postgres", self.db.container_id,
                             INSIDE + "/jdk/bin/java", "-Xmx32m", "-XX:ActiveProcessorCount=1",
                             "-Djava.net.preferIPv4Stack=true",
                             "-cp", INSIDE + "/transport", "RawHttp", str(parsed.port),
                             base64.b64encode(request).decode(), name="raw-http", cap=20)
        try:
            raw = base64.b64decode(output, validate=True)
        except ValueError as error:
            raise HarnessError("raw transport output was not base64") from error
        path = self.evidence / (stem + ".response.bin")
        path.write_bytes(raw)
        record.update({"response": path.name, "bytes": len(raw), "sha256": sha(path), "finished": utc()})
        json_file(self.evidence / (stem + ".json"), record)
        status, response_headers, entity = parse_wire(raw, method)
        record.update({"status": status, "headers": response_headers, "complete": True})
        json_file(self.evidence / (stem + ".json"), record)
        return {"status": status, "headers": response_headers, "body": entity,
                "raw_path": path.name, "url": url, "request_path": stem + ".request.bin"}

    def stop(self, peer, cleanup=False):
        item = self.processes.get(peer)
        if not item:
            return
        process = item["process"]
        if process.poll() is None:
            self.docker("exec", "--user", "postgres", self.db.container_id, "/bin/sh", "-c",
                        OWNED_PROCESS_GUARD, "owned-stop", item["pidfile"], item["executable"], "stop",
                        name="owned-stop", cleanup=cleanup)
            try:
                process.wait(timeout=self.remaining(25, cleanup))
            except subprocess.TimeoutExpired as error:
                raise HarnessError(f"owned {peer} process did not stop gracefully") from error
        item["log"].close()
        require(item["path"].stat().st_size <= LOG_CAP, "serving log cap exceeded")
        # docker exec may propagate signal termination (143); OSH shutdown hook
        # must separately report clean persistence before a restart is accepted.
        require(process.returncode in (0, 143), f"{peer} process exited {process.returncode}")
        if peer == "osh":
            require("SensorHub was cleanly stopped" in item["path"].read_text(errors="replace"),
                    "OSH clean shutdown evidence absent")
            self.docker("exec", self.db.container_id, "test", "-s", INSIDE + "/observations.dat",
                        name="persistent-h2-file", cleanup=cleanup)
        del self.processes[peer]

    def restart(self, peer):
        require(peer in self.bases and self.starts[peer] == 1, "restart already used or peer not started")
        self.stop(peer)
        self.start(peer)

    def __enter__(self):
        try:
            self.prepare()
            return self
        except BaseException as primary:
            try:
                self.close()
            except BaseException as cleanup:
                raise BaseExceptionGroup("setup and cleanup both failed", [primary, cleanup])
            raise

    def __exit__(self, exc_type, primary, traceback):
        try:
            self.close()
        except BaseException as cleanup:
            if primary:
                raise BaseExceptionGroup("comparison and cleanup both failed", [primary, cleanup])
            raise
        return False

    def close(self):
        # The outer execution alarm reserves 120 seconds inside the same total
        # budget. Do not let that execution alarm interrupt ownership-checked
        # cleanup; all cleanup commands remain individually bounded.
        signal.setitimer(signal.ITIMER_REAL, 0)
        self.cleanup_deadline = min(time.monotonic() + 100, self.deadline + 110)
        failures = []
        for peer in list(self.processes):
            try:
                self.stop(peer, cleanup=True)
            except BaseException as error:
                failures.append(error)
        if self.db:
            try:
                self.db.close()  # unchanged ownership checks and fatal cleanup
            except BaseException as error:
                failures.append(error)
        for item in self.processes.values():
            try:
                item["process"].wait(timeout=10)
                item["log"].close()
            except BaseException as error:
                failures.append(error)
        if self.peer_before is not None:
            try:
                after = {p: sha(self.peer / p) for p in self.peer_before}
                json_file(self.evidence / "peer-source-after.json", after)
                require(after == self.peer_before, "peer build modified tracked source")
            except BaseException as error:
                failures.append(error)
        if self.temporary:
            try:
                # TemporaryDirectory owns this exact fresh directory; no user path
                # or workspace root is accepted as a cleanup target.
                self.temporary.cleanup()
            except BaseException as error:
                failures.append(error)
        self.setup.update({"finished": utc(), "non_readiness_requests": self.requests,
                           "cleanup": "failed" if failures else "complete",
                           "cleanup_errors": [str(error) for error in failures]})
        self.save()
        if failures:
            raise BaseExceptionGroup("bounded comparison cleanup/preservation failed", failures)
