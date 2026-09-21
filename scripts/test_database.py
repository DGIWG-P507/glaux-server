"""Real disposable PostgreSQL/PostGIS lifecycle checks for task 1.1.3.

These are harness checks through the image's psql client, not Rust storage tests
or CSAPI behavior. All rows and clock inputs below are synthetic and fixed.
"""

import json
import sys
import unittest

from database_harness import MIGRATION, PIN, DisposablePostgis, HarnessError, docker


SEED_TIME = "2026-09-21T12:34:56.123456Z"
TARGET_DATABASE = "glaux_harness_test"
CONTROL_DATABASE = "glaux_harness_control"


def seed_marker(db):
    """Use known coordinates, value and time rather than a runtime clock/random row."""
    db.query(
        "CREATE TABLE harness_marker (marker text PRIMARY KEY, value numeric NOT NULL, "
        "observed_at timestamptz NOT NULL, location geometry(Point,4326) NOT NULL); "
        "INSERT INTO harness_marker VALUES "
        "('first', 12.5, TIMESTAMPTZ '2026-09-21T12:34:56.123456Z', "
        "ST_SetSRID(ST_MakePoint(4,5),4326));"
    )


def create_control(db):
    """An existing, non-target database inside this same disposable container."""
    db.probe(
        "CREATE TABLE protected_marker (marker text PRIMARY KEY); "
        "INSERT INTO protected_marker VALUES ('must-survive-reset');",
        database=CONTROL_DATABASE,
    )


