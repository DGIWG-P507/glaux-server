# SWE DataChoice components

[Task #32](https://github.com/DGIWG-P507/glaux-server/issues/32) adds named
alternatives and validation of the selected component at the shared-model
boundary (Guide §4.3). To represent a value that can have several different
shapes, the caller supplies the selected item's name and its typed input; Glaux
checks that item alone. Passing another item's rules cannot rescue a mismatch.
This is not an aggregate JSON/Text/Binary decoder or a conformance declaration.

## Description and selection

`glaux_standards::choice::ChoiceContract::compile` retains the original source
and compiles every alternative, including alternatives not selected by a later
value. Items keep declaration order and names unique within their parent.
Two or more items are required by the conceptual model. The original JSON
schema omits that minimum; structural schema acceptance and semantic rejection
remain distinct, with no upstream schema edits. DataChoice's label and definition
are optional; supplied metadata is retained.

Alternatives can use the implemented scalar/range families, DataRecord, Vector,
and nested DataChoice. These reuse existing exact-number, time, unit, constraint
and nil handling. Linked-only children and not-yet-implemented component families
fail explicitly, without retrieving anything. Nested ranges retain the original
schema defaults described in [aggregate components](aggregate-components.md).

`check_value(&[NamedValue])` requires exactly one selection. `ComponentValue`
distinguishes scalar/range JSON leaves from already-decoded Record, Vector and
Choice nodes; it does not accept an encoded aggregate payload. Unknown names,
zero/multiple selections, wrong value kinds and invalid selected values fail.
The result is `CheckedChoiceValue` with the exact selected name and a typed
`CheckedComponentValue`, including nested checked values and metadata.

`AggregateContract::check_value` also checks records/vectors containing choices.
Records preserve declared field order and represent an omitted optional field
as `None`; a supplied null is not absence. Vector coordinates remain required,
ordered, and bound to their enclosing frame. Future codecs own any wire-specific
omission/null or selector mapping; this API does not invent those mappings.

Optional `choiceValue` is retained as a separately validated Category contract,
available through `choice_value()`. Its own values/constraints are checked by
that contract. It describes an encoded-stream selector token, not an additional
token supplied to this model API. Glaux neither requires its enumeration to
equal all item names nor uses it as a hidden filter on explicit model selection.
An inline selector or alternative value does not auto-select an item, supply a
missing payload, or make that value a constant discriminator. Decoding a wire
selector and applying its Category rules belong to the owning codec task.

## Sources and bounded diagnostics

Sources are pinned to SWE/CSAPI commit
[`8e03b236a049849f2ccc24b4fd9fdce5ff69bed2`](https://github.com/opengeospatial/ogcapi-connected-systems/tree/8e03b236a049849f2ccc24b4fd9fdce5ff69bed2/swecommon).
The [choice conceptual rules](https://github.com/opengeospatial/ogcapi-connected-systems/blob/8e03b236a049849f2ccc24b4fd9fdce5ff69bed2/swecommon/standard/sections/clause_7.4_uml_choice_components.adoc)
and [UML figure](https://github.com/opengeospatial/ogcapi-connected-systems/blob/8e03b236a049849f2ccc24b4fd9fdce5ff69bed2/swecommon/standard/figures/fig7.27.png)
correspond to published SWE 3.0 §8.4.1 (unique names, item multiplicity, optional
selector). Requirements 79 and 87 establish unambiguous selection and validation
against the selected item's structure; the latter's
[JSON encoding rule](https://github.com/opengeospatial/ogcapi-connected-systems/blob/8e03b236a049849f2ccc24b4fd9fdce5ff69bed2/swecommon/standard/requirements/encoding_rules/json/requirement6.adoc)
is a source for the semantic tests, not a claim that this task implements that
wire mapping. The fixed DataChoice structural entry asserts formats, as the
DataRecord/Vector entries do, without changing source schemas.

The existing complete-document parser budgets apply before description
compilation. Borrowed model input also has a total byte budget across leaf
bytes and names, plus depth, node, per-container member and name-length bounds,
checked before semantic recursion or checked-output allocation. Diagnostics
carry an error kind and bounded declared-child indices: `[3, 0]`, for example,
identifies item 3's child 0. They do not echo input values or unknown names;
unknown selection/member errors identify the containing component. Whole-input
budget failures identify the root. Detailed child diagnosis never bypasses the
enclosing original schema check. Existing `AggregateContract::compile` retains
its earlier root-schema-first error categories.

## Verification

Nine required test functions use independently authored mixed alternatives,
including disjoint exact Count constraints above binary64 integer precision,
nested records/vectors/choices, nils, optional fields and invalid descriptions.
They assert selected names, exact values, retained metadata and error paths.
Sixty-four deterministic generated descriptions vary alternative names/order
and disjoint values; each checks both correct selection and wrong-arm rejection.
Round-trip source checks supplement, not replace, those independent expectations.

The existing `rust-suites` lane must discover and execute every named test. A
controlled fault in that same lane tries other arms after the named arm fails.
The unchanged test must first pass, then fail specifically because `zLow`
incorrectly accepts a value valid only for `aHigh`. Compilation/setup failures
and timeouts are not detection. Hosted results and separate review are recorded
in the PR and issue; this document is not an execution claim.

No command execution/authorization, public-schema redaction, stream revision
management, HTTP routes, persistence, reference resolution or wire framing is
added here.
