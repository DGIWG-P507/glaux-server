# SWE component references

[Task #35](https://github.com/DGIWG-P507/glaux-server/issues/35) adds bounded
local reference resolution. To reuse one component description elsewhere in a
record, Glaux resolves its declared ID and keeps both the original link and its
target. It does not copy an expanded tree or replace the original document.

## What is resolved

`glaux_standards::references::ComponentGraph` is a separate immutable reference
index over the exact source bytes. It records components in declaration order,
their source locations and permitted association links. Local references resolve
to component IDs, not field names, JSON pointers or binary-encoding member paths.
The same target can be shared without duplicating its source.

The permitted association positions are DataRecord fields, DataChoice items,
DataArray/Matrix element types and array element counts. Vector coordinates and
DataChoice selectors are inline components, not additional association slots.
Unknown extension objects, semantic definition URIs, unit references and frame
identifiers never become graph edges or alternative sources of component IDs.

A fragment-only link such as `#SAMPLE_COUNT` resolves within this document.
Percent-encoded fragment bytes are decoded once as UTF-8; IDs remain
case-sensitive, and plus signs and slashes are literal ID characters, not form
encoding or traversal instructions. Empty or malformed local references,
duplicate component IDs and missing local targets fail explicitly.

Other URI references, including relative, HTTPS, file and data references, remain
explicitly nonlocal. Their original link, role, arcrole, title and source bytes
are preserved without fetching anything. A preserved nonlocal link is not a
resolved component or proof that the referenced resource exists.

## Bounds and composition

The existing complete-document byte, syntax-depth, node, member, string and
numeric bounds apply first. Separate graph limits also bound component count,
reference count, resolved traversal depth and total visited occurrences: a small
JSON document can otherwise describe a deep chain or a rapidly expanding graph.
The Glaux budgets are 512 inline components, 512 reference associations,
32 component levels (root is level one), and 4,096 visited component
occurrences, counting repeated local targets. These are implementation resource
limits, not sizes imposed by SWE. A nonlocal edge has no target occurrence.
Traversal follows declaration order and rejects cycles across containment and
local-reference edges. Shared acyclic targets remain valid within the bounds.
Failures return bounded categorical errors, not supplied IDs or fetched data.

An element-count target must be a Count. Matrix element restrictions and the
descriptor-only rule for array elements also apply through resolved links;
using a link cannot evade those rules. Runtime array counts, encoded record
occurrences and ordering of decoded values are not established by finding a
descriptor with the correct ID.

This graph stage is separate from the existing inline typed-component compilers.
A graph result establishes structural validity and reference relationships, not
every scalar constraint, runtime value or encoding rule. The owning
[compiled-contract task #38](https://github.com/DGIWG-P507/glaux-server/issues/38)
composes these stages into the complete immutable validation/codec description.
The existing Array/Aggregate APIs do not silently change their acceptance or
pretend to compile reference-bearing payloads. Inline and dynamic quality
semantics belong to #36/#37; binary member-name paths and payload execution
belong to their codec tasks.

The original enclosing schema is checked, not merely each referenced object.
The opt-in count-reference correction described in
[array components](array-components.md#original-schemas-and-the-qualified-reference-correction)
remains explicit and records a qualified result; the packaged original schemas
and retained source bytes are unchanged.

## Sources and verification

Controlling sources are Guide §§4.3/4.10/8.1.1 and the pinned SWE
[`basicTypes.json`](https://github.com/opengeospatial/ogcapi-connected-systems/blob/8e03b236a049849f2ccc24b4fd9fdce5ff69bed2/swecommon/schemas/json/basicTypes.json)
ID/association definitions and DataRecord, DataChoice, DataArray, Matrix and
Vector schemas in the same directory.
[SWE 3.0](https://docs.ogc.org/is/24-014/24-014.html) §§8.5/8.8 distinguish
array-count references from binary encoding's component-name paths.
[RFC 3986](https://www.rfc-editor.org/rfc/rfc3986.html) §§2.4/3.5 supply URI
fragment parsing rules; SWE supplies the meaning of the resulting ID.

Independent synthetic fixtures distinguish similarly named components by exact
ID, target value and source location. Tests cover nested/shared links, retained
nonlocal metadata, invalid targets, duplicates, unresolved locals, cycles and
the limits immediately below, at and above each bound. Generated graphs check
exact targets rather than only successful parsing. A compiled disposable fault
must fail an intended behavioral assertion after its unmodified baseline passes.
Required test discovery and actual execution use the existing hosted CI lanes.
Actual results, limitations and separate review are recorded in the PR and issue.

No network/file resolver, new dependency, endpoint, database operation or
completed conformance class is introduced.
