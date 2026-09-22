"""Exact-time Rust -> real PostgreSQL -> Rust proof for task 1.2.5.

Uses the existing owned disposable cluster and its psql, not SQLx, a Rust
database driver, resource-family tables or an application migration. Expected
keys, equality groups, query results and source metadata are authored here
before Rust parses or PostgreSQL stores anything. No binary float oracle.
"""

from dataclasses import dataclass
from decimal import Decimal
from pathlib import Path
import subprocess
import sys
import unittest

from database_harness import DisposablePostgis, HarnessError


ROOT = Path(__file__).resolve().parents[1]
PROBE = ROOT / "target/debug/examples/time-storage-probe"
HUNDRED_A = "000000001" + "0" * 90 + "1"
HUNDRED_B = "000000001" + "0" * 90 + "2"
MAX_FRACTION = "0" * 4074 + "1"  # 4096 timestamp bytes, not a DB scale.


@dataclass(frozen=True)
class Fixture:
    name: str
    source: str
    second: int
    leap: bool
    fraction: str
    digits: int
    offset: int = 0
    # Explicit known numeric local offset, not knowledge of the UTC instant.
    # Z and -00:00 are false under the RFC 9557-updated source-context contract.
    known: bool = False


# Epoch seconds are independently specified civil-UTC coordinates, not counts
# of elapsed SI seconds: the leap slot follows its preceding civil second.
FIXTURES = (
    Fixture("pre_epoch", "1969-12-31T23:59:59.999999999Z", -1, False,
            "0.999999999", 9),
    Fixture("pre_epoch_offset", "1969-12-31T18:59:59.999999999-05:00", -1,
            False, "0.999999999", 9, -18000, True),
    Fixture("epoch", "1970-01-01T00:00:00Z", 0, False, "0", 0),
    Fixture("epoch_plus", "1970-01-01T01:00:00+01:00", 0, False, "0", 0, 3600, True),
    Fixture("epoch_unknown", "1970-01-01T00:00:00-00:00", 0, False, "0", 0,
            known=False),
    Fixture("below_nanos", "1970-01-01T00:00:00." + MAX_FRACTION + "Z", 0,
            False, "0." + MAX_FRACTION, 4075),
    Fixture("nanos", "1970-01-01T00:00:00.000000001Z", 0, False,
            "0.000000001", 9),
    Fixture("nanos_10_digits", "1970-01-01T00:00:00.0000000010Z", 0, False,
            "0.000000001", 10),
    Fixture("nanos_hundred_a", "1970-01-01T00:00:00." + HUNDRED_A + "Z", 0,
            False, "0." + HUNDRED_A, 100),
    Fixture("nanos_hundred_b", "1970-01-01T00:00:00." + HUNDRED_B + "Z", 0,
            False, "0." + HUNDRED_B, 100),
    Fixture("two_nanos", "1970-01-01T00:00:00.000000002Z", 0, False,
            "0.000000002", 9),
    Fixture("micro_a", "1970-01-01T00:00:00.0000011Z", 0, False,
            "0.0000011", 7),
    Fixture("micro_b", "1970-01-01T00:00:00.0000012Z", 0, False,
            "0.0000012", 7),
    Fixture("pre_leap", "2016-12-31T23:59:59.999999999999999999Z", 1483228799,
            False, "0.999999999999999999", 18),
    Fixture("leap", "2016-12-31T23:59:60Z", 1483228799, True, "0", 0),
    Fixture("leap_fraction", "2016-12-31T23:59:60.0000000001Z", 1483228799,
            True, "0.0000000001", 10),
    Fixture("leap_offset", "2017-01-01T00:59:60.0000000001+01:00", 1483228799,
            True, "0.0000000001", 10, 3600, True),
    Fixture("post_leap", "2017-01-01T00:00:00Z", 1483228800, False, "0", 0),
)
BY_NAME = {fixture.name: fixture for fixture in FIXTURES}
EXPECTED_ORDER = [
    "pre_epoch", "pre_epoch_offset", "epoch", "epoch_plus", "epoch_unknown",
    "below_nanos", "nanos", "nanos_10_digits", "nanos_hundred_a",
    "nanos_hundred_b", "two_nanos", "micro_a", "micro_b", "pre_leap", "leap",
    "leap_fraction", "leap_offset", "post_leap",
]
REQUIRED_TESTS = {
    "test_rust_database_roundtrip_preserves_exact_keys_and_source_metadata",
    "test_database_equality_order_and_exact_boundaries",
    "test_database_fraction_constraints_reject_invalid_values",
    "test_reconstruction_rejects_database_key_source_mismatch",
    "test_lossy_timestamp_control_collapses_values_exact_storage_distinguishes",
}