class DatabaseLifecycleTests(unittest.TestCase):
    def assert_control_preserved(self, db):
        self.assertEqual(
            db.probe("SELECT marker FROM protected_marker ORDER BY marker", database=CONTROL_DATABASE),
            "must-survive-reset",
        )

    def test_identity_migration_and_exact_seed(self):
        with DisposablePostgis() as db:
            self.assertFalse(db.ready)
            self.assertEqual(
                db.probe("SELECT count(*) FROM pg_database WHERE datname='glaux_harness_test'", database="postgres"),
                "0",
            )
            db.setup()
            self.assertTrue(db.ready)
            self.assertEqual(db.query("SELECT current_database()"), TARGET_DATABASE)
            self.assertEqual(db.query("SHOW server_version_num"), str(PIN["postgres_version_num"]))
            self.assertEqual(db.query("SELECT postgis_lib_version()"), PIN["postgis_version"])
            print("Actual PostGIS: " + db.query("SELECT postgis_full_version()"), flush=True)
            self.assertEqual(
                json.loads(db.query(
                    "SELECT json_build_object('name',e.extname,'version',e.extversion,'schema',n.nspname) "
                    "FROM pg_extension e JOIN pg_namespace n ON n.oid=e.extnamespace "
                    "WHERE e.extname='postgis'"
                )),
                {"name": "postgis", "version": PIN["postgis_version"], "schema": "public"},
            )
            self.assertIn("CREATE EXTENSION IF NOT EXISTS postgis;", MIGRATION.read_text(encoding="utf-8"))
            seed_marker(db)
            self.assertEqual(
                json.loads(db.query(
                    "SELECT json_build_object('marker',marker,'value',value,"
                    "'time',to_char(observed_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"'),"
                    "'x',ST_X(location),'y',ST_Y(location),'srid',ST_SRID(location),"
                    "'wkt',ST_AsText(location)) FROM harness_marker"
                )),
                {"marker": "first", "value": 12.5, "time": SEED_TIME,
                 "x": 4, "y": 5, "srid": 4326, "wkt": "POINT(4 5)"},
            )

    def test_reset_repeated_and_simultaneous_fixtures_are_isolated(self):
        with DisposablePostgis() as first:
            first.setup()
            first_id = first.container_id
            seed_marker(first)
            with DisposablePostgis() as second:
                second.setup()
                self.assertNotEqual(first.container_id, second.container_id)
                self.assertNotEqual(first.nonce, second.nonce)
                self.assertEqual(second.query("SELECT to_regclass('public.harness_marker') IS NULL"), "t")
                seed_marker(second)
                second.query("UPDATE harness_marker SET marker='second'")
                self.assertEqual(first.query("SELECT marker FROM harness_marker"), "first")
                self.assertEqual(second.query("SELECT marker FROM harness_marker"), "second")
                first.reset()
                self.assertTrue(first.ready)
                self.assertEqual(first.query("SELECT to_regclass('public.harness_marker') IS NULL"), "t")
                self.assertEqual(first.query("SELECT postgis_lib_version()"), PIN["postgis_version"])
                self.assertEqual(second.query("SELECT marker FROM harness_marker"), "second")
            # Repeating reset on an already clean fixture must remain usable.
            first.reset()
            self.assertEqual(first.query("SELECT to_regclass('public.harness_marker') IS NULL"), "t")
        with DisposablePostgis() as repeated:
            repeated.setup()
            self.assertNotEqual(first_id, repeated.container_id)
            self.assertEqual(repeated.query("SELECT to_regclass('public.harness_marker') IS NULL"), "t")

    def test_setup_failure_rolls_back_migration_and_blocks_queries(self):
        with DisposablePostgis() as db:
            with self.assertRaises(HarnessError) as failed:
                db.setup(fault=True)
            self.assertIn("division by zero", str(failed.exception))
            self.assertFalse(db.ready)
            self.assertEqual(db.probe("SELECT count(*) FROM pg_extension WHERE extname='postgis'"), "0")
            with self.assertRaises(HarnessError):
                db.query("SELECT 1")

    def test_reset_failure_blocks_stale_fixture_and_preserves_control(self):
        with DisposablePostgis() as db:
            db.setup()
            seed_marker(db)
            create_control(db)
            with self.assertRaises(HarnessError) as failed:
                db.reset(fault=True)
            self.assertIn("division by zero", str(failed.exception))
            self.assertFalse(db.ready)
            with self.assertRaises(HarnessError):
                db.query("SELECT marker FROM harness_marker")
            # Diagnostic inspection proves stale data exists but cannot be used
            # through the ready-only interface after a failed reset.
            self.assertEqual(db.probe("SELECT marker FROM harness_marker"), "first")
            self.assert_control_preserved(db)
            db.reset()
            self.assertTrue(db.ready)
            self.assertEqual(db.query("SELECT to_regclass('public.harness_marker') IS NULL"), "t")
            self.assert_control_preserved(db)

    def test_cleanup_failure_is_reported_then_explicit_cleanup_succeeds(self):
        with DisposablePostgis() as db:
            db.setup()
            owned_id = db.container_id
            with self.assertRaises(HarnessError) as failed:
                db.close(fault=True)
            self.assertTrue(str(failed.exception).strip())
            self.assertEqual(db.container_id, owned_id)
            self.assertEqual(docker("inspect", "--format", "{{.Id}}", owned_id), owned_id)
            db.close()
            self.assertEqual(docker("ps", "-aq", "--no-trunc", "--filter", f"id={owned_id}"), "")

    def test_unavailable_storage_is_a_failure_not_a_skip(self):
        with DisposablePostgis() as db:
            db.setup()
            self.assertEqual(db.query("SELECT 42"), "42")
            docker("stop", "--time", "5", db.container_id)
            with self.assertRaises(HarnessError) as failed:
                db.query("SELECT 42")
            self.assertTrue(str(failed.exception).strip())

    def test_wrong_owner_cannot_reset_or_remove_existing_databases(self):
        with DisposablePostgis() as db:
            db.setup()
            seed_marker(db)
            create_control(db)
            owned_id, owned_nonce = db.container_id, db.nonce
            try:
                # Correctly shaped but wrong ownership, not an invalid-input shortcut.
                db.nonce = "0" * 32 if owned_nonce != "0" * 32 else "1" * 32
                with self.assertRaises(HarnessError):
                    db.reset()
                self.assertFalse(db.ready)
                with self.assertRaises(HarnessError):
                    db.close()
                self.assertEqual(db.container_id, owned_id)
                self.assertEqual(docker("inspect", "--format", "{{.Id}}", owned_id), owned_id)
            finally:
                db.nonce = owned_nonce
            self.assertEqual(db.probe("SELECT marker FROM harness_marker"), "first")
            self.assert_control_preserved(db)


if __name__ == "__main__":
    print(f"Synthetic fixture: value=12.5; point=(4,5), EPSG:4326; time={SEED_TIME}", flush=True)
    print(f"Pinned database inputs: {json.dumps(PIN, sort_keys=True)}", flush=True)
    suite = unittest.defaultTestLoader.loadTestsFromTestCase(DatabaseLifecycleTests)
    expected_count = 7
    if suite.countTestCases() != expected_count:
        sys.exit(f"Required database suite discovery mismatch: expected {expected_count}, got {suite.countTestCases()}")
    result = unittest.TextTestRunner(verbosity=2).run(suite)
    if not result.wasSuccessful() or result.skipped or result.testsRun != expected_count:
        sys.exit("Database lifecycle did not execute and pass every required test; see outcomes above.")
    print(f"Database lifecycle: {result.testsRun} passed; 0 failed; 0 skipped", flush=True)
