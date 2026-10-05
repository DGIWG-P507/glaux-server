# SWE DataRecord and Vector components

[Task #31](https://github.com/DGIWG-P507/glaux-server/issues/31) adds bounded,
immutable component descriptions under Guide §4.3. It does not add HTTP routes,
storage, record payload encoders, coordinate transformations or a reference-graph
resolver. [Task #32](choice-components.md) adds choices and model-level selected
value checking. [Task #33](array-components.md) adds array/matrix descriptions,
not their payload values. [Task #34](geometry-components.md) adds distinct
Geometry children; quality remains its owning task's work.

## What the contract preserves and checks

`glaux_standards::aggregate::AggregateContract::compile` accepts source bytes and
the reusable `StructuralValidator`. Its domain `AggregateComponent` distinguishes
DataRecord fields from Vector coordinates. Both use ordered named children, not
a map whose sorting could change the data layout. Child identity is scoped to
its parent; the same name may occur in different nested records.

Records can contain the implemented scalar and range families, records, vectors
and choices, arrays, matrices and Geometry descriptions. Child compilation reuses
the corresponding typed validation, including exact
numbers, units, constraints, time context and declared nil reasons. Root and
child `source()` retain the original bytes, including numeric spelling and
extensions. `children()` exposes immutable child contracts so callers can inspect
checked inline scalar/range meaning instead of inferring it from raw domain
values. Public domain structs are data, not proof of validation.

DataRecord's `definition` and `label` are optional in its JSON contract; a
Vector requires both plus its reference frame. Supplied descriptions, IDs,
optionality and update flags remain metadata. A descriptor without an inline
value is not a missing required payload value. Payload omission/null handling
belongs to later encoders; this compiler does not manufacture values.

Vectors contain one or more Count, Quantity or Time coordinates. Names are
unique within the vector, each coordinate names its axis, and coordinates
cannot redeclare `referenceFrame` or set `optional: true`. Omitted or false
coordinate optionality is permitted; the whole Vector can be optional.
`localFrame`, when supplied, must differ from `referenceFrame`. Frame/axis
strings are retained without fetching a registry or claiming to validate the
real-world frame's axis inventory. No extra axis-identifier uniqueness rule is
inferred from the rule about unique coordinate names.

The enclosing vector supplies every coordinate's frame. In particular, a Time
coordinate must not acquire the standalone Time UTC default when its vector
names another frame. Its checked `TimeReference` records the inherited frame,
while its original metadata/source still show no coordinate-level declaration.
Unknown frame meaning remains unresolved; this is not a conversion engine.

## Sources and visible limits

Sources remain pinned to SWE/CSAPI commit
[`8e03b236a049849f2ccc24b4fd9fdce5ff69bed2`](https://github.com/opengeospatial/ogcapi-connected-systems/tree/8e03b236a049849f2ccc24b4fd9fdce5ff69bed2/swecommon).
The [record conceptual rules](https://github.com/opengeospatial/ogcapi-connected-systems/blob/8e03b236a049849f2ccc24b4fd9fdce5ff69bed2/swecommon/standard/sections/clause_7.3_uml_record_components.adoc)
and [record JSON mapping](https://github.com/opengeospatial/ogcapi-connected-systems/blob/8e03b236a049849f2ccc24b4fd9fdce5ff69bed2/swecommon/standard/sections/clause_9.2_json_record_components.adoc)
correspond to published SWE 3.0 §§8.3/9.2. Required name/frame/axis checks follow
requirements 37–41, independently of whether the original JSON schema catches
the error. The [Vector UML figure](https://github.com/opengeospatial/ogcapi-connected-systems/blob/8e03b236a049849f2ccc24b4fd9fdce5ff69bed2/swecommon/standard/figures/fig7.26.png)
supplies its `1..*` coordinate cardinality. Published §§10.3.3/10.4.3 disallow
optional coordinates; requirement 78 preserves component order for later
encoding. No encoding conformance is claimed here.

The original Vector schema lacks several conceptual checks, including the
minimum count and mandatory axes. Some published illustrative vectors omit
axes despite requirement 40. Tests keep schema acceptance separate from typed
rejection, rather than changing the schema or deriving an axis from position.

The fixed `DataRecord` and `Vector` structural entries assert formats so nested
Time/TimeRange strings distinguish calendar values from special numeric tokens.
The earlier `SweRecord` source-diagnostic entry keeps its existing settings.
Neither alters upstream bytes; [structural validation](structural-validation.md)
describes the distinction. Nested ranges currently use original-schema defaults:
no automatic CountRange nil correction and no invented Category order. Explicit
range options remain available through the standalone range API; nesting does
not silently select them.

The whole source passes the existing byte/depth/node/member/string budgets
before recursive compilation. Linked-only children and not-yet-implemented
component families fail explicitly. Metadata URIs and extension members are
not retrieval authority. A retained extension is not a validated component.

## Verification

Independent synthetic fixtures exercise nonalphabetical order, mixed/nested
records, all allowed numeric coordinate kinds, axis/frame bindings, invalid
names/members/optionality, nested scalar/range failures and bounded input.
Expected names, kinds, values and metadata are written from the source rules,
not produced by the compiler under test. Original-byte assertions complement
these semantic checks; recompilation is not a payload codec round trip.

Every named test is required for discovery and execution in the existing
`rust-suites` lane. A controlled fault sorts record children alphabetically in
a disposable source copy. The unchanged test must first pass, then fail with
the exact wrong-versus-expected ordered names. A build/setup failure or timeout
does not count as detection. Hosted results and separate review belong to this
issue's PR/execution record; this document is not an execution claim.
