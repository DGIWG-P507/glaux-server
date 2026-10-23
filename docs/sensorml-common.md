# Common SensorML descriptive content

[Task #39 / 2.2.1](https://github.com/DGIWG-P507/glaux-server/issues/39)
provides the shared description layer used by later SensorML resource mappings.
To retain what a publisher said about a system or process, it keeps typed
descriptive fields, generated JSON and exact original bytes separate. A contact
or identifier authority is supplied descriptive information, not an authenticated
account, ownership grant or independently verified claim.

## Mapping and source preservation

`glaux_standards::sensorml::CommonDescription::compile` uses the fixed local
`DescribedObject` schema and the existing bounded, duplicate-key-rejecting parser.
It owns the input bytes, media-type metadata and SHA-256 digest. The digest covers
the received bytes, including whitespace, not generated JSON; it is not a
signature. Reordering object members can preserve meaning while changing that
digest. List order, supplied text and absent versus supplied-empty lists remain
distinct.

The media string is bounded source metadata, not HTTP negotiation. The parser
accepts JSON bytes only; a supplied metadata token cannot enable XML processing.
The eventual HTTP owner must enforce the Guide's selected `application/sml+json`
binding and its separately documented negotiation rules.

| Content | Preserved meaning and boundary |
|---|---|
| Common identity | Object type, local `id` when supplied, URI-form `uniqueId`, label, description, language and keywords; no identity generation or merging |
| Identifiers and classifiers | Ordered terms with exact label/value, optional definition and `codeSpace`; the latter identifies the supplied authority, not proof that it recognizes the identifier |
| Contacts | Inline individual or organization details, or named contact links, with their supplied roles; neither form grants access |
| Documentation | Named document links and optional role/description, retaining link metadata without retrieving their targets |
| Validity | Two exact date-time endpoints or the schema's symbolic `now`; concrete reversed intervals fail, and `now` is not replaced with ingestion time |
| Other content | Retained without flattening or inventing typed family semantics; structural acceptance is not proof of complete process, deployment or property behavior |

`generated_json()` reconstructs the mapped fields and preserves the remainder.
Returning that value from an HTTP handler still requires the owning operation's
authorization and family checks. Original bytes must not be used as a way around
those checks. This common layer is not a full family mapper: in particular,
DerivedProperty does not inherit DescribedObject and must not be forced through
this entry point by its later owner.

## Extensions and references

`with_extensions` is an explicit library helper for adding namespaced foreign
members to generated JSON, not an advertised HTTP extension profile. It rejects
reserved common, identity and relationship fields and existing-key collisions,
and does not mutate the compiled description or its source evidence. This helper's
namespace policy is a bounded Glaux mapping, not a claim that every otherwise
permitted unknown JSON member is invalid under SensorML. Nested extension payloads
remain nested and uninterpreted; they do not replace validated fields or configure
process execution.

The mapper has no fetch operation. Document, contact, vocabulary and extension
references remain data, including controlled test URLs. Schema references resolve
only within the pinned local catalog. Supplied links do not prove a remote object
exists or that Glaux may access it.

## Verification and limits

Source-attributed fixtures and independently specified field assertions check
minimal/rich descriptions, exact source evidence, object-member reorderings,
typed invalid input, validity, extensions, collisions and bounded parsing. A
controlled listener checks that links cause no connection; a deliberate contact
omission in a disposable compiled copy must fail the exact generated-field test.
The existing required-test inventory checks discovery and actual execution.
Execution results and separate review belong in the issue and its PR.

This does not add family endpoints, XML import, external metadata resolution,
executable processes, identity administration or a conformance declaration.

## Controlling sources

- [Guide §§4.2–4.3 and 4.10](https://github.com/DGIWG-P507/glaux/blob/main/Docs/Plans/glaux-server/glaux-server-implementation-guide.md#43-sensorml-swe-common-validation-and-semantic-bindings): source/meaning/output separation, reserved fields, no request-time dereference and supplied versus verified roles.
- [SensorML 3.0, OGC 23-000](https://docs.ogc.org/is/23-000/23-000.html): §§8.2.2 and 9.1.4 define common metadata; requirements 9/10 separate extension content from SensorML semantics and process execution.
- [Pinned DescribedObject schema](../crates/glaux-standards/corpus/originals/csapi/sensorml/schemas/json/DescribedObject.json), [common definitions](../crates/glaux-standards/corpus/originals/csapi/sensorml/schemas/json/commonDefs.json), [ResponsibleParty](../crates/glaux-standards/corpus/originals/csapi/sensorml/schemas/json/ResponsibleParty.json), [ContactLink](../crates/glaux-standards/corpus/originals/csapi/sensorml/schemas/json/ContactLink.json) and [Document](../crates/glaux-standards/corpus/originals/csapi/sensorml/schemas/json/Document.json), with their fixed local time/link references: actual JSON shapes. The [corpus record](standards-corpus.md) identifies provenance and preserved originals; no upstream artifact is changed.
