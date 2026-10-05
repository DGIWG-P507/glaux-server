# Boolean, Text and Category components

Task 2.1.1 ([issue #27](https://github.com/DGIWG-P507/glaux-server/issues/27))
adds typed descriptions and local value checks, not HTTP operations, database
persistence or payload codecs. It implements the scalar portion of
[Guide §4.3](https://github.com/DGIWG-P507/glaux/blob/main/Docs/Plans/glaux-server/glaux-server-implementation-guide.md#43-sensorml-swe-common-validation-and-semantic-bindings).

## What it does

Task 2.1.2 adds [Count and Quantity](numeric-components.md) through the same
compiler, with exact values, numeric constraints and offline unit checks. The
Boolean/Text/Category rules below remain unchanged.

`glaux_domain::scalar` holds the three distinct component/value types and their
metadata. `glaux_standards::scalar::ScalarContract::compile` accepts a reusable
`StructuralValidator` and source JSON bytes. It first uses the bounded parser and
the original Boolean/Text/Category schemas, then checks local scalar semantics.
The resulting contract is immutable and keeps the exact received bytes, including
permitted extensions. Public domain structs alone are not proof of validation.

- Boolean values are actual JSON booleans, not numbers or strings.
- Text and Category values are strings. Case, whitespace and Unicode are retained.
- Missing `value`, `false`, and `""` remain different typed states. Supplied `null`
  is not converted into absence or nil.
- Required definition and label, optional identifier/description, optional and
  updatable flags, reference frame and axis are retained without synthesizing
  defaults. Original schemas supply presence/type/nonempty rules. URI and
  URI-reference syntax is checked explicitly, without fetching a resource.
- Text/Category `AllowedTokens` enumerations use exact membership. Duplicate
  allowed tokens are retained because the source does not prohibit them. Pattern
  checks use the existing pinned JSON Schema engine with its resource limits;
  they search unless the supplied expression anchors itself.
- Category must carry a code-space URI, a nonempty enumerated list, or both.
  A pattern alone does not replace that list when code space is absent.

`check_value` checks one supplied JSON value and returns its typed meaning plus a
`CodeSpaceCheck`. A supplied code space always yields `Unresolved(uri)`, even when
local enumeration/pattern checks succeed: neither external membership nor the
enumeration's subset relationship was established. The same local checks apply
to a description's inline value. Callers must not relabel compilation or local
success as proof of external vocabulary validity.

## Sources and explicit limits

The fixed corpus is pinned at
[`8e03b236a049849f2ccc24b4fd9fdce5ff69bed2`](https://github.com/opengeospatial/ogcapi-connected-systems/tree/8e03b236a049849f2ccc24b4fd9fdce5ff69bed2/swecommon).
The concrete and inherited JSON schemas, `basicTypes.json`'s `AllowedTokens`,
and SWE Category requirements 23–25 control these rules. Requirement 24 requires
a code space or enumeration; 23 and 25 explain the unresolved external checks.
The accepted IDR-022 scalar/constraint analysis is supporting research, not a
replacement for these sources. The vendored originals are unchanged.

Task 2.1.4 adds [declared nil semantics](range-components.md) and the checked
`nil_reason` distinction for supported scalars. Quality remains owned by a later
task and returns `UnsupportedFeature`; it is not silently discarded. Boolean constraints also
return that result: this task invents no Boolean constraint form and does not
claim that the inherited open schema universally prohibits extensions.

Regex behavior is bounded, not a full ECMAScript implementation. The pinned
engine's ECMA-to-Rust translation is best-effort and bypasses translation for
lookarounds and backreferences. This compiler explicitly refuses those constructs,
inline flags, word boundaries and alphanumeric escapes other than
`d D w W f n r t v`; ordinary escaped punctuation and noncapturing groups
remain available. Unsupported expressions fail explicitly rather than being
accepted under unverified advanced semantics. Syntax/compile errors also fail.
Wildcard dot and `\s`/`\S` are unsupported because the engine differs on ECMA
line terminators/whitespace; escaped dot and `[.]` work. Nested character classes
and class set operators `&&`, `--`, `~~` are refused so Rust-specific meanings
cannot be substituted for ECMA ones. This deliberately limited pattern support
does not declare valid but unsupported SWE expressions invalid under the standard.
No input controls a schema URL, retriever or external resolver. Parser and
regex resource bounds are the existing [validation bounds](structural-validation.md#resource-budgets-and-diagnostics),
not a throughput guarantee or a proof of complete SWE/CSAPI conformance.

## Verification

Eight required tests in `crates/glaux-standards/src/scalar/tests.rs` compare exact
typed states and retained bytes with independently authored expectations. They
cover both valid and invalid values, metadata, constraints, vocabulary uncertainty,
duplicate keys and input limits. Expected values do not come from a server
encoder or round trip.

The existing `rust-suites` lane runs them and checks named discovery/execution.
`scripts/test-validation-failures.py` adds three controlled-fault alternatives
to behavioral test-first evidence: false collapsed to absent, empty text collapsed
to absent, and bypassed Category membership. Every selected assertion must pass
in an unmodified copy first, then fail at its exact expected assertion in a
compiled disposable copy. Build/setup failure is not detection. Four existing
validation faults remain. Later numeric, Time and range controls extend the
same runner; [CI documentation](ci.md) records the current inventory.
Run the full required CI;
the issue and PR record actual hosted outcomes, not this command list.

```sh
cargo test --locked --offline -p glaux-standards --lib scalar::tests -- --nocapture
python3 -u scripts/test-validation-failures.py
```
