# Count and Quantity components

Task 2.1.2 ([issue #28](https://github.com/DGIWG-P507/glaux-server/issues/28))
extends the [scalar contract compiler](scalar-components.md) with Count and
Quantity. It checks descriptions and individual JSON values locally; it does
not add observation storage, payload codecs, unit conversion or new routes.

## Exact values, constraints and units

`ScalarContract::compile` applies the original pinned schema and then the local
numeric rules. `check_value` uses the same constraints for a supplied value.
The domain model retains the complete unit declaration and numeric constraints;
the compiled contract retains the exact original description bytes, including
extensions. Domain structs alone are not proof of validation.

- Count uses the existing arbitrary-precision `CountValue`. Negative integers
  are not universally forbidden. Fractional values and special non-finite
  states are rejected; an integral exponent or decimal spelling is still an
  integer. No machine-integer range is silently imposed.
- Quantity uses the existing exact decimal/rational value model. The pinned
  JSON binding additionally permits the exact strings `NaN`, `Infinity`,
  `+Infinity` and `-Infinity`. No universal finite-only rule is added. Numeric
  strings, booleans, null and alternate special spellings are not coerced.
- Absence, zero and a named non-finite state stay distinct. Inline values and
  individually checked values retain their numeric lexeme, including exponent
  notation, trailing zeros and negative zero. No floating-point intermediate
  or implicit rounding is used.
- `AllowedValues` enumerations and inclusive intervals form a union. Every
  interval is checked even when an enumeration could admit the value. Count
  constraints must also be integral, with no `significantFigures` constraint.
- Quantity `significantFigures` is a maximum on supplied significant digits,
  not a rounding instruction. Leading zeros and the exponent do not count;
  trailing zeros do. `12.2300` has six and `0.00052` has two. Excess precision
  is rejected, not silently changed.
- Quantity must supply a nonempty string `label` and a unit declaration under
  the published schema. The compiler never synthesizes a label from definition.
  This is the approved issue amendment, not a new universal label policy for
  every future component kind.
- `uom.code` is checked offline against the incorporated [UCUM 2.1 basis](ucum.md).
  `href` is checked as an absolute URI and remains explicitly unresolved. A
  code-only successful value check returns `UnitReferenceCheck::CodeValidated`;
  any supplied URI returns `Unresolved(uri)`, even alongside a valid code.
  This never proves code/URI agreement, dimension, property equivalence or
  external dictionary meaning. An invalid code is not excused by a valid URI.

The schema engine's `arbitrary-precision` feature is enabled as well as the
JSON parser's separately named `arbitrary_precision` feature. Parser precision
alone does not guarantee exact schema integer classification. The version stays
pinned; the reviewed Cargo inventory records the feature and resolved edges.

## Source basis and explicit choices

The original `Count.json`, `Quantity.json` and `basicTypes.json` files are pinned
in the [standards corpus](standards-corpus.md). The corresponding
[SWE simple-component and constraint prose](https://github.com/opengeospatial/ogcapi-connected-systems/blob/8e03b236a049849f2ccc24b4fd9fdce5ff69bed2/swecommon/standard/sections/clause_7.2_uml_simple_components.adoc)
controls the local semantics; IDR-022 and IDR-024 supply supporting research.
For significant figures, the explicit AllowedValues definition and examples
control, rather than treating the earlier Quantity summary's "decimal point"
wording as a decimal-place limit.

The sources do not completely define NaN membership or all-zero precision.
The local rules are explicit: an enumerated NaN admits the named NaN state;
NaN is never inside an ordered interval and is rejected as an interval bound.
Reversed bounds are rejected; equal ordered bounds are allowed. This does not
change the underlying numeric type's IEEE-like NaN equality/ordering. All-zero
precision counts fractional places, with a minimum of one (`0` = one, `0.00`
= two). Named non-finite states have no decimal digit count, but still must
satisfy membership when a constraint exists. These choices are not additional
claims about a standards-mandated IEEE policy.

The existing [numeric input limits](exact-numbers.md) and bounded JSON parsing
apply. Special-scale unit algebra has a separate unsupported result, documented
with all UCUM parser limits. The shared JSON parser enforces numeric token and
exponent bounds before any schema or projection evaluation, including numbers
in extensions; the generic engine's larger limits do not replace local budgets.
Nil/quality semantics remain explicitly unsupported
here and belong to later tasks. No external resource is fetched.

## Verification

Eight required numeric-component tests use independently authored exact values,
metadata and verdicts, plus a bounded generated integer/fraction/lexeme campaign.
They cover large integers beyond both binary64 precision and u64, huge finite
exponents, exact fraction rejection by the schema engine, absence/zero, required
labels, all permitted special states, union constraints, inclusive boundaries,
precision rejection and unit-reference uncertainty. They do not use a server
encoder to generate their expected answers. Five required UCUM tests and the
separate source/table/license check are described in [the unit documentation](ucum.md).

Three new disposable compiled faults in `scripts/test-validation-failures.py`
round a large Count, bypass numeric membership and change the submitted unit.
Each selected assertion must first pass in an unmodified copy and then fail at
its intended expected-value assertion. A build error or setup failure is not
fault detection. This is the controlled-fault alternative to test-first
behavioral evidence; the issue/PR records actual hosted execution results.

```sh
cargo test --locked --offline -p glaux-standards --lib scalar::numeric_tests -- --nocapture
cargo test --locked --offline -p glaux-standards --lib units::tests -- --nocapture
python3 crates/glaux-standards/src/units/verify.py --self-test
python3 -u scripts/test-validation-failures.py
```
