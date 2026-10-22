# Inline SWE quality assertions

[Task #36](https://github.com/DGIWG-P507/glaux-server/issues/36) adds the
Guide's separate semantic validation for inline quality. To describe a result's
uncertainty, classification or textual assessment, Glaux keeps the supplied
quality component attached to the result component it describes. It does not
turn different measures into a universal confidence score.

## Interpretation and checked representation

The published SWE JSON simple-component schema does not declare `quality`.
An upstream schema pass therefore cannot prove that quality is meaningful.
The original schemas remain unchanged. After their structural checks,
`ScalarContract` and `RangeContract` validate the optional quality array using
the explicit Glaux interpretation in Guide §§4.3, 4.10 and 13.

Each inline entry is a `Quantity`, `QuantityRange`, `Category` or `Text`.
It reuses the corresponding component's existing unit, constraint, nil and
supplied-value checks, including exact numeric values. Multiple entries retain
their order. Quality components may themselves carry quality; the existing
whole-document byte, depth, node, member and string limits apply before recursive
compilation. Malformed arrays, unsupported entry types and invalid inline
components fail, even when the unmodified parent schema accepts them.

The checked contract's `quality()` accessor returns `QualityAssertions`;
`is_present()` distinguishes an absent member from a supplied empty array, and
`entries()` exposes the checked inline contracts. The containing component is
the subject. For a record member, its containing child and name retain that
association; no new mandatory `subject` JSON field is invented. A range owns
its assertions once, not once per endpoint.

Each entry's `availability()` reports `Missing`, `Nil` or `Supplied`. A range
with either endpoint declared nil is unavailable as a complete static quality
interval; the endpoint values and reasons remain individually accessible.
`Supplied` means a non-nil value was provided, **not** that the result passed
a quality test, that a remote vocabulary was verified, or that the measurement
is accurate. Zero and empty text remain supplied values unless explicitly
declared nil. An absent array, empty array or description without a value
does not manufacture a value or a successful assessment.

Use the checked contracts to retain quality. The lower-level domain
`ScalarComponent`/`RangeComponent` views do not by themselves carry these
assertions. Exact original source bytes remain available from the contracts,
including units, definitions and supplied extension context such as method,
version, evaluator and coverage conditions. Source preservation does not
validate an extension's external meaning or establish evaluator authority.
Description round trips here mean retaining and recompiling those bytes,
not an implemented observation/command payload codec.

## Boundaries and evidence

Reference entries fail explicitly as unsupported at this stage; they are not
ignored or fetched. Per-record binding and retained nonlocal quality-reference
metadata belong to [#37](https://github.com/DGIWG-P507/glaux-server/issues/37).
Complete component/reference assembly belongs to
[#38](https://github.com/DGIWG-P507/glaux-server/issues/38). This change does not
add aggregate-root or Geometry quality, endpoints, payload fields, uncertainty
fusion, a provenance service or a new conformance claim.

The source basis is the approved
[Guide §4.3](https://github.com/DGIWG-P507/glaux/blob/0ad4504c75ec13c7ba530a039c7f80d9273380c1/Docs/Plans/glaux-server/glaux-server-implementation-guide.md#43-sensorml-swe-common-validation-and-semantic-bindings),
[SWE 3.0 §8.2.15](https://docs.ogc.org/is/24-014/24-014.html#_quality_union)
and the unchanged packaged
[simple-component schema](../crates/glaux-standards/corpus/originals/csapi/swecommon/schemas/json/AbstractSimpleComponent.json).
This is evidence for Glaux's documented JSON interpretation, not a claim that
the published JSON schema specifies this array form.

Independently authored fixtures check all four types, exact supplied facts,
subject attachment, retained source, malformed forms, units, constraints, nils
and bounded recursion. Hosted CI runs the tests in the existing Rust lane and
checks their discovery and actual execution. Disposable compiled faults must
first have passing unmodified baselines, then fail the intended assertions when
quality validation is bypassed or missing quality is reported as supplied.
The task PR records actual results and separate review. Database, broker and
external-client behavior are not claimed by these model tests.
