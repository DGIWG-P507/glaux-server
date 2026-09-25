"""Hosted-only SQLx/System storage proof and independent PostgreSQL controls.

No caller database/container/URL is accepted. The Rust binary runs inside the
existing validated, owned, network-isolated harness container. SQL sensitivity
controls use transactional DDL and roll back; ordering comes from observed
backend/lock states, never an assumed sleep interval.
"""

import os
from pathlib import Path
import subprocess
import sys
import time

from database_harness import DisposablePostgis, HarnessError, TARGET, docker


ROOT = Path(__file__).resolve().parents[1]
PROBE = ROOT / "target/debug/examples/system-storage-proof"
SERVER = ROOT / "target/debug/glaux-server"
GROUPS = (
    "migration-initial-preservation", "exact-identity-and-lookups", "conflicts-atomic",
    "typed-parent-rollback", "long-lexical-identities", "migration-reapply-preservation",
    "schema-compatibility-read-only",
)
FINAL = "Required System storage proof passed: 7 groups."
A, B, BARE, NEW, MISSING = (
    "00000000-0000-7000-8000-00000000" + suffix
    for suffix in ("a001", "a002", "a003", "a004", "a099")
)


def require(condition, message):
    if not condition:
        raise HarnessError(message)


def quote(text):
    return "'" + text.replace("'", "''") + "'"


def identity(local_id, uid):
    return ("INSERT INTO public.resource_identity(id,family,uid) VALUES (" +
            quote(local_id) + ",'system'," + quote(uid) + ");")


def edge(child, parent):
    return ("INSERT INTO public.system_parent(child_id,parent_id) VALUES (" +
            quote(child) + "," + quote(parent) + ");")


def rejected(sql, state, detail):
    # A missing table, syntax error, timeout or unrelated error cannot satisfy
    # this control. Unexpected acceptance raises a different SQLSTATE.
    return ("DO $proof$ BEGIN BEGIN " + sql +
            " RAISE EXCEPTION 'Expected rejection did not occur'; " +
            "EXCEPTION WHEN SQLSTATE " + quote(state) + " THEN " +
            "IF position(" + quote(detail) + " in SQLERRM)=0 THEN RAISE; END IF; " +
            "END; END $proof$;")


def snapshot(db):
    return db.query("SELECT json_build_object(" + ",".join(
        quote(table) + ",(SELECT coalesce(json_agg(row_to_json(t) ORDER BY " + order +
        "),'[]'::json) FROM public." + table + " t)"
        for table, order in (
            ("resource_identity", "id"), ("system_identity", "id"),
            ("source_identity", "resource_id,authority,identifier"),
            ("system_parent", "child_id"),
        )
    ) + ")")


def rust_proof(db):
    db.validate_target()
    docker("cp", str(PROBE), db.container_id + ":/tmp/glaux-system-storage-proof")
    db.validate_target()
    output = docker("exec", "--user", "postgres", db.container_id,
                    "/tmp/glaux-system-storage-proof", timeout=120)
    print(output, flush=True)
    expected = ["System storage group passed: " + group for group in GROUPS]
    observed = [line for line in output.splitlines()
                if line.startswith("System storage group passed:")]
    require(observed == expected, "Rust System storage groups missing, duplicated or reordered")
    require(output.splitlines().count(FINAL) == 1, "Rust System storage final marker missing/duplicated")


def cli(db, command):
    db.validate_target()
    return docker("exec", "--user", "postgres", "--env",
                  "GLAUX_DATABASE_URL=postgresql:///glaux_harness_test?host=/var/run/postgresql&user=postgres",
                  db.container_id, "/tmp/glaux-storage-server", command, timeout=40)


