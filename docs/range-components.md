# SWE ranges and declared nil values

[Task #30](https://github.com/DGIWG-P507/glaux-server/issues/30) adds
CategoryRange, CountRange, QuantityRange and TimeRange model contracts, and the
shared scalar nil meanings they require. It follows Guide §§4.3/13; full payload
codecs, enclosing-record optional/null handling, storage and HTTP are later work.

## Contract and states

`glaux_standards::range::RangeContract::compile` takes the original bytes, a
reusable `StructuralValidator`, and `RangeOptions`. Domain ranges are aggregates
of two endpoints, not scalar subclasses. Endpoint metadata and constraints reuse
the scalar model, including exact numbers, units and time references. The
immutable contract retains the exact original source, including extensions.

An omitted inline `value` remains `None`; this describes a component without an
inline value, not a rule authorizing omission in every data stream. A supplied
pair must have exactly two correctly typed endpoints. `null`, an empty pair and
an omitted value are not nil declarations. Normal endpoints satisfy their
constraints, with inclusive numeric/time bounds. Comparable reversed bounds
fail; no rounding, unit conversion or numeric-to-UTC conversion is introduced.

Both scalar and range checks return an explicit `nil_reason`: `None` for an
ordinary value, or the supplied reason URI for a declared sentinel. Nil
recognition occurs before ordinary constraints. The original typed sentinel and
declaration are retained; no result is manufactured for absence. Reason URI
syntax is checked, not fetched or certified to resolve. Boolean has no nil
sentinel. Duplicate typed sentinels are rejected as an explicit local ambiguity
policy, including numerically equal aliases and duplicate NaN; no reason wins by
list order. At most 128 nil declarations are accepted per component; this and
the existing input limits are implementation budgets, not OGC limits.

`ScalarContract::component().value()` is the retained raw typed inline value;
use `inline_value()` or `check_value()` for its checked ordinary/nil meaning.
Public domain structs alone are not a validation capability. NaN equality in
the numeric primitive remains unordered; reserved-sentinel matching handles
NaN explicitly without changing numeric equality.

The range's `OrderCheck` states what was established. Local Category ordering
evidence must identify the exact declared code-space URI (or the unreferenced
local vocabulary), contain unique ascending tokens, and cover ordinary endpoints
and allowed enum members. Its order is not inferred from alphabetic order or an
AllowedTokens list. Without this evidence the order remains
`UnresolvedCategory`; a local check is not verification of a remote vocabulary.
Unknown calendar-frame order remains `UnresolvedTimeFrame` where no comparison
is established. These statuses must not be promoted to an unqualified semantic
or conformance pass by later callers.

## Published seams and bounded interpretations

The original corpus stays at CSAPI/SWE commit
[`8e03b236a049849f2ccc24b4fd9fdce5ff69bed2`](https://github.com/opengeospatial/ogcapi-connected-systems/tree/8e03b236a049849f2ccc24b4fd9fdce5ff69bed2/swecommon).
The [simple-component UML rules](https://github.com/opengeospatial/ogcapi-connected-systems/blob/8e03b236a049849f2ccc24b4fd9fdce5ff69bed2/swecommon/standard/sections/clause_7.2_uml_simple_components.adoc)
define extents, scalar endpoint requirements, ordered categories and typed nil
sentinels. Accepted IDR-022 supplies supporting nil-before-constraint analysis.

- **CountRange nil schema.** The pinned
  [CountRange schema](https://github.com/opengeospatial/ogcapi-connected-systems/blob/8e03b236a049849f2ccc24b4fd9fdce5ff69bed2/swecommon/schemas/json/CountRange.json)
  points `nilValues` at `NilValuesText`, despite integer endpoints and the
  datatype-coherence rule. Default compilation retains original-schema behavior.
  A caller must explicitly select `CountNilSchema::IntegerNilCorrection` to
  validate integer nil declarations with a separate local catalog changing only
  that reference to `NilValuesInteger`. The reference's old value is asserted;
  changed upstream assumptions fail. `source_validation()` then reports
  `CountIntegerNilCorrection`, not `Original`. Original `StructuralValidator`
  validation still rejects that same input. No string-to-integer coercion, source
  byte change, general schema relaxation or upstream correction is claimed.
- **Special range endpoints.** The published
  [JSON range mapping](https://github.com/opengeospatial/ogcapi-connected-systems/blob/8e03b236a049849f2ccc24b4fd9fdce5ff69bed2/swecommon/standard/sections/clause_10.5_json_encoding_rules.adoc)
  illustrates NaN pairs and calendar/infinity TimeRange bounds. Range checks
  permit those representations without broadening ordinary scalar ISO Time
  values. NaN is `UnorderedSpecial`, not a nil reason, unless explicitly
  declared. Infinity participates in bound comparison; ordinary constraints
  still apply. A pair with one or two declared nil endpoints is `NilEndpoint`;
  the other endpoint keeps its own meaning. This is the documented per-endpoint
  interpretation, not an invented all-or-none nil rule.
- **Calendar Time nil replacement.** The nil schema permits named specials,
  while the ordinary ISO Time rule excludes them. Glaux recognizes explicitly
  declared NaN/infinity nil sentinels before ordinary calendar validation,
  preserving that qualification. Finite numeric sentinels under Gregorian units
  remain unsupported rather than acquiring an invented calendar meaning.

These are bounded local interpretations, not amendments to OGC or evidence of a
completed SWE conformance class. Later claim owners must retain these limits.

## Verification

`range/tests.rs` and `scalar/nil_tests.rs` contain synthetic, independently
specified expectations for all four families, nil declarations/reasons,
ordinary/absent/nil distinction, exact bounds, constraints, original versus
adapted schema results, and ordering evidence. Recompilation of retained source
is a model-level round trip, not proof of a payload encoder. Direct endpoint and
reason assertions prevent a shared serialization mistake from proving itself.

The existing `rust-suites` lane discovers and executes every named test. The
compiled extra-endpoint fault changes the pair guard to accept a third item;
the required assertion must pass without the fault and fail with the exact
`None` versus `Some(Cardinality)` mismatch in a disposable source copy. Build
errors, setup failures and timeouts do not count as detection. No new CI lane,
tool installation or dependency is needed. Actual hosted results and separate
review are recorded in the PR and issue, not inferred from this document.
