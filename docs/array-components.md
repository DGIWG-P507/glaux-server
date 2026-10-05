# SWE DataArray and Matrix descriptions

[Task #33](https://github.com/DGIWG-P507/glaux-server/issues/33) describes
repeated components under Guide §4.3. To describe an image with rows and columns,
for example, Glaux retains two nested arrays and their counts in outer-to-inner
order. It does not flatten them, allocate that many pixels, or decode payloads.

## Counts, elements and frames

`glaux_standards::array::ArrayContract` compiles immutable descriptions and preserves
original bytes.
DataArray elements can use the implemented scalar/range and aggregate families.
Matrix elements are narrower: Count, Quantity, Time, or another Matrix. Matrix
frames are optional; supplied frame metadata is retained without coordinate
transformation or registry lookup. Vector-specific mandatory frame/axis rules
are not imposed on Matrix. A parent frame is inherited when a child has no
explicit frame; an explicit child declaration is retained, not overwritten.

Each dimension has an `elementCount`. A Count description with an inline value
means a fixed size; without a value it describes a variable size. This special
count form does not require a synthetic type, definition or label. Fixed counts
are exact positive integers and must satisfy supplied constraints. Published
SWE §§10.3.6/10.4.5 distinguish fixed sizes of at least one from a variable
array whose runtime size can be zero; this task does not decode runtime sizes.
A huge valid count is retained as a count, not converted into a
machine-sized allocation. Nested arrays retain each dimension's own meaning.

A reference count preserves its `href` and supplied association metadata, with
unresolved status explicit. Locally checkable invalid forms are rejected. The
full graph/occurrence resolver belongs to #35; compiling a reference does not
claim to have found its runtime count or established stream decoding order.
There is no network or filesystem retrieval through these references.
Exact local references whose known targets are all non-Count components are
rejected; missing or ambiguous targets remain unresolved. Specialized count
descriptions with nil or quality declarations are explicitly unsupported here,
not interpreted as a nil-sized array.

Array elements are descriptions, not inline data values. Their data-bearing
descendants cannot carry inline values; fixed nested dimension counts are
descriptor metadata, not forbidden payload values. This task does not accept
encoded array payloads or claim to execute an encoding descriptor. Selecting an
array in a model-value operation fails explicitly as unsupported at this stage.

The complete source is bounded before recursive traversal by the existing
byte/depth/node/member/string/numeric limits. Dimension traversal is bounded by
that description, never by multiplying the declared counts. Unknown extensions
remain source data, not permission to replace checked component semantics.

## Original schemas and the qualified reference correction

Original DataArray/Matrix JSON schemas do not require `elementCount`, although
the conceptual DataArray model requires it. The descriptor compiler requires an
explicit count rather than inferring a dimension from absent payloads. Original
schema acceptance and semantic acceptance are distinct.

The original `elementCount` schema also uses `oneOf` between an association
reference and an inline ElementCount. The inline branch is an open object with
no required members, so a reference-only object matches both and is rejected.
That is a conflict with the standard's described variable-count reference form.

The default compile path retains the original verdict. Callers may explicitly
select `CountReferenceSchema::DisjointReferenceCorrection`: a separate local
catalog excludes objects containing `href` from the inline-count branch, while
retaining that branch's original schema and the full enclosing wrapper checks.
The qualified result is recorded as a correction, never an unmodified upstream
pass. Mixed reference/inline count declarations and other malformed content
still fail. Packaged originals and retained source bytes are never rewritten.
This is a named compatibility option, not a general schema repair service.

## Controlling sources and verification

SWE source is pinned to
[`8e03b236a049849f2ccc24b4fd9fdce5ff69bed2`](https://github.com/opengeospatial/ogcapi-connected-systems/tree/8e03b236a049849f2ccc24b4fd9fdce5ff69bed2/swecommon).
The [conceptual block rules](https://github.com/opengeospatial/ogcapi-connected-systems/blob/8e03b236a049849f2ccc24b4fd9fdce5ff69bed2/swecommon/standard/sections/clause_7.5_uml_block_components.adoc)
define fixed/variable counts, recursive dimensions, descriptor-only elements,
and Matrix's restricted element family (published SWE 3.0 §§8.5.1–8.5.2,
requirements 45/47). The
[JSON mapping](https://github.com/opengeospatial/ogcapi-connected-systems/blob/8e03b236a049849f2ccc24b4fd9fdce5ff69bed2/swecommon/standard/sections/clause_9.4_json_block_components.adoc)
and unmodified
[DataArray schema](https://github.com/opengeospatial/ogcapi-connected-systems/blob/8e03b236a049849f2ccc24b4fd9fdce5ff69bed2/swecommon/schemas/json/DataArray.json)
are checked separately from those semantic rules. Illustrations with encoded
values are not authority to invent missing sizes for descriptor-only input.

Independent fixtures check element kinds/names, exact counts, ordered dimensions,
fixed and variable nested structures, Matrix restrictions and frame meaning,
source preservation, and malformed or inconsistent descriptions. Bounded
generated dimensions/counts exercise both positive and negative cases. Tests
keep original-schema results separate from explicitly qualified results.
Required discovery and execution use the existing CI lanes, with a compiled
disposable fault demonstrating a meaningful assertion failure from a passing
baseline. Actual results and separate review belong to the PR/execution record.

No array payload encoding/decoding, database array layout, compression, complete
reference graph or extra SWE component family is introduced here.