def cli_before_migration(db):
    db.validate_target()
    docker("cp", str(SERVER), db.container_id + ":/tmp/glaux-storage-server")
    absent = ("SELECT to_regclass('public._sqlx_migrations') IS NULL AND "
              "to_regclass('public.resource_identity') IS NULL")
    require(db.query(absent) == "t", "CLI initial schema fixture is not unmigrated")
    try:
        cli(db, "check-schema")
    except HarnessError as error:
        require("Docker exec failed (1)" in str(error) and
                "glaux-server: database schema is not compatible." in str(error),
                "CLI failed for an unintended reason: " + str(error))
    else:
        raise HarnessError("CLI accepted an incompatible schema")
    require(db.query(absent) == "t", "CLI check-schema unexpectedly applied migrations")
    try:
        db.validate_target()
        docker("exec", "--user", "postgres", "--env",
               "GLAUX_DATABASE_URL=postgres://postgres@127.0.0.1:5432/glaux_harness_test?sslmode=disable",
               db.container_id, "/tmp/glaux-storage-server", "migrate", timeout=40)
    except HarnessError as error:
        require("Docker exec failed (1)" in str(error) and
                "glaux-server: database operation failed." in str(error),
                "Network TLS control failed for an unintended reason: " + str(error))
    else:
        raise HarnessError("Network CLI allowed the caller to downgrade TLS")
    require(db.query(absent) == "t", "Rejected network migration changed the database")


def cli_after_migration(db):
    before = snapshot(db)
    for command in ("migrate", "check-schema"):
        require(cli(db, command) == "Database command completed.",
                "Explicit administrative CLI did not report actual completion")
    require(snapshot(db) == before, "CLI migration reapplication changed existing identity data")
    print("System storage CLI checks passed: initial rejection, reapply preservation, compatible check",
          flush=True)


def cli_fresh_migration(db):
    # Reapplication alone would not catch a command that falsely reports success
    # without calling migrate. Reset only this harness-owned synthetic database.
    db.reset()
    cli_before_migration(db)
    require(cli(db, "migrate") == "Database command completed.",
            "Explicit CLI migration did not complete on a fresh target")
    require(db.query("SELECT string_agg(version::text,',' ORDER BY version) "
                     "FROM public._sqlx_migrations WHERE success") == "1,2,3,4,5,6",
            "CLI did not apply every packaged migration to the fresh target")
    require(db.query("SELECT string_agg(tablename,',' ORDER BY tablename) FROM pg_tables "
                     "WHERE schemaname='public' AND tablename IN "
                     "('resource_identity','source_identity','system_identity',"
                     "'system_parent','system_parent_write_guard')") ==
            "resource_identity,source_identity,system_identity,system_parent,system_parent_write_guard",
            "Fresh CLI migration did not create the complete initial schema")
    require(cli(db, "check-schema") == "Database command completed.",
            "Fresh explicit CLI migration is not compatible")
    print("System storage CLI checks passed: fresh-target explicit migration", flush=True)


