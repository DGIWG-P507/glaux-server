"""Disposable real PostgreSQL/PostGIS lifecycle, not application storage.

Runs only against containers this process creates on the local Docker daemon.
No user URL, container, database name, host mount or published port is accepted.
The two fault arguments are narrow test hooks; they never suppress a failure.
"""

import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import time
import uuid

ROOT = Path(__file__).resolve().parents[1]
PIN = json.loads((ROOT / "scripts/database-image.json").read_text())
MIGRATION = ROOT / "crates/glaux-server/migrations/0001_enable_postgis.sql"
CONTROL = "glaux_harness_control"
TARGET = "glaux_harness_test"
LABEL = "org.glaux.database-test-owner"


class HarnessError(RuntimeError):
    """A required lifecycle operation failed; never a skipped test."""


def docker(*args, input=None, timeout=15):
    # Never inherit a remote daemon or Docker context from the user's shell.
    environment = {
        key: value for key, value in os.environ.items()
        if not key.startswith("DOCKER_")
    }
    try:
        result = subprocess.run(
            ["docker", "--host", "unix:///var/run/docker.sock", *args],
            input=input, text=True, capture_output=True, timeout=timeout,
            env=environment, check=False,
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        raise HarnessError(f"Docker {args[0]} unavailable/timeout: {error}") from error
    if result.returncode:
        raise HarnessError(
            f"Docker {args[0]} failed ({result.returncode}): "
            f"{result.stderr.strip()} {result.stdout.strip()}"
        )
    return result.stdout.strip()


class DisposablePostgis:
    def __init__(self):
        self.nonce = uuid.uuid4().hex
        self.name = "glaux-db-test-" + self.nonce
        self.container_id = None
        self.ready = False

    def validate_target(self):
        if not self.container_id or not re.fullmatch(r"[a-f0-9]{64}", self.container_id):
            raise HarnessError("Refusing lifecycle operation: no owned container identity")
        details = json.loads(docker("inspect", self.container_id))[0]
        config, host = details["Config"], details["HostConfig"]
        valid = (
            details["Id"] == self.container_id
            and details["Name"] == "/" + self.name
            and config["Labels"].get(LABEL) == self.nonce
            and config["Image"] == PIN["image"]
            and host["NetworkMode"] == "none"
            and not host["PortBindings"]
            and not host["Binds"]
            and host["Tmpfs"].get(PIN["tmpfs"]) == "rw,size=512m"
            and all(mount["Type"] == "tmpfs" for mount in details["Mounts"])
        )
        if not valid:
            raise HarnessError("Refusing lifecycle operation: disposable target mismatch")

    def __enter__(self):
        try:
            self.container_id = docker(
                "create", "--pull=never", "--platform", PIN["platform"],
                "--name", self.name, "--label", f"{LABEL}={self.nonce}",
                "--network", "none", "--tmpfs", PIN["tmpfs"] + ":rw,size=512m",
                "--memory", "768m", "--cpus", "1",
                "--env", "POSTGRES_HOST_AUTH_METHOD=trust",
                "--env", "POSTGRES_DB=" + CONTROL,
                "--env", "PGDATA=" + PIN["pgdata"],
                PIN["image"], timeout=30,
            )
            self.validate_target()
            docker("start", self.container_id)
            deadline = time.monotonic() + 45
            last_error = "No database response"
            while time.monotonic() < deadline:
                try:
                    # TCP loopback waits for the final server, not entrypoint's
                    # temporary socket-only initialization server.
                    identity = self.probe(
                        "SELECT current_database() || '|' || current_user || '|' || "
                        "current_setting('server_version_num');", database=CONTROL,
                    )
                    expected = f"{CONTROL}|postgres|{PIN['postgres_version_num']}"
                    if identity != expected:
                        raise HarnessError(f"Database identity/version mismatch: {identity}")
                    print(f"Ready: {identity}; target={self.container_id}", flush=True)
                    return self
                except HarnessError as error:
                    last_error = str(error)
                    time.sleep(0.2)  # Bounded polling; only SQL establishes readiness.
            raise HarnessError(f"Database readiness deadline exceeded: {last_error}")
        except BaseException as primary:
            try:
                if self.container_id:
                    self.close()
            except BaseException as cleanup:
                raise BaseExceptionGroup("Setup and cleanup both failed", [primary, cleanup])
            raise

    def __exit__(self, exc_type, primary, traceback):
        try:
            self.close()
        except BaseException as cleanup:
            if primary:
                raise BaseExceptionGroup("Test and cleanup both failed", [primary, cleanup])
            raise
        return False

    def probe(self, sql, database=TARGET):
        """Diagnostic SQL in this owned cluster; not an application query API."""
        self.validate_target()
        if database not in (TARGET, CONTROL, "postgres"):
            raise HarnessError("Refusing unrecognised database target")
        return docker(
            "exec", "-i", "--user", "postgres",
            "--env", "PGCONNECT_TIMEOUT=2",
            "--env", "PGOPTIONS=-c statement_timeout=5000 -c lock_timeout=1000",
            self.container_id, "psql", "-X", "-w", "-qAt",
            "--host=127.0.0.1", "--username=postgres", "--dbname=" + database,
            "--set=ON_ERROR_STOP=1", "--file=-", input=sql,
        )

    def setup(self, fault=False):
        self.ready = False
        try:
            self.probe("CREATE DATABASE " + TARGET + " TEMPLATE template0;", database=CONTROL)
            sql = MIGRATION.read_text()
            print("Migration SHA256: " + hashlib.sha256(MIGRATION.read_bytes()).hexdigest(),
                  flush=True)
            self.probe("BEGIN;\n" + sql + ("\nSELECT 1/0;" if fault else "") + "\nCOMMIT;")
            self.ready = True
        except HarnessError as error:
            raise HarnessError(f"Setup failed; fixtures unavailable: {error}") from error

    def query(self, sql):
        if not self.ready:
            raise HarnessError("Fixtures unavailable: setup/reset did not succeed")
        return self.probe(sql)

    def reset(self, fault=False):
        self.ready = False
        try:
            self.validate_target()
            if fault:
                self.probe("SELECT 1/0;", database=CONTROL)
            # Deliberate first-run defect: leaves old mutable fixtures in place.
            # The independently written reset test must fail before this is fixed.
            self.ready = True
        except HarnessError as error:
            raise HarnessError(f"Reset failed; fixtures unavailable: {error}") from error

    def close(self, fault=False):
        self.ready = False
        if not self.container_id:
            return
        self.validate_target()
        if fault:
            raise HarnessError("Cleanup failed: injected removal failure; target retained")
        removed = self.container_id
        docker("rm", "--force", "--volumes", removed, timeout=30)
        # Successful removal alone is not enough: verify that exact ID is absent.
        remaining = docker("ps", "--all", "--quiet", "--no-trunc", "--filter", "id=" + removed)
        if remaining:
            raise HarnessError("Cleanup failed: removed container is still present")
        self.container_id = None
        print("Cleaned owned target: " + removed, flush=True)
