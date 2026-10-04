# SWE Time components

[Task #29](https://github.com/DGIWG-P507/glaux-server/issues/29) extends the
immutable scalar compiler with Time descriptions and independently checked
values. It adds no HTTP route, database mapping, observation-time selection,
range/nil handling or complete SWE wire codec.

## What the values mean

Calendar values use the exact Gregorian encoding URI
`http://www.opengis.net/def/uom/ISO-8601/0/Gregorian`. Complete RFC 3339 calendar
coordinates retain their lexical form, offset and every fractional digit within
the existing [exact-time budget](exact-time.md). An omitted reference frame has
the SWE UTC default, distinct from an explicitly supplied frame.
The recognized explicit UTC identifier is
`http://www.opengis.net/def/trs/BIPM/0/UTC`, as in the
[pinned UTC example](https://github.com/opengeospatial/ogcapi-connected-systems/blob/8e03b236a049849f2ccc24b4fd9fdce5ff69bed2/swecommon/schemas/json/examples/spec/time3.json);
other spellings are not silently
equated to it. In these established UTC cases, `utc_instant()` exposes the
existing exact instant. An unfamiliar frame retains its calendar coordinate,
but requesting a UTC instant returns `UnsupportedTimeConversion`. A trailing
`Z` in a GPS-declared coordinate does not override the frame declaration.

Numeric coordinates retain an exact number or explicit SWE numeric special,
along with the complete unit, frame, optional reference time and local frame.
For example, `1250.0000000000000001` milliseconds relative to a supplied origin
remains that exact offset with that origin. Neither an omitted origin nor a
numeric zero implies 1970. Even with an explicit epoch, this issue does not add
numeric-to-calendar arithmetic: `utc_instant()` fails explicitly. This avoids
inventing leap-second, calendar-month or custom-frame conversion rules. Numeric
constraints compare coordinates in their common declared unit/frame without
needing such a conversion.

`referenceTime` is interpreted in the same declared frame as the value.
`localFrame` names the frame whose origin the value locates, not a replacement
for its reference frame; identical local and effective reference identifiers
are rejected. Frame URIs, definitions and unit links are never fetched.
Typed domain structs themselves remain data, not proof of validation; obtain
checked values through `ScalarContract`.

## Validation and explicit limits

The fixed Time entry point uses the unchanged pinned Time JSON schema with
format assertions enabled. This distinguishes the calendar-string branch of
`DateTimeNumberOrSpecial` from its named-special branch. Other fixed structural
entry points retain their existing format policy; this is not a corpus rewrite.
Then local semantic checks enforce the declared representation and references.

- Calendar data values cannot be NaN or infinities. Infinite calendar constraint
  endpoints are supported, as in the standard's AllowedTimes example.
- AllowedTimes enumeration and inclusive intervals form a union. Empty
  `intervals` admits nothing by itself. Reversed or unordered bounds fail.
  Numeric significant-figure checks reuse Quantity's no-rounding rule.
- Calendar significant-figure meaning, different unresolved-calendar comparisons,
  mixed finite numeric/calendar bounds, and unknown-frame leap-second meaning
  fail explicitly as unsupported. No lexical ordering substitutes for temporal
  meaning. Unknown-frame ordinary calendar syntax is checked, not converted.
- Gregorian calendar values require the Gregorian URI and no conflicting UCUM
  code. Numeric Time code units use the bounded temporal subset documented in
  [UCUM](ucum.md), rejecting known incompatible units; unfamiliar URI-only unit
  meaning remains explicitly unresolved. A validated UCUM code is not proof that
  an accompanying arbitrary URI describes the same unit.
- The JSON schema requires full date-time even though the conceptual model also
  discusses date-only positions. This task does not widen the pinned JSON shape.
  Nil declarations and inline quality remain explicitly unsupported here until
  their owning component tasks, rather than being silently discarded.

Original source bytes (including extensions) remain separate from typed meaning.
No rounding, clock access, origin fallback, cross-frame comparison, unit
conversion, or external lookup is hidden inside a successful value check.

## Evidence and sources

Required Time tests run in the existing Rust lane. Source-derived cases cover
exact fractions and offset equivalence, numeric origin/unit binding, invalid
calendars and declarations, explicit foreign frames, special values, constraints
and deterministic calendar boundaries. Disposable compiled faults must fail
their intended behavioral assertions from passing baselines; setup errors are
not proof of detection. Actual run and separate-review evidence belongs to the
issue and its PR, not a conformance claim here.

The unchanged corpus pin is `8e03b236a049849f2ccc24b4fd9fdce5ff69bed2`:

- [Time conceptual semantics and AllowedTimes](https://github.com/opengeospatial/ogcapi-connected-systems/blob/8e03b236a049849f2ccc24b4fd9fdce5ff69bed2/swecommon/standard/sections/clause_7.2_uml_simple_components.adoc):
  frame default, explicit epoch, temporal unit and common-axis constraints.
- [Time schema](../crates/glaux-standards/corpus/originals/csapi/swecommon/schemas/json/Time.json)
  and [basic types](../crates/glaux-standards/corpus/originals/csapi/swecommon/schemas/json/basicTypes.json):
  selected JSON shapes, required metadata and AllowedTimes.
- [Published SWE Common 3.0 §§8.2.9, 8.2.18, 9.1.2 and 9.1.18](https://docs.ogc.org/is/24-014/24-014.html):
  requirements 26–28 and 56/60, including ISO-special exclusion and the
  calendar/infinite-bound example.

These bounded library checks do not complete a SWE conformance class.