def sensitivity_controls(db):
    db.query(identity(A, "urn:glaux:runner:a") + identity(B, "urn:glaux:runner:b") +
             identity(BARE, "urn:glaux:runner:bare") +
             f"INSERT INTO public.system_identity(id,label) VALUES ('{A}','same label'),"
             f"('{B}','same label'); INSERT INTO public.source_identity VALUES "
             f"('{A}','runner authority','source-7');")
    cases = (
        ("local-id", identity(A, "urn:glaux:runner:rejected-id"), "23505", "resource_identity_pkey",
         "ALTER TABLE public.resource_identity DROP CONSTRAINT resource_identity_pkey CASCADE;"),
        ("uid", identity(NEW, "urn:glaux:runner:a"), "23P01", "resource_uid_unique",
         "ALTER TABLE public.resource_identity DROP CONSTRAINT resource_uid_unique;"),
        ("source", identity(NEW, "urn:glaux:runner:new") +
         f"INSERT INTO public.source_identity VALUES ('{NEW}','runner authority','source-7');",
         "23P01", "source_pair_unique",
         "ALTER TABLE public.source_identity DROP CONSTRAINT source_pair_unique;"),
        ("missing-parent", edge(A, MISSING), "23503", "system_parent_parent_id_fkey",
         "ALTER TABLE public.system_parent DROP CONSTRAINT system_parent_parent_id_fkey;"),
        ("identity-is-not-system-parent", edge(A, BARE), "23503", "system_parent_parent_id_fkey",
         "ALTER TABLE public.system_parent DROP CONSTRAINT system_parent_parent_id_fkey;"),
        ("identity-is-not-system-child", edge(BARE, A), "23503", "system_parent_child_id_fkey",
         "ALTER TABLE public.system_parent DROP CONSTRAINT system_parent_child_id_fkey;"),
    )
    for name, invalid, state, detail, disable in cases:
        before = snapshot(db)
        db.query(rejected(invalid, state, detail))
        require(snapshot(db) == before, name + " rejection left partial changes")
        require(db.query("BEGIN; " + disable + invalid +
                         " SELECT 'invalid-control-accepted'; ROLLBACK;") ==
                "invalid-control-accepted", name + " disabled-check control did not execute")
        require(snapshot(db) == before, name + " rollback failed to restore exact rows")
        db.query(rejected(invalid, state, detail))
        print("System storage SQL control passed: " + name, flush=True)
    db.query(edge(A, B))
    before = snapshot(db)
    invalid = edge(B, A)
    db.query(rejected(invalid, "23514", "system parent cycle"))
    require(snapshot(db) == before, "Cycle rejection left partial changes")
    require(db.query("BEGIN; ALTER TABLE public.system_parent DISABLE TRIGGER "
                     "system_parent_cycle_check; " + invalid +
                     " SELECT 'cycle-control-accepted'; ROLLBACK;") ==
            "cycle-control-accepted", "Disabled-cycle control did not execute")
    require(snapshot(db) == before, "Cycle-control rollback failed to restore exact rows")
    # Rejection after transactional DDL proves the checks were restored too.
    db.query(rejected(invalid, "23514", "system parent cycle"))
    print("System storage SQL control passed: cycle", flush=True)
    existing_parent = "01890f20-7b5a-7cc3-98c4-dc0c0c073901"
    db.query(rejected(edge(A, existing_parent), "23505", "system_parent_pkey"))
    db.query(rejected(edge(B, B), "23514", "system parent cycle"))
    db.query("BEGIN; DELETE FROM public.system_parent_write_guard; " +
             rejected(edge(B, existing_parent), "23514", "system parent guard missing") +
             "ROLLBACK;")
    require(snapshot(db) == before, "Parent invariant rejection changed existing rows")
    require(db.query("SELECT count(*) FROM public.system_parent_write_guard WHERE singleton") == "1",
            "Missing-guard probe was not rolled back")
    print("System storage parent invariant checks passed: cardinality, self, guard", flush=True)


def hash_collision_control(db):
    pair = db.query("BEGIN; SET LOCAL statement_timeout=10000; WITH candidates AS "
                    "(SELECT 'urn:glaux:runner:collision:' || i AS uid "
                    "FROM generate_series(1,400000) i) "
                    "SELECT min(uid) || chr(9) || max(uid) FROM candidates "
                    "GROUP BY hashtext(uid COLLATE \"C\") HAVING count(*)>1 "
                    "ORDER BY min(uid) LIMIT 1; ROLLBACK;").split("\t")
    require(len(pair) == 2 and pair[0] != pair[1], "Bounded hash collision search found no pair")
    require(db.query("SELECT (hashtext(" + quote(pair[0]) + " COLLATE \"C\") = hashtext(" +
                     quote(pair[1]) + " COLLATE \"C\"))::text") == "true",
            "Collision fixture does not share PostgreSQL's index hash")
    ids = ["00000000-0000-7000-8000-00000000a01" + str(i) for i in range(3)]
    before = snapshot(db)
    output = db.query("BEGIN; " + identity(ids[0], pair[0]) + identity(ids[1], pair[1]) +
                      rejected(identity(ids[2], pair[0]), "23P01", "resource_uid_unique") +
                      "SELECT uid FROM public.resource_identity WHERE id IN (" +
                      quote(ids[0]) + "," + quote(ids[1]) + ") ORDER BY id; ROLLBACK;")
    require(output.splitlines() == pair, "Hash collision conflated distinct full identities")
    require(snapshot(db) == before, "Hash-control rollback failed to restore exact rows")
    print("System storage SQL control passed: full-equality-after-hash-collision", flush=True)