CREATE_TABLE = """
CREATE TABLE time_exact_probe (
    fixture text PRIMARY KEY,
    civil_second bigint NOT NULL,
    leap_second boolean NOT NULL,
    fraction numeric NOT NULL CHECK (
        fraction NOT IN ('NaN'::numeric, 'Infinity'::numeric, '-Infinity'::numeric)
        AND fraction >= 0 AND fraction < 1
    ),
    source text NOT NULL
);
"""
STORAGE_FIELDS = (
    "civil_second::text || chr(9) || leap_second::text || chr(9) || "
    "fraction::text || chr(9) || source"
)
KEY = "(civil_second, leap_second, fraction)"


def run_probe(mode, text):
    """An absent/crashed/timed-out runner is an error, never rejection evidence."""
    return subprocess.run(
        [str(PROBE), mode], input=text, text=True, capture_output=True,
        timeout=15, check=False, cwd=ROOT,
    )


def quote(text):
    # Diagnostic statements only, never a user-facing SQL construction API.
    return "'" + text.replace("'", "''") + "'"


class ExactTimeDatabaseTests(unittest.TestCase):
    def checked_rows(self, result, expected):
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stderr, "")
        rows = [line.split("\t") for line in result.stdout.splitlines()]
        self.assertEqual(len(rows), len(expected))
        self.assertTrue(rows, "An empty successful runner is not evidence")
        for fields, fixture in zip(rows, expected, strict=True):
            self.assertEqual(len(fields), 7)
            second, leap, fraction, source, digits, offset, known = fields
            self.assertEqual(second, str(fixture.second), fixture.name)
            self.assertEqual(leap, str(fixture.leap).lower(), fixture.name)
            self.assertRegex(fraction, r"\A0(?:\.[0-9]+)?\Z")
            self.assertLessEqual(len(fraction), 4096)
            self.assertEqual(Decimal(fraction), Decimal(fixture.fraction), fixture.name)
            self.assertEqual(source, fixture.source, fixture.name)
            self.assertEqual(digits, str(fixture.digits), fixture.name)
            self.assertEqual(offset, str(fixture.offset), fixture.name)
            self.assertEqual(known, str(fixture.known).lower(), fixture.name)
        return rows

    def seed(self, db, fixtures=FIXTURES):
        # Check independent answers BEFORE using any Rust output in INSERT.
        rows = self.checked_rows(
            run_probe("parse", "\n".join(f.source for f in fixtures) + "\n"), fixtures,
        )
        db.setup()
        db.query(CREATE_TABLE)
        values = []
        for fixture, row in zip(fixtures, rows, strict=True):
            second, leap, fraction, source = row[:4]
            values.append(
                f"({quote(fixture.name)}, {second}, {leap}, {fraction}, {quote(source)})"
            )
        db.query("INSERT INTO time_exact_probe VALUES " + ",\n".join(values))

    def test_rust_database_roundtrip_preserves_exact_keys_and_source_metadata(self):
        with DisposablePostgis() as db:
            self.seed(db)
            self.assertEqual(
                db.query("SELECT format_type(atttypid, atttypmod) FROM pg_attribute "
                         "WHERE attrelid='time_exact_probe'::regclass AND attname='fraction'"),
                "numeric",  # Reject an unnoticed fixed-scale declaration.
            )
            names = db.query("SELECT fixture FROM time_exact_probe ORDER BY fixture").splitlines()
            expected = [BY_NAME[name] for name in names]
            self.assertEqual(set(names), set(BY_NAME))
            stored = db.query("SELECT " + STORAGE_FIELDS + " FROM time_exact_probe ORDER BY fixture")
            self.checked_rows(run_probe("reconstruct", stored + "\n"), expected)

    def test_database_equality_order_and_exact_boundaries(self):
        with DisposablePostgis() as db:
            self.seed(db)
            self.assertEqual(
                db.query("SELECT fixture FROM time_exact_probe ORDER BY "
                         "civil_second, leap_second, fraction, fixture").splitlines(),
                EXPECTED_ORDER,
            )
            # Equality is exact instant identity, not spelling, precision or offset.
            self.assertEqual(
                db.query("SELECT string_agg(fixture, ',' ORDER BY fixture) FROM time_exact_probe "
                         "GROUP BY civil_second, leap_second, fraction HAVING count(*) > 1 "
                         "ORDER BY string_agg(fixture, ',' ORDER BY fixture)").splitlines(),
                ["epoch,epoch_plus,epoch_unknown", "leap_fraction,leap_offset",
                 "nanos,nanos_10_digits", "pre_epoch,pre_epoch_offset"],
            )
            self.assertEqual(
                db.query("SELECT fixture FROM time_exact_probe WHERE " + KEY +
                         " > (0::bigint, false, 0.000000001::numeric) AND " + KEY +
                         " < (0::bigint, false, 0.000000002::numeric) ORDER BY fraction").splitlines(),
                ["nanos_hundred_a", "nanos_hundred_b"],
            )
            self.assertEqual(
                db.query("SELECT fixture FROM time_exact_probe WHERE " + KEY +
                         " > (1483228799::bigint, false, 0.999999999999999999::numeric) AND " + KEY +
                         " < (1483228800::bigint, false, 0::numeric) "
                         "ORDER BY civil_second, leap_second, fraction, fixture").splitlines(),
                ["leap", "leap_fraction", "leap_offset"],
            )
            ordered = db.query("SELECT " + STORAGE_FIELDS + " FROM time_exact_probe "
                               "ORDER BY civil_second, leap_second, fraction, fixture")
            self.checked_rows(run_probe("reconstruct", ordered + "\n"),
                              [BY_NAME[name] for name in EXPECTED_ORDER])

    def test_database_fraction_constraints_reject_invalid_values(self):
        with DisposablePostgis() as db:
            self.seed(db, (BY_NAME["epoch"],))
            for value in ("'NaN'::numeric", "'Infinity'::numeric", "'-Infinity'::numeric",
                          "-0.0000000001", "1", "1.0000000001"):
                with self.subTest(fraction=value):
                    with self.assertRaises(HarnessError) as failure:
                        db.query("INSERT INTO time_exact_probe VALUES ('invalid', 0, false, " +
                                 value + ", '1970-01-01T00:00:00Z')")
                    self.assertIn("check constraint", str(failure.exception))
                    self.assertEqual(db.query("SELECT fixture FROM time_exact_probe"), "epoch")
            with self.assertRaises(HarnessError) as failure:
                db.query("INSERT INTO time_exact_probe VALUES "
                         "('null', 0, false, NULL, '1970-01-01T00:00:00Z')")
            self.assertIn("not-null constraint", str(failure.exception))
            self.assertEqual(db.query("SELECT fixture FROM time_exact_probe"), "epoch")

    def test_reconstruction_rejects_database_key_source_mismatch(self):
        with DisposablePostgis() as db:
            fixture = BY_NAME["nanos"]
            self.seed(db, (fixture,))
            # This valid control must succeed before any deliberate corruption.
            control = db.query("SELECT " + STORAGE_FIELDS + " FROM time_exact_probe")
            self.checked_rows(run_probe("reconstruct", control + "\n"), (fixture,))
            for change in ("civil_second=1", "leap_second=true",
                           "fraction=0." + HUNDRED_A,
                           "source='1970-01-01T00:00:00Z'"):
                with self.subTest(change=change):
                    db.query("UPDATE time_exact_probe SET civil_second=0, leap_second=false, "
                             "fraction=0.000000001, source='1970-01-01T00:00:00.000000001Z'")
                    db.query("UPDATE time_exact_probe SET " + change)
                    corrupted = db.query("SELECT " + STORAGE_FIELDS + " FROM time_exact_probe")
                    result = run_probe("reconstruct", corrupted + "\n")
                    self.assertEqual(result.returncode, 1, result.stderr)
                    self.assertEqual(result.stdout, "")
                    self.assertTrue(
                        result.stderr.startswith("Time storage probe rejected input: reconstruct:"),
                        result.stderr,
                    )

    def test_lossy_timestamp_control_collapses_values_exact_storage_distinguishes(self):
        with DisposablePostgis() as db:
            self.seed(db, (BY_NAME["micro_a"], BY_NAME["micro_b"]))
            # Deliberately wrong storage model as a sensitivity control. Both
            # values round to one PostgreSQL microsecond; neither is stored in
            # a timestamp column by the actual representation under test.
            self.assertEqual(
                db.query("SELECT (TIMESTAMPTZ '1970-01-01T00:00:00.0000011Z' = "
                         "TIMESTAMPTZ '1970-01-01T00:00:00.0000012Z')::text"),
                "true",
            )
            self.assertEqual(db.query("SELECT count(DISTINCT " + KEY + ") FROM time_exact_probe"), "2")
            self.assertEqual(
                db.query("SELECT fixture FROM time_exact_probe "
                         "WHERE fraction > 0.0000011 AND fraction <= 0.0000012"),
                "micro_b",
            )


