# Initial standards schema corpus

[Task 1.2.1 / issue #7](https://github.com/DGIWG-P507/glaux-server/issues/7) packages the original schema sources and independently authored expectations needed for the first offline validation work. The corpus is in [crates/glaux-standards/corpus](../crates/glaux-standards/corpus/). It contains 138 original artifacts: 129 JSON schemas, four standards source headers, and five licence or source-notice files. The 23 fixture cases are separate project material.

This is a schema-focused initial package, not a complete mirror of standards prose, OpenAPI YAML, examples or every external draft in the approved plan. The four AsciiDoc source headers retain publication/version context and their original include directives; their complete document trees are not packaged or claimed to build offline. Later tasks that consume additional API descriptions, transaction drafts, experimental parts or other standards must package their selected original artifacts and required dependencies. This package does not select a validator, implement codecs or establish service conformance.

## Sources, pins and licences

The [manifest](../crates/glaux-standards/corpus/manifest.json) records each original artifact's source URL, retrieval time, byte count, SHA-256 digest, source revision where applicable, local path, licence path and any independently compared mirror. The initial acquisition occurred on 22 September 2026 UTC (21 September local). Original bytes are preserved, including line endings; `.gitattributes` prevents Git text conversion in `originals/`. A digest identifies the bytes, while a revision identifies a source commit only where that correspondence is established.

| Material | Packaged source and provenance |
|---|---|
| CSAPI Parts 1/2, SensorML 3.0 and SWE Common 3.0 | Publication source `8e03b236a049849f2ccc24b4fd9fdce5ff69bed2`: 69 API schemas, 20 SensorML schemas, 23 SWE schemas and four common dependencies. The same archive supplies four source headers and its [OGC licence](../crates/glaux-standards/corpus/originals/csapi/LICENSE). The manifest also records the archive URL and digest. |
| SWE registry correspondence | All 23 original SWE schemas were independently fetched from `https://schemas.opengis.net/sweCommon/3.0/json/` and compared byte-for-byte by digest with the publication source. Each verified registry URL is an explicit alias for those identical local bytes. The five earlier action-list digests remain consistent; they were not treated as the complete corpus. |
| GeoJSON | Feature, FeatureCollection, Geometry and Point schemas from publication commit `f2bc6e8f1e8e1ba376d901914ecf8d0fed947d6e`, each independently compared with its `https://geojson.org/schema/` endpoint. The [MIT licence](../crates/glaux-standards/corpus/originals/licences/geojson-MIT.md) comes from source commit `660d67d1d44d168aa2bba3931fb23618b058d14b`. |
| JSON Schema metaschemas | Eight resources from the canonical draft 2020-12 endpoints (the main schema and seven referenced vocabulary metaschemas), plus the canonical draft-07 schema required by GeoJSON. All nine are snapshots pinned by retrieval time and digest, with `source_revision: null`. The draft-07 HTTP identifier and HTTPS retrieval address are explicitly mapped to the same packaged file. |

Some hosted JSON Schema meta resources differ from their historical draft-tag files. An attempted comparison rejected a mismatch before import. The final package obtains all nine resources directly from the canonical endpoints, rather than assigning the historical commits to different bytes. Future upstream differences must fail verification and receive explicit review; they are not an instruction to refresh the saved corpus.

JSON Schema's historical [2020-12 README](../crates/glaux-standards/corpus/originals/licences/json-schema-2020-12-README.md) at `add836e705c9a07434c467b6b90946ba45258a73` and [draft-07 README](../crates/glaux-standards/corpus/originals/licences/json-schema-draft-07-README.md) at `1afc34b65ead445ff363cfc870a28de0ca56e20f` preserve the AFL-or-BSD source grant. The complete [BSD/AFL licence text](../crates/glaux-standards/corpus/originals/licences/json-schema-LICENSE) is preserved separately from notice commit `4f56a9900674b27804f0ec32e3b7fdfa4efad695`. That is a notice pin, not a claimed revision for the hosted metaschema snapshots. The project's Apache-2.0 licence does not relicense any of these third-party originals.

SensorML uses the original relative references within the complete publication source tree. The separately inspected SensorML registry layout references `common/timeInstantOrNow.json`, which returned HTTP 404. The publication tree includes its own required `common/timeInstantOrNow.json`. No SensorML registry alias is asserted, no missing registry file is manufactured, and no upstream schema is edited to conceal that distinction.

## Offline inspection and explicit source verification

The manifest defines the fixed URI-to-file allowlist. Relative references use the original source base and must reach a packaged schema; fragments must identify a local target. A URI outside this mapping must fail instead of falling back to the network or an arbitrary filesystem path. URI and path spelling remain significant. This lookup is corpus inspection infrastructure. Task #8 now supplies the separate [bounded structural validator](structural-validation.md); no HTTP request handler exists yet.

Run the packaging checks from the repository root in the supported Python environment:

```sh
python3 scripts/check_corpus.py
python3 scripts/test_corpus.py
```

These commands inspect saved artifacts and exercise the packaging checks offline. They check the local evidence and its failure sensitivity; they do not fetch upstream sources or execute the 23 expected schema-validation outcomes. Their actual execution results belong in the issue/PR delivery record, not in an assumed claim that authored fixtures have passed.

For a deliberate network comparison against the original sources and recorded mirrors, run:

```powershell
pwsh -File scripts/vendor-corpus.ps1 -Mode Verify
```

`Verify` downloads the fixed source selection, checks its correspondence with the saved manifest and local bytes, and makes no local edits. It is distinct from routine offline inspection and is not a runtime reference resolver. The script's `Import` mode is initial acquisition only: it refuses to replace an existing manifest or originals directory. Neither command installs software. A changed upstream byte, unavailable source or inconsistent mapping must be investigated without weakening the saved expectations.

## Fixture expectations and known source distinctions

[fixtures/cases.json](../crates/glaux-standards/corpus/fixtures/cases.json) specifies 23 cases with input filenames, fixed schema targets, expected structural outcomes, controlling sources and independent reasons. The inputs were authored from the schemas and the approved [Guide §4.3 amendment](https://github.com/DGIWG-P507/glaux/blob/c49889a378eeb7dbd2ca3283849fa0244fd9710c/Docs/Plans/glaux-server/glaux-server-implementation-guide.md#43-sensorml-swe-common-validation-and-semantic-bindings), not generated by a server or validator. Seven outcomes are expected valid and 16 invalid. Task #8 now executes these exact expectations using the selected validator; [PR #316](https://github.com/DGIWG-P507/glaux-server/pull/316) records the hosted outcomes. Parsing fixture JSON alone is still not evidence of validation.

- The same otherwise valid BinaryEncoding descriptor is expected to fail the original `encodings.json` root, which omits BinaryEncoding, and pass the named `#/$defs/BinaryEncoding` definition. A complete binary observation-schema wrapper is expected to pass through its prescribed nested reference. The root result remains a source-artifact diagnostic. It is not the product's binary rejection policy.
- Missing byte order, an unsupported byte-order value and empty members have separate negatives. A valid TextEncoding inside a binary wrapper and a binary wrapper missing `recordSchema` must fail. These distinguish descriptor checking from checking the complete applicable wrapper. JSONEncoding, TextEncoding and BinaryEncoding remain fixed named entry points under the Guide; clients do not select arbitrary validation URIs.
- A labelled Quantity is paired with missing, empty, null and numeric-label variants. All five are repeated inside a complete SWE JSON observation-schema wrapper. The conceptual model's optional-label wording is retained beside JSON's required nonempty string label; no default is synthesized and no label requirement is inferred for unrelated components or individual encoded measurements.
- Recursive SWE records/arrays and recursive SensorML components/outputs have positive inputs and deep negative variants. Their finite shapes exercise recursive schema references. Task #8 adds input-depth limits and non-progressing-cycle rejection; complete component semantics remain later work.
- A binary wrapper referencing a nonexistent component is expected to pass the structural schema because `ref` is only constrained as a string there. Its recorded later semantic outcome is rejection by the component/layout compiler in #140. Structural acceptance is not product acceptance or proof of a working codec.

`originals/` contains unchanged third-party artifacts; `fixtures/` contains original Glaux examples and expectations. No project-adapted schema is included in this initial package. Any future projection or adaptation must remain visibly separate, preserve the original, and record its source, reason and affected checks. Corpus packaging establishes trustworthy inputs and inspectable expectations; task #8's separate execution establishes the bounded structural results. Neither establishes supported wire codecs or CSAPI conformance.
