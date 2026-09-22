# Request and response validation projections

[Task 1.2.6 / issue #12](https://github.com/DGIWG-P507/glaux-server/issues/12)
adds fixed projections for System GeoJSON, System SensorML JSON, DataStreams,
ControlStreams and ordinary observation JSON. The implementation is in
[`projection.rs`](../crates/glaux-standards/src/projection.rs). Its planning
baseline is [Guide v1.21 §§4.3, 4.6, 6.2 and 13][Guide] at planning commit
`6c801a47227639d623d41297b9c9074ba0c91eb1`. The CSAPI schema source is commit
`8e03b236a049849f2ccc24b4fd9fdce5ff69bed2`, with its packaged dependency closure
and individual digests recorded in the [original corpus](standards-corpus.md).

This is a validation primitive, not an HTTP endpoint, persistence layer,
authorization decision or complete resource-admission check. It does not compile
stream contracts, execute codecs, validate observation results against their
parent contract, or establish full SensorML/SWE semantics or OGC conformance.

## Original schemas and explicit adaptations

JSON Schema `readOnly` and `writeOnly` annotations do not by themselves change a
`required` array or prohibit an output member. Original files remain untouched.
Startup clones the embedded catalog for request validation and changes exactly
these four top-level arrays; nested requirements and property constraints remain
unchanged. Each original array must match the pinned expected value exactly, or
startup fails. Both catalogs use the existing offline allowlisted compiler.

| Pinned source | Original `required` | Request `required` |
|---|---|---|
| [baseStream.json][BaseStream] | `id`, `name`, `formats` | `name` |
| [dataStream.json][DataStream] | `name`, `system@link`, `observedProperties`, `phenomenonTime`, `resultTime`, `resultType`, `live` | `name` |
| [controlStream.json][ControlStream] | `name`, `system@link`, `controlledProperties`, `issueTime`, `executionTime`, `live`, `async` | `name`, `async` |
| [observation.json][Observation] | `id`, `datastream@id`, `resultTime` | `resultTime` |

The request catalog is internal; an adapted schema is not published under an
upstream identity or reported as an unmodified-source result. Important details
are handled explicitly, not by recursively deleting annotated properties:

- Stream creation additionally requires `schema`, following
  [dataStream_create.json][DataCreate] and
  [controlStream_create.json][ControlCreate]. The schema's nested wrapper and
  component requirements still apply. Replacement and complete merged-candidate
  structural checks do not impose the creation-only presence requirement.
- Stream responses use the original required arrays and reject any `schema`
  member, including `null`; absence is different from a null output value.
- System responses require top-level `id` and `links` in addition to their
  original schema checks. These are prose-derived response checks, not claims
  that the [GeoJSON][SystemGeo] or [SensorML][SystemSml] schema requires them.
  Part 1 §19.2.4 requirement 92 supplies the SensorML local-ID rule; §§19.1–19.2
  and Tables 41/50 supply the encoding/association mappings. The inherited
  Features Part 1 §8.3 requirement 39 and §7.16.2 requirement 35 supply the
  GeoJSON response ID/core-link rules. This primitive checks presence and source
  shape, not URL-ID equality or the complete applicable relation/media-type/link
  inventory; those remain endpoint/navigation work. [Part 1][Part1],
  [Features Part 1][Features1]
- Part 2 §9.2.2 permits the server to derive DataStream `live` and ignore updates
  when it does. Glaux selects that permitted server-summary behavior here.
  The DataStream schema has no `readOnly` annotation on `live`; this choice is
  not inferred from one. [Part 2][Part2], [dataStream.json][DataStream]
- ControlStream `live` has a `readOnly` annotation inside its Boolean branch,
  not on the whole Boolean/null union. Guide §4.9 selects server ownership of
  the whole member, including null. `async` remains required client-writable
  content. Neither all nullable fields nor all Boolean fields become generated.
- Observation requests retain required `resultTime` and exactly one of `result`
  or `result@link`. Response IDs and the parent ID remain required on output.
  This does not validate result values against a DataStream or enforce temporal
  semantics beyond the selected structural contract.

Quantity descriptions still require their published nonempty string `label`,
including inside SensorML outputs and stream-schema wrappers. The adaptation
does not invent labels, require them on all component types, prohibit whitespace
labels, or remove nested component IDs. See [Guide §4.3][Guide] and the packaged
[Quantity schema](../crates/glaux-standards/corpus/originals/csapi/swecommon/schemas/json/Quantity.json).

## Use the appropriate API boundary

`ProjectionValidator::new()` compiles the fixed resource set. A caller selects
the `Resource` and `Projection` enums; input cannot select a schema URI.

| API | Meaning and limits |
|---|---|
| `validate_original(resource, input)` | Diagnostic against the unmodified source schema, without operation policy. It can reject a valid minimal stream request or accept a write-only output leak. It is not admission. |
| `validate(resource, projection, input)` | Operation/direction shape check for `CreateRequest`, `ReplaceRequest`, complete `MergedPatch`, or `Response`. It does not perform trusted UID, parent or locked-contract checks, and returns no writable document. |
| `request(resource, projection, input, context)` | Shape check, trusted-context checks and extraction of writable content. Rejects `Response`. Its returned JSON value is still not authorized or committed resource state. |
| `patch_members(resource, input)` | Checks a partial patch's member intent before merging and returns its permitted members. It is not validation of the complete result and does not apply Merge Patch. |

The fixed System PATCH representation is SensorML JSON. `MergedPatch` and
`patch_members` reject `SystemGeoJson`; GeoJSON creation and replacement remain
supported structural projections. `MergedPatch` never means that a small patch
fragment can stand in for a complete resulting resource.

After bounded parsing, `validate()` and `request()` remove only the submitted
outer resource-local `id` before schema validation for `ReplaceRequest` and
`MergedPatch`. The selected Features transaction revision's replacement
`/put-rid` and Update `/rid` requirements ignore that member unconditionally:
null, an array, an empty string and a different identifier are all ignored.
The target comes from the request path, not this value. `patch_members()` also
removes the outer `id` before checking partial-patch intent. [Guide §4.6][Guide],
[selected transaction revision `9ca25f56a58ed822ea8a685a7a41afa7181aaa8b`][Transactions]

Creation is separate: Glaux retains present-ID schema checks on `CreateRequest`
before discarding an accepted supplied local ID during writable extraction.
`Response` and `validate_original()` retain the source ID shape checks as well.
Other known members are structurally checked before trusted-context checks and
generated-member removal. None of these operations recursively removes nested
component/content IDs, and the PUT/PATCH exception does not bypass bounded
parsing or protected UID, parent and contract checks.

Trusted `RequestContext` must come from the caller's established target/state,
not from fields copied out of the untrusted request:

- `existing_uid` is required for System replacement and complete PATCH
  candidates. An unchanged UID echo is allowed; a changed UID is rejected.
  Missing or malformed required UID content still fails structural/typed checks.
- Streams require `Parent::System`; observations require `Parent::Datastream`,
  even when the request omits its generated parent member. The former carries
  the expected **canonical parent-link `href`**, not the System's domain UID,
  despite using the `Uid` URI type. Comparison is exact; this helper does not
  resolve aliases, dereference links or prove that a URI identifies an accessible
  parent. Observations compare with the expected typed local DataStream ID.
- `locked_schema`, when the owning state check says a stream contract cannot
  change, carries that retained schema. A supplied different parsed JSON value
  is rejected. This is not byte-for-byte source comparison or proof of semantic
  schema equivalence. PUT may omit the separate write-only contract without
  replacing it. A complete locked PATCH candidate may not omit it.

For PATCH, the caller must use this sequence:

1. Run `patch_members` on the actual partial input before merging or restoring
   protected data. Touching generated/protected members such as parent links or
   ControlStream `live`, even with null, is rejected. An unchanged UID echo is
   not rejected here; the complete candidate is compared with trusted state.
2. Apply Merge Patch to the full writable baseline. A stream baseline includes
   its retained write-only `schema`, not merely the public response document.
   Otherwise an unrelated patch could lose the contract, or `schema: null`
   could evade a meaningful removal check.
3. Run `request(..., MergedPatch, ..., context)` on the complete candidate before
   any restoration could conceal a changed/deleted UID or locked schema.
4. Continue the owning operation's remaining semantics, authorization,
   concurrency and transaction checks. None is supplied by this module.

## Observation envelope policy and bounded input

Guide §4.6's Glaux policy ignores unmapped, unadvertised top-level observation
members on input. No observation extension is selected by this initial helper.
`request()` and `patch_members()` remove such root members; `validate()` alone
does not return or clean a document. `foi@id` is not a `samplingFeature@id` alias.
Response validation rejects an unmapped root member with `UnmappedMember`, even
when the permissive original schema accepts it.

This policy is not recursive. Nested `result`, `parameters` and link content
remain present and subject to their applicable current structural checks and
later semantic/parent-contract checks. It is not an extension policy for
SensorML, GeoJSON or every Part 2 resource. Known invalid members and protected
changes do not become acceptable by adding an unknown member.

All entry points use the existing [bounded parser](structural-validation.md#resource-budgets-and-diagnostics)
before any ignoring/removal: 262,144 input bytes, depth 32, 4,096 nodes, 512
members/items per container and 16,384 decoded string bytes. Duplicate keys and
malformed input fail, including inside content that would otherwise be ignored.
Arbitrary-precision JSON parsing avoids a binary64 round trip; the regression
retains `9007199254740993` exactly. That is not a claim of numeric arithmetic,
codec conversion, database or parent-schema correctness. Extraction returns a
parsed value, not preservation of original source formatting/bytes.

Diagnostics expose bounded error categories and fixed paths, not input values
or protected schema details: `Input`, `Missing`, `WriteOnly`, `Protected`,
`UnmappedMember`, `Context`, `UnsupportedProjection` and `Structure`. An output
stream's `schema` presence is rejected before checking missing generated fields.

## Authored verification and remaining evidence

The design includes fourteen named tests: eight independently authored
[fixture tests](../crates/glaux-standards/src/projection/tests.rs) and six
[policy/context tests](../crates/glaux-standards/src/projection/policy_tests.rs).
Their expected documents, field sets and failures are source-derived, not
captured from a production serializer. The number is an execution inventory,
not a quality score or conformance claim.

| Test | Intended wrong behavior detected |
|---|---|
| `minimal_requests_and_complete_responses_have_independent_contracts` | Requiring generated fields on minimal input or rejecting complete independently authored output |
| `generated_response_members_remain_required` | Letting request relaxations leak into required output |
| `writable_required_members_survive_request_projection` | Relaxing client-writable requirements or applying creation-only schema presence to replacement |
| `stream_schema_is_write_only_in_responses` | Leaking a registration schema, including a null member, into output |
| `nested_quantity_label_requirements_survive_projection` | Relaxing nested Quantity labels or imposing unsupported blanket label restrictions |
| `malformed_generated_members_are_not_ignored` | Ignoring known member types beyond PUT/PATCH's explicit outer-ID exception |
| `only_resource_local_ids_are_ignored` | Rejecting null/array/empty/different PUT/PATCH outer IDs, weakening creation/output/source ID checks, erasing nested IDs, or retaining the outer ID in writable extraction |
| `wrong_direction_and_original_schema_results_are_distinct` | Confusing original-source, request and response results or validating a fragment as a whole PATCH result |
| `trusted_context_separates_uid_parent_and_ignored_local_id` | Accepting changed UID/parent associations or missing required trusted context |
| `locked_contract_rejects_change_and_patch_removal` | Replacing or deleting a locked schema, including deletion concealed by absence |
| `partial_patch_intent_is_checked_before_full_candidate_validation` | Losing protected-member intent during merge/restoration or allowing a deleted UID in the complete result |
| `observation_envelope_ignoring_is_not_recursive_or_an_alias` | Retaining unknown root members, inventing an alias, or stripping nested content |
| `ignored_members_do_not_bypass_bounded_lossless_parsing` | Ignoring duplicate/oversized input before safe parsing or rounding an exact integer |
| `adaptations_are_separate_and_fail_when_source_preconditions_change` | Modifying the original catalog or accepting changed/missing adaptation sources |

Four disposable fault controls are planned for the approved hosted runner:

| Control | Injected mistake |
|---|---|
| `original-as-request` | Compile the original catalog in place of request adaptations |
| `write-only-output-leak` | Bypass the response schema-member prohibition |
| `protected-uid-bypass` | Skip comparison with the trusted System UID |
| `locked-schema-removal` | Skip rejection of a locked schema absent from a complete merged candidate |

Each control must first pass its exact unmodified baseline, then compile a
disposable faulty copy and fail the intended behavioral assertion with the
expected actual/expected values. Build failure, missing execution, timeout or an
unrelated assertion is not proof of fault detection. Existing corpus digest
checks separately cover the packaged original bytes.

These are authored checks and verification design, not a statement that they
have run or passed. The issue/PR and [CI instructions](ci.md) record actual
commands, tested head, outcomes, failures/retries and separate review. No local
Rust runtime, tool installation, endpoint exercise or full semantic/conformance
result is established by this document.

[Guide]: https://github.com/DGIWG-P507/glaux/blob/6c801a47227639d623d41297b9c9074ba0c91eb1/Docs/Plans/glaux-server/glaux-server-implementation-guide.md
[BaseStream]: https://github.com/opengeospatial/ogcapi-connected-systems/blob/8e03b236a049849f2ccc24b4fd9fdce5ff69bed2/api/part2/openapi/schemas/json/baseStream.json
[DataStream]: https://github.com/opengeospatial/ogcapi-connected-systems/blob/8e03b236a049849f2ccc24b4fd9fdce5ff69bed2/api/part2/openapi/schemas/json/dataStream.json
[ControlStream]: https://github.com/opengeospatial/ogcapi-connected-systems/blob/8e03b236a049849f2ccc24b4fd9fdce5ff69bed2/api/part2/openapi/schemas/json/controlStream.json
[Observation]: https://github.com/opengeospatial/ogcapi-connected-systems/blob/8e03b236a049849f2ccc24b4fd9fdce5ff69bed2/api/part2/openapi/schemas/json/observation.json
[DataCreate]: https://github.com/opengeospatial/ogcapi-connected-systems/blob/8e03b236a049849f2ccc24b4fd9fdce5ff69bed2/api/part2/openapi/schemas/json/dataStream_create.json
[ControlCreate]: https://github.com/opengeospatial/ogcapi-connected-systems/blob/8e03b236a049849f2ccc24b4fd9fdce5ff69bed2/api/part2/openapi/schemas/json/controlStream_create.json
[SystemGeo]: https://github.com/opengeospatial/ogcapi-connected-systems/blob/8e03b236a049849f2ccc24b4fd9fdce5ff69bed2/api/part1/openapi/schemas/geojson/system.json
[SystemSml]: https://github.com/opengeospatial/ogcapi-connected-systems/blob/8e03b236a049849f2ccc24b4fd9fdce5ff69bed2/api/part1/openapi/schemas/sensorml/system.json
[Part1]: https://docs.ogc.org/is/23-001/23-001.html
[Part2]: https://docs.ogc.org/is/23-002/23-002.html
[Features1]: https://docs.ogc.org/is/17-069r4/17-069r4.html
[Transactions]: https://github.com/opengeospatial/ogcapi-features/tree/9ca25f56a58ed822ea8a685a7a41afa7181aaa8b/extensions/transactions