class Session:
    def __init__(self, db, name, sql, isolation):
        db.validate_target()
        # PostgreSQL truncates application_name to 63 bytes. Keep the distinct
        # holder/waiter suffix inside that bound or the lock barrier is invalid.
        self.name = "glaux_storage_" + db.nonce[:12] + "_" + name
        self.finished = False
        environment = {key: value for key, value in os.environ.items()
                       if not key.startswith("DOCKER_")}
        self.process = subprocess.Popen(
            ["docker", "--host", "unix:///var/run/docker.sock", "exec", "-i", "--user", "postgres",
             "--env", "PGAPPNAME=" + self.name,
             "--env", "PGCONNECT_TIMEOUT=2",
             "--env", "PGOPTIONS=-c statement_timeout=15000 -c lock_timeout=10000",
             db.container_id, "psql", "-X", "-w", "-qAt", "--host=127.0.0.1",
             "--username=postgres", "--dbname=" + TARGET, "--set=ON_ERROR_STOP=1",
             "--set=VERBOSITY=verbose", "--file=-"],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            text=True, env=environment,
        )
        require(isolation in ("READ COMMITTED", "REPEATABLE READ"), "Unexpected test isolation")
        self.process.stdin.write("BEGIN ISOLATION LEVEL " + isolation + ";\n" + sql + "\n")
        self.process.stdin.flush()

    def finish(self, sql=""):
        out, err = self.process.communicate(sql, timeout=20)
        self.finished = True
        return self.process.returncode, out, err


def wait_for(db, sql, description, processes):
    deadline = time.monotonic() + 8
    while time.monotonic() < deadline:
        require(all(p.process.poll() is None for p in processes),
                description + ": psql exited before the required state")
        if db.query(sql) == "t":
            return
    raise HarnessError(description + ": deadline without the required backend/lock state")


def race(db, name, first_sql, second_sql, state, detail, expected_sql, expected,
         isolation="READ COMMITTED"):
    sessions = []
    primary = None
    try:
        first = Session(db, name + "_holder", first_sql, isolation)
        sessions.append(first)
        wait_for(db, "SELECT EXISTS(SELECT FROM pg_stat_activity WHERE application_name=" +
                 quote(first.name) + " AND state='idle in transaction')", name + " holder", sessions)
        second = Session(db, name + "_waiter", second_sql, isolation)
        sessions.append(second)
        wait_for(db, "SELECT EXISTS(SELECT FROM pg_stat_activity w JOIN pg_stat_activity h "
                 "ON h.pid=ANY(pg_blocking_pids(w.pid)) WHERE w.application_name=" +
                 quote(second.name) + " AND h.application_name=" + quote(first.name) +
                 " AND w.wait_event_type='Lock')", name + " competing writer", sessions)
        code, out, err = first.finish("COMMIT;\n")
        require(code == 0, name + " first transaction failed: " + err + out)
        code, out, err = second.finish()
        require(code != 0 and state in err and detail in err,
                name + " competitor did not fail at its intended constraint: " + err + out)
        require(db.query(expected_sql) == expected, name + " committed state differs from expected")
    except BaseException as error:
        primary = error
        raise
    finally:
        cleanup_errors = []
        if sessions:
            # Only backends named for these sessions in this owned test DB.
            try:
                db.query("SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE datname=" +
                         quote(TARGET) + " AND application_name IN (" +
                         ",".join(quote(s.name) for s in sessions) + ") AND pid<>pg_backend_pid();")
            except BaseException as error:
                cleanup_errors.append(error)
            for session in sessions:
                try:
                    if not session.finished:
                        if session.process.poll() is None:
                            session.process.kill()
                        session.process.communicate(timeout=5)
                except BaseException as error:
                    cleanup_errors.append(error)
        if cleanup_errors:
            raise BaseExceptionGroup("Concurrency proof cleanup failed",
                                     ([primary] if primary else []) + cleanup_errors)
    print("System storage concurrency passed: " + name, flush=True)


