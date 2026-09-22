# Exact numeric primitives

[Issue #10](https://github.com/DGIWG-P507/glaux-server/issues/10) implements Guide
§§4.3, 4.7 and 9.2's numeric parsing/comparison foundation in
`glaux_domain::numeric`. It does not implement complete SWE components, codecs,
database values, units, time or enhanced filters.

## Contracts

| Boundary | Meaning |
| --- | --- |
| `ExactNumber::parse_json_number` / `FromStr` | One finite RFC 8259 number token, including decimal fractions/exponents. No surrounding whitespace, leading plus, leading-zero integer, quoted number, fraction slash or special raw number. No implicit repair or floating-point intermediate. |
| `ExactNumber` | Reduced arbitrary-precision rational value plus original decimal spelling or finite binary64 bits. Equality, ordering and hashing use exact numeric value; lexical precision and signed zero remain separately available. |
| `CountValue` | An exactly integral value, with original spelling retained. Signed values are allowed. No universal i64/u64 restriction; explicitly requested conversions to those types fail on overflow or incompatible sign. Fractional thresholds remain ordinary numbers, not rounded Counts. |
| `ExactNumber::from_binary64` | Exact value of the supplied finite IEEE binary64 bits, not their shortest decimal display. Non-finite input returns an error. It cannot recover precision already lost before this call. |
| `NumericValue` | Finite value, NaN, positive infinity or negative infinity. NaN is unordered, including with itself. Infinities have their usual extended ordering. This is not CQL2 NULL/filter semantics. |
| `NumericValue::from_swe_special` | An already-decoded SWE NumberOrSpecial string: exactly `NaN`, `Infinity`, `+Infinity` or `-Infinity`. Other strings, including XML Schema `INF`, are not aliases at this boundary. |

For example, decimal `0.1` is exactly 1/10. Binary64 `0.1` is exactly
3602879701896397/36028797018963968; these values must not compare equal.
`1`, `1.00` and `1e0` do compare equal while retaining their different source
spelling. Adjacent integers beyond 2^53 remain distinct. Values such as `1e400`
or `1e-400` do not overflow to infinity or underflow to zero.

The explicit special-state conversion retains semantic category, not NaN payload
or sign bits. Finite binary64 inputs retain all original bits, including negative
zero. Original wire-byte preservation belongs to the surrounding artifact/codec
contract; this numeric primitive is not a full wire round-trip guarantee.

## Bounds and exclusions

Finite JSON tokens are limited to 4,096 bytes and an explicit decimal exponent
whose absolute value is at most 4,096. Syntax/exponent checks precede big-integer
creation and powers of ten; fractional digit count is bounded by token length.
No arbitrary rational constructor or arithmetic API can bypass these input bounds.
The zero coefficient avoids needless power allocation but does not bypass limits.
Errors contain a safe category, not the supplied token.

These are local resource budgets, not SWE value-space restrictions or proof of
all standard inputs. Larger otherwise-valid inputs fail explicitly, never round.
Limits must be considered by later endpoint/codec conformance owners; this task
does not silently establish a restricted SWE conformance claim. No universal
request CPU deadline or production performance target is established.

Nil depends on a component's declared sentinel/reason mapping. Missing values,
nil, NaN, infinity and zero are not interchangeable. Full component constraints,
unit compatibility, declared nil handling and operation-specific validation remain
later work. SWE Text's XML Schema lexical conventions are not implemented by this
deliberately named JSON-token parser. No serializer or database adapter is added.

## Sources and selection

- [RFC 8259 §6](https://www.rfc-editor.org/rfc/rfc8259.html#section-6) specifies
  JSON number-token grammar and distinguishes syntax from numeric implementation
  precision/range. These tests deliberately exceed ordinary binary64 precision.
- [SWE Common 3.0](https://docs.ogc.org/is/24-014/24-014.html), §§8.2.7–8.2.8,
  10.2–10.3, and the unchanged packaged
  [Count](../crates/glaux-standards/corpus/originals/csapi/swecommon/schemas/json/Count.json),
  [Quantity](../crates/glaux-standards/corpus/originals/csapi/swecommon/schemas/json/Quantity.json)
  and [NumberOrSpecial](../crates/glaux-standards/corpus/originals/csapi/swecommon/schemas/json/basicTypes.json)
  definitions supply the integer/quantity/special distinctions. Count has no
  universal nonnegative minimum; component constraints still apply.
- Exact pins reuse already locked MIT/Apache alternatives:
  [num-bigint 0.4.8](https://github.com/rust-num/num-bigint/tree/num-bigint-0.4.8),
  [num-rational 0.4.2](https://github.com/rust-num/num-rational/tree/num-rational-0.4.2)
  and [num-traits 0.2.19](https://github.com/rust-num/num-traits/tree/num-traits-0.2.19).
  Direct defaults are disabled; only rational's `num-bigint` feature is requested.
  Actual unified features, checksums and notices remain in the
  [reviewed Cargo snapshot](cargo-dependencies.json).

The parser converts validated decimal coefficient/scale into a reduced
`BigRational`; it does not use that crate's fraction-string parser as JSON syntax
or its floating conversion as a comparison shortcut.

## Executable evidence

Eight named unit tests and four deterministic property tests are mandatory for
both discovery and actual execution. Expectations include independent decimal
ratios, adjacent large integers, fraction/exponent/zero cases, signed/unsigned
conversion edges, all four special strings, exact binary64/subnormal cases and
explicit malformed/budget errors. Source spelling and equality/hash consistency
are checked separately.

The bounded numeric-parser campaign runs 2,048 cases with a fixed, tested generator,
covering valid, invalid, malformed UTF-8 and budget-boundary partitions. It checks
semantic relationships and exact integer expectations, not merely absence of a
panic. Invalid UTF-8 is rejected before entering the public `&str` parser.
Generator version/seed, reached partitions and distinct cases are printed; code
and literal regressions remain in source. This is deterministic mutation/property
coverage, not coverage-guided fuzzing or an exhaustive proof.

Two disposable faults must compile and fail their exact assertions after all
selected baselines pass: comparison through binary64 collapses adjacent large
integers; shortest-decimal conversion destroys binary64 0.1's exact ratio.
Setup errors, filtered tests and unrelated failures do not count as detection.
All runtime checks execute on GitHub-hosted Linux, with existing identity, schema,
database and false-green checks retained. Issue/PR records carry actual run results.
