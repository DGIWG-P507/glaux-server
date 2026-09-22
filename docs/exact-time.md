# Exact time primitives and storage proof

[Issue #11](https://github.com/DGIWG-P507/glaux-server/issues/11) implements
Guide §§4.7, 6.3 and 9.2's instant foundation in `glaux_domain::temporal`.
It does not implement interval filtering, observation routes, family tables,
SQLx connectivity, durations, clock synchronization or full CSAPI conformance.

## Meaning and representation

`ExactInstant` parses one complete RFC 3339 timestamp, validates its Gregorian
calendar/clock/offset fields and retains its exact source string and fractional
digit count. No floating-point, microsecond or nanosecond conversion occurs.
Equality, ordering and hashing use this key:

| Component | Meaning |
| --- | --- |
| Civil second, `i64` | Normalized UTC calendar second relative to 1970-01-01. For a positive leap, use its preceding ordinary second. |
| Leap slot, `bool` | False for an ordinary second, true for a validated inserted second. Compare this before the fraction. |
| Fraction, `ExactNumber` | Exact finite decimal in [0, 1); every accepted source digit participates in comparisons. |

Thus `23:59:59.999999999999` precedes `23:59:60`, which precedes the next
`00:00:00`. Never discard the leap slot or fraction. The civil-second field
alone is not a complete Unix timestamp, and differences between these keys are
not elapsed SI seconds or TAI. Pre-1972 dates use proleptic Gregorian civil
ordering, not a reconstruction of historical physical timekeeping.

Equivalent offset/fraction spellings compare equal but remain different source
strings. Lowercase t/z, all RFC numeric minute offsets and years 0000–9999 are
accepted; normalization may cross the lexical year boundary. Missing offsets,
24-hour notation, whitespace, date-only values, intervals, named zones and
IXDTF bracket suffixes are not accepted by this instant parser.

`offset_seconds()` is the adjustment used for normalization, not an inferred
named timezone. `offset_known()` means an explicit known numeric local offset
was supplied: it is false for Z/z and -00:00, true for +00:00 and other numeric
offsets. The UTC instant is known in every accepted case. Retained spelling
distinguishes those forms; RFC 9557 §2 updates RFC 3339's original Z metadata
interpretation. This does not add IXDTF syntax or change instant arithmetic.

## Precision, leap basis and limits

The local token budget is 4,096 ASCII bytes. Within it, fractions have no fixed
6/9-digit precision ceiling; exact-number parsing retains every supplied digit.
Oversized input fails explicitly before large-number allocation. This budget
is not a standards precision maximum. Endpoint owners must declare resource
limits and assess them in their own conformance tests; this proof does not
authorize rounding or rejecting otherwise required precision to fit a timestamp.

The 27 positive leap dates through 2016-12-31 are sourced date constants, not
copied implementation code. The checked basis is:

- [IERS Leap_Second.dat](https://hpiers.obspm.fr/eoppc/bul/bulc/Leap_Second.dat),
  through Bulletin C72, 1,352 bytes, SHA-256
  `6cb6f5d4b819f2e568e25db4b0b26d89dedf031fdffb18bc94d40f4e94e268d7`;
  source expiry 2027-06-28.
- [IERS Bulletin C72](https://hpiers.obspm.fr/eoppc/bul/bulc/bulletinc.72),
  2026-07-06, 1,528 bytes, SHA-256
  `310e172eadacacca3adf92cb9bd646fcc0d6320e35e996a106d2644ede3b52f0`;
  no insertion at the end of December 2026.

The source lists effective next-day offsets; constants identify the preceding
UTC date containing the insertion. Validate an offset-shifted :60 against that
UTC instant, not its local date. Unknown/unannounced :60 returns a distinct
error, never the following minute. This basis has no negative event; a future
negative announcement requires a reviewed update that rejects the deleted :59,
not merely another positive date. Ordinary future civil dates remain parseable
but do not certify that no future leap announcement will affect validation.
Review IERS announcements before later releases claiming current coverage.
No runtime upstream fetch or wall-clock expiry invalidates historical records.

## Real database boundary

The executable proof uses the existing isolated PostgreSQL 18.6 / PostGIS 3.6.4
harness. Store `bigint` civil second, `boolean` leap slot, **unconstrained
`numeric`** fraction and `text` source. Use all three key fields in SQL
ordering/predicates. A declared numeric scale or timestamp cast is not an exact
substitute. Validate finite [0, 1) fractions and restore through
`from_storage_parts`, which reparses the source and rejects a mismatched key.
The exact source retains offset, case and trailing fractional zeroes.

The test bridge exercises actual Rust parsing, PostgreSQL storage/comparison,
and Rust reconstruction against independently authored expected keys and
metadata. It is not an application adapter or a migration for empty family
tables. Later SQLx/family owners must preserve this contract and repeat the
driver-specific proof; passing psql transport does not prove a future driver.
The [database isolation rules](database-tests.md) remain unchanged.

## Verification and sources

Six unit and four deterministic property tests cover calendar/century rules,
offset normalization, exact fractions, lexical metadata, leap ordering and
storage mismatch rejection. A bounded 2,048-case parser campaign includes
semantic expectations, malformed UTF-8, invalid forms and resource boundaries.
These are reproducible bounded checks, not exhaustive or coverage-guided fuzzing.
From passing baselines, three disposable faults must compile and fail their
specific assertions: reversed offset sign, discarded fraction and lost leap slot.
Existing suites remain required; actual run outcomes belong to the issue/PR.

- [RFC 3339 §§4–5 and Appendix C](https://www.rfc-editor.org/rfc/rfc3339.html):
  calendar grammar, numeric offsets, decimal fractions and leap placement.
- [RFC 9557 §2](https://www.rfc-editor.org/rfc/rfc9557.html#section-2):
  Z metadata clarification, not adoption of its extended suffix grammar.
- [PostgreSQL 18 datetime types](https://www.postgresql.org/docs/18/datatype-datetime.html)
  and [datetime functions](https://www.postgresql.org/docs/18/functions-datetime.html):
  microsecond timestamp resolution and lack of leap-second handling.
- [PostgreSQL 18 numeric types](https://www.postgresql.org/docs/18/datatype-numeric.html):
  unconstrained decimals do not force a scale; their 16,383 fractional-digit
  capacity exceeds this parser's budget.

No new Cargo package or calendar/time runtime dependency is added. Small bounded
integer calendar normalization reuses task #10's reviewed exact-number primitive.
All Rust/Python/database execution is GitHub-hosted; no laptop installation.