class RequiredResult(unittest.TextTestResult):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        self.successful_names = []

    def addSuccess(self, test):
        self.successful_names.append(test.id().rsplit(".", 1)[1])
        super().addSuccess(test)


def main():
    if sys.argv[1:]:
        sys.exit("No test-selection override is accepted.")
    print("Exact-time synthetic fixtures: civil bigint, leap flag, unconstrained numeric, source text", flush=True)
    command = ["cargo", "build", "--locked", "--offline", "-p", "glaux-domain",
               "--example", "time-storage-probe", "--target-dir", str(ROOT / "target")]
    try:
        build = subprocess.run(command, cwd=ROOT, capture_output=True, text=True,
                               timeout=120, check=False)
    except (OSError, subprocess.TimeoutExpired) as error:
        sys.exit(f"Required time-storage probe build unavailable/timeout: {error}")
    print(build.stdout + build.stderr, end="", flush=True)
    if build.returncode or not PROBE.is_file():
        sys.exit("Required time-storage probe did not build successfully.")
    suite = unittest.defaultTestLoader.loadTestsFromTestCase(ExactTimeDatabaseTests)
    discovered = [test.id().rsplit(".", 1)[1] for test in suite]
    if len(discovered) != len(REQUIRED_TESTS) or set(discovered) != REQUIRED_TESTS:
        sys.exit(f"Required exact-time database discovery mismatch: {discovered}")
    result = unittest.TextTestRunner(verbosity=2, resultclass=RequiredResult).run(suite)
    if (not result.wasSuccessful() or result.skipped or result.testsRun != len(REQUIRED_TESTS)
            or len(result.successful_names) != len(REQUIRED_TESTS)
            or set(result.successful_names) != REQUIRED_TESTS):
        sys.exit("Exact-time database proof did not execute and pass every required test.")
    print("Exact-time database: 5 passed; 0 failed; 0 skipped", flush=True)


if __name__ == "__main__":
    main()