def concurrency_controls(db):
    x, y = ("00000000-0000-7000-8000-00000000a02" + str(i) for i in range(2))
    race(db, "duplicate-uid", identity(x, "urn:glaux:runner:race"),
         identity(y, "urn:glaux:runner:race"), "23P01", "resource_uid_unique",
         f"SELECT id::text || '|' || uid FROM public.resource_identity WHERE id IN ('{x}','{y}') ORDER BY id",
         x + "|urn:glaux:runner:race")
    e, f = ("00000000-0000-7000-8000-00000000a04" + str(i) for i in range(2))
    race(db, "duplicate-source-pair",
         identity(e, "urn:glaux:runner:race-source-1") +
         f"INSERT INTO public.source_identity VALUES ('{e}','race authority','race source');",
         identity(f, "urn:glaux:runner:race-source-2") +
         f"INSERT INTO public.source_identity VALUES ('{f}','race authority','race source');",
         "23P01", "source_pair_unique",
         "SELECT r.id::text || '|' || r.uid || '|' || coalesce(s.authority,'NULL') || '|' || "
         "coalesce(s.identifier,'NULL') FROM public.resource_identity r "
         "LEFT JOIN public.source_identity s ON s.resource_id=r.id "
         f"WHERE r.id IN ('{e}','{f}') ORDER BY r.id",
         e + "|urn:glaux:runner:race-source-1|race authority|race source")
    c, d = ("00000000-0000-7000-8000-00000000a03" + str(i) for i in range(2))
    db.query(identity(c, "urn:glaux:runner:cycle-c") + identity(d, "urn:glaux:runner:cycle-d") +
             f"INSERT INTO public.system_identity(id,label) VALUES ('{c}','C'),('{d}','D');")
    race(db, "opposing-parent-edges", edge(c, d), edge(d, c), "23514", "system parent cycle",
         f"SELECT child_id::text || '|' || parent_id::text FROM public.system_parent "
         f"WHERE child_id IN ('{c}','{d}') ORDER BY child_id", c + "|" + d)
    g, h = ("00000000-0000-7000-8000-00000000a05" + str(i) for i in range(2))
    db.query(identity(g, "urn:glaux:runner:repeatable-g") + identity(h, "urn:glaux:runner:repeatable-h") +
             f"INSERT INTO public.system_identity(id,label) VALUES ('{g}','G'),('{h}','H');")
    race(db, "repeatable-read-parent-edges", edge(g, h), edge(h, g), "40001",
         "could not serialize access due to concurrent update",
         f"SELECT child_id::text || '|' || parent_id::text FROM public.system_parent "
         f"WHERE child_id IN ('{g}','{h}') ORDER BY child_id", g + "|" + h,
         isolation="REPEATABLE READ")


def main():
    require(not sys.argv[1:], "No selection or target override is accepted")
    build = subprocess.run(
        ["cargo", "build", "--locked", "--offline", "-p", "glaux-server", "--example",
         "system-storage-proof", "--bin", "glaux-server", "--target-dir", str(ROOT / "target")],
        cwd=ROOT, capture_output=True, text=True, timeout=180, check=False,
    )
    print(build.stdout + build.stderr, end="", flush=True)
    require(build.returncode == 0 and PROBE.is_file() and SERVER.is_file(),
            "Required System proof and administrative CLI failed to build")
    with DisposablePostgis() as db:
        db.setup()
        cli_before_migration(db)
        rust_proof(db)
        cli_after_migration(db)
        sensitivity_controls(db)
        hash_collision_control(db)
        concurrency_controls(db)
        cli_fresh_migration(db)
    print("System storage: 7 Rust groups, 8 SQL controls, 4 races passed; 0 failed; 0 skipped",
          flush=True)
    print("System storage database: all required checks passed.", flush=True)


if __name__ == "__main__":
    main()
