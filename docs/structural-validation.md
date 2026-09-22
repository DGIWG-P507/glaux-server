# Bounded offline structural validation

[Task 1.2.2 / issue #8](https://github.com/DGIWG-P507/glaux-server/issues/8)
implements the initial structural-validation boundary in
[`glaux-standards`](../crates/glaux-standards/src/validation.rs). Its controlling
sources are the [Guide §§2.4, 4.3 and 9.2](https://github.com/DGIWG-P507/glaux/blob/main/Docs/Plans/glaux-server/glaux-server-implementation-guide.md)
and [Roadmap task 1.2.2](https://github.com/DGIWG-P507/glaux/blob/main/Docs/Plans/glaux-server/glaux-server-roadmap.md).
The [original corpus](standards-corpus.md) remains unchanged.

This document describes the implementation, checks and selected development
evidence. It does not substitute for the final required CI and delivery record.
The issue/PR identifies the tested commit, actual commands, outcomes, failure
controls and separate review; structural proof is not service conformance.

## Dependency selection and scope

The selected library is Rust `jsonschema = "=0.56.0"`, with `default-features = false`.
The selected crate supports Draft 2020-12 and Draft-07 and has an MIT licence and
Rust 1.85.0 minimum; Glaux's workspace toolchain remains pinned separately.
Disabling defaults excludes the crate's HTTP and filesystem resolution features.
The version-specific [package manifest](https://github.com/Stranger6667/jsonschema/blob/rust-v0.56.0/crates/jsonschema/Cargo.toml),
[workspace licence/toolchain metadata](https://github.com/Stranger6667/jsonschema/blob/rust-v0.56.0/Cargo.toml)
and [upstream README](https://github.com/Stranger6667/jsonschema/blob/rust-v0.56.0/README.md)
are the selection sources. The resolved lockfile and
[dependency/licence inventory](dependencies.md) are required delivery evidence;
an exact top-level version declaration alone does not identify every dependency.

`serde_json = "=1.0.151"` enables `arbitrary_precision` and `raw_value`. Parsing retains numeric
values without first forcing them through a floating-point number. An authored
regression checks preservation of a large integer beyond `u64`. This is a
parser check, not proof of every numeric assertion inside the validator, exact
decimal arithmetic, application numeric conversions, database representation
or the complete numeric work owned by issue #10. Those distinctions must remain
visible when extending this boundary.

Raw JSON containers are decoded through `RawValue`, then explicitly constructed
as objects/arrays before scalar tokens use `Value`. This prevents serde_json's
private arbitrary-precision marker from reinterpreting a wire object as a number.
The regression covers the marker as a numeric-position object (reject), an
ordinary extension (preserve), and an escaped nested key. Before the fix, the
[hosted red run](https://github.com/DGIWG-P507/glaux-server/actions/runs/35675152809)
accepted the wrong wire kind; the [corrected candidate run](https://github.com/DGIWG-P507/glaux-server/actions/runs/35675239342)
passed the regression and all 23 original cases. These were preparation runs,
not the final required CI result; PR #316 records final delivery evidence.

Structural success says that an input satisfies the selected schema under the
pinned implementation. It does not establish units, component-reference
resolution, binary layout, supported wire codecs, request authorization,
operation semantics or CSAPI conformance. The compiler does not explicitly
enable `format` assertions; the library's draft defaults apply. In Draft
2020-12, ordinary format annotations are not a complete URI/date/unit business
validation layer. Referenced resources retain their own dialect declarations;
the internal entry-point wrapper uses Draft 2020-12. See the pinned
[format/draft options](https://github.com/Stranger6667/jsonschema/blob/rust-v0.56.0/crates/jsonschema/src/options.rs).

## Fixed entry points

`StructuralValidator::new()` constructs the embedded catalog, checks its graph
and compiles the fixed contracts. Callers retain that validator for reuse.
`validate(contract, bytes)` accepts a `Contract` enum and JSON bytes. There is no
public method accepting a schema URL, filesystem path or supplied JSON Schema.

In the table, **SWE** is the verified alias base
`https://schemas.opengis.net/sweCommon/3.0/json/`; **publication** is the original
Connected Systems source tree at commit
`8e03b236a049849f2ccc24b4fd9fdce5ff69bed2`. The manifest supplies their local bytes
and explicit aliases.

| Contract | Fixed target within its base |
|---|---|
| `Quantity` | SWE `Quantity.json` |
| `SweRecord` | SWE `DataRecord.json` |
| `PhysicalSystem` | publication `sensorml/schemas/json/PhysicalSystem.json` |
| `ObservationSwe` | publication `api/part2/openapi/schemas/json/observationSchemaSwe.json` |
| `CommandSwe` | publication `api/part2/openapi/schemas/json/commandSchemaSwe.json` |
| `JsonEncoding` | SWE `encodings.json#/$defs/JSONEncoding` |
| `TextEncoding` | SWE `encodings.json#/$defs/TextEncoding` |
| `BinaryEncoding` | SWE `encodings.json#/$defs/BinaryEncoding` |

The private compiler uses a small `$ref` wrapper to select the target, preserving
the originals and their reference bases. `validate_encoding(format, bytes)`
also requires the descriptor's `type` to match the caller's fixed `Encoding`
choice. That helper does not replace validation of the complete applicable
observation/command wrapper. XML is not an entry point.

The original aggregate `encodings.json` root excludes BinaryEncoding although
its named definition and applicable wrapper permit it. The fixture test calls
that root privately to retain the source diagnostic. Public binary descriptor
validation selects the named definition. Likewise, Quantity's required nonempty
JSON label is preserved; no label is synthesized from another property.

## Embedded resolution and schema preflight

[`build.rs`](../crates/glaux-standards/build.rs) reads the checked-in manifest and
generates `include_str!` entries for its 129 original schemas and their explicit
aliases. Build-time file access is limited to resolved paths under the corpus
directory; the resulting executable contains the schema strings. Manifest
integrity/digests remain covered by the existing corpus checks. Runtime catalog
construction parses those strings and does not open schema files.

The compiler populates `Registry::new().add(...).prepare()` from the embedded
catalog and installs the same `DenyRetrieval` at registry preparation and validator
construction. This hook only increments an observation counter and returns an
error; it has no network, filesystem or fallback implementation. It replaces the
library's default retriever, in addition to disabling the transport features.
See the pinned
[registry implementation](https://github.com/Stranger6667/jsonschema/blob/rust-v0.56.0/crates/jsonschema-referencing/src/registry/mod.rs)
and [retriever implementation](https://github.com/Stranger6667/jsonschema/blob/rust-v0.56.0/crates/jsonschema-referencing/src/retriever.rs).
Instance strings named `$ref`, `href` or similar are data, not requests to load
a new schema. The test observes zero retrieval requests while compiling the
packaged Quantity contract and validating HTTP/file/data-URI instance metadata.
Missing schema URIs exercise the observer and fail; a missing catalog dependency
also exercises registry-preparation denial. Thus the zero-count observation has
a positive control proving the observer is connected. The graph's separate
HTTP/file/data/URI-escape canaries are rejected before compilation. No external
server is contacted or arbitrary local file read by these probes.

[`schema_guard.rs`](../crates/glaux-standards/src/schema_guard.rs) preflights the
fixed catalog before compilation. It follows only schema-valued keywords,
indexes resource IDs, anchors and schema positions, and checks reference targets
against those in-memory indexes. URI resolution and fragment decoding use the
pinned library's URI facilities. Missing resources, missing or malformed
pointers, missing anchors, conflicting IDs and references into instance data
such as `default` are rejected. Defaults, examples, constants and enum values
are not recursively mistaken for schemas.

The guard distinguishes references that apply another schema to the same
instance from descent into a property or array member. Its cycle graph includes
`$ref`, the initial static targets of `$dynamicRef`/`$recursiveRef`, composition,
conditionals and schema dependencies. Edges that descend into instance members
are omitted from this cycle graph; definitions are indexed independently.
Every indexed node starts a graph check, so a non-progressing cycle inside a
definition or below a property is still rejected. Valid finite recursive SWE
records/arrays and SensorML components can therefore proceed to validation.

This extra check is necessary because upstream 0.56.0 intentionally treats some
pure reference cycles as satisfied, as shown in its
[reference-cycle tests](https://github.com/Stranger6667/jsonschema/blob/rust-v0.56.0/crates/jsonschema/src/keywords/ref_.rs).
The guard is conservative catalog preflight, not a general JSON Schema
implementation or proof for client-supplied dynamic schemas. It checks dynamic
references at their initial static targets; complete dynamic evaluation belongs
to the pinned validator and must be tested against the selected corpus.

## Resource budgets and diagnostics

These initial implementation limits are local resource budgets, not size limits
imposed by SensorML, SWE Common or JSON Schema.

| Input check | Maximum |
|---|---:|
| Raw JSON bytes before parsing | 262,144 |
| Open object/array containers during lexical scan | 32 |
| Parsed JSON values, including containers and scalar values | 4,096 |
| Members per object or elements per array | 512 |
| UTF-8 bytes per decoded string, including object keys | 16,384 |
| Indexed schema nodes across the catalog, including alias copies | 20,000 |
| Nesting of schema-valued positions, with a root at depth one | 128 |
| Nodes on a same-instance schema path | 256 |
| Fancy-regex backtracking attempts | 20,000 |
| Approximate compiled regex size | 1,048,576 bytes |
| Lazy DFA cache capacity | 1,048,576 bytes |

The scanner checks raw size and container depth before constructing the full
JSON value, decodes strings and rejects duplicate decoded object keys, including
equivalent escaped spellings. `serde_json` then checks the complete JSON syntax.
An iterative walk checks node/member counts before schema evaluation. Array and
total-node checks occur after parsing, so they are not claims of zero allocation
for rejected input; the raw-byte ceiling also bounds that parse input.

Schema graph limits constrain the fixed compilation input and same-instance
reference chains. Together with instance-depth/size limits, they limit the
recursive input accepted by this boundary. They are not a validator-wide fuel
counter, a per-request deadline, a process memory quota or proof that every input
below the limits is cheap. Upstream's public validation API has no general
evaluation-step/depth budget. Branching schema evaluation still needs empirical
checks for the fixed corpus. Validation uses `is_valid()` and returns bounded
`Failure` variants rather than schema contents or detailed instance values;
startup compilation errors are a separate internal result.

## Authored verification and its limits

[`validation/tests.rs`](../crates/glaux-standards/src/validation/tests.rs)
contains a runner for the 23 independently authored expectations from #7:
seven expected valid and sixteen expected invalid. They cover the binary
root/definition/wrapper distinction, byte-order/member negatives, Quantity
labels directly and inside wrappers, and positive/deep-negative recursive SWE
and SensorML documents. The hosted candidate executed all 23 with the recorded
seven accepted/sixteen rejected outcomes; the final required run repeats them.
The unresolved binary component
fixture is expected to pass structure and later fail the semantic compiler in
#140; structural acceptance must not become product acceptance.

Additional authored tests cover fixed encoding selection, malformed JSON,
escaped duplicate keys, input boundaries, numeric preservation, external-link
data, catalog escapes, pointer/anchor/ID resolution, graph cycles and graph
budget boundaries. Required execution, meaningful false-green controls and
separate review remain part of the [CI procedure](ci.md).

[`fuzz/schema_parser.rs`](../fuzz/schema_parser.rs) is a deterministic mutation
driver exposed as the `schema-parser-fuzz` Cargo example. Version 2 checks 1,024
cases with seed `0x475c000800000001`: eight partitions of 128 cases cover varied
valid Quantities, recursive SWE/SensorML and observation wrappers, invalid direct
and nested labels/encodings, malformed valid seeds, byte mutations of both valid
and invalid seeds, and concrete regressions. Constructive cases have independently
specified verdicts; arbitrary mutations check determinism and insignificant outer
whitespace without claiming a complete oracle. At least 256 distinct accepted
inputs and 640 distinct overall inputs are required, with per-partition counts.
It also checks explicit size/depth rejection and both accepted/rejected outcomes.
Two [saved regression inputs](../fuzz/corpus/schema-parser/) cover missing and
duplicate labels.

This driver is not coverage-guided libFuzzer, an exhaustive parser test or an
independent oracle for every mutation. Its 30-second assertion is checked
between iterations and cannot preempt a stuck validation call. A successful run
would establish only the stated seed, cases and assertions on that commit.
Mutation-fuzz evidence also does not substitute for fault controls that show
the important fixture assertions detect plausible wrong validator behavior.

On the authorized hosted Linux test environment, the task-specific commands are:

```sh
cargo test -p glaux-standards --locked --offline -- --nocapture
cargo run -p glaux-standards --example schema-parser-fuzz --locked --offline
```

Run the full required workflow as well. Record actual outputs and failures;
neither this command list nor the existence of the test source is proof that
the checks ran.
