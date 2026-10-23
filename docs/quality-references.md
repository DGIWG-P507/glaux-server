# Per-record SWE quality references

[Task #37](https://github.com/DGIWG-P507/glaux-server/issues/37) connects a
measurement to quality information already present in its record. For example,
a pressure component's `quality: [{"href":"#PRESS_QC"}]` refers to the ordinary
Category field with component ID `PRESS_QC`. Each record carries its own value
for that field. Binding does not add a hidden field, reorder fields, or turn a
quality value into a universal confidence score.

## Description and occurrence are different

The local fragment identifies one component description within the retained
contract. Its eligible type is Quantity, QuantityRange, Category or Text. The
binding must also identify a particular occurrence of that component. Knowing
the component ID alone is insufficient when records repeat.

Glaux walks an already-decoded model and derives occurrence addresses from its
structure. Shared containing records and repeated elements must refer to the
same occurrence on both sides of the link. This keeps a measurement in one
record from receiving its neighbour's otherwise valid quality value. Reusing
a record description at two different positions likewise does not merge their
values. If multiple compatible target occurrences remain, binding fails as
ambiguous rather than choosing the first, latest or most favourable value.

An omitted target or inactive branch supplies no quality value. A declared nil
retains its reason and remains unavailable, including a QuantityRange with a nil
endpoint. A static descriptor value does not fill in a missing per-record value.
Supplied values use the target's existing scalar/range checks; `Supplied` is
availability, not a quality verdict. Nonlocal links keep their original metadata
and remain unresolved without network or filesystem access.

The occurrence addresses are internal model locations, not new JSON fields or
wire identifiers. Inline quality descriptions and selector/count metadata do not
become payload occurrences. Local dynamic targets must be ordinary payload
components; pointing at a metadata-only quality description is not per-record
resolution. Nested quality references retain the containing payload subject's
occurrence context.

## Bounds and limits of this stage

The API is `glaux_standards::quality::binding::QualityReferencePlan`. `compile`
retains the exact source through `graph()` and prepares the eligible target
contracts. `bind` accepts the existing borrowed `ComponentValue` model, with
`Array` representing already-decoded repetitions. It returns associations with
the graph reference index, subject and target occurrence addresses, and checked
scalar/range values or explicit `Missing`/`Nonlocal` states. It never accepts a
caller-selected target occurrence. An error returns no partial binding result.

The complete submitted description is bounded before indexing. Duplicate IDs,
unresolved local fragments, ineligible targets and reference cycles fail.
Quality edges participate in the existing component/reference/traversal budgets;
they cannot evade those bounds by living under the permissive JSON `quality`
member. Already-decoded values also have whole-model node, depth, member, string
and byte bounds. Missing values do not justify borrowing another occurrence.

This stage checks quality binding, not every payload obligation. Repeated model
values are not an implementation of JSON/Text/Binary framing, runtime array-count
validation or complete compiled-contract assembly. The latter is provided by
the separate [compiled-contract API](compiled-contracts.md); codecs remain with
their owning tasks. No quality endpoint, database change, aggregation policy,
external lookup or additional conformance claim is introduced.

## Interpretation and evidence

The controlling source is the approved
[Guide §4.3](https://github.com/DGIWG-P507/glaux/blob/0ad4504c75ec13c7ba530a039c7f80d9273380c1/Docs/Plans/glaux-server/glaux-server-implementation-guide.md#43-sensorml-swe-common-validation-and-semantic-bindings),
with §§4.10 and 8.1.1 for quality meaning and verification. As with
[inline quality](inline-quality.md), the optional JSON array and its reference
interpretation are explicit Glaux choices: the original published simple
component schemas remain unchanged. The occurrence-selection rule above is a
bounded implementation of the Guide's requirement to identify both component
and occurrence, not a claimed upstream JSON encoding rule.

Independent fixtures use different expected values in neighbouring nested and
repeated records. They check exact targets, values and nil states; all eligible
types; ambiguous, duplicate, invalid and cyclic links; nonlocal metadata; and
resource bounds. The existing hosted Rust lane checks discovery and execution.
A disposable compiled fault deliberately selects a neighbouring record's value
and must fail the intended semantic assertion after the unmodified baseline
passes. The PR records actual execution and separate review; these checks do not
claim database, broker or external-client interoperability.
