# SWE Geometry components

[Task #34](https://github.com/DGIWG-P507/glaux-server/issues/34) adds a distinct
Geometry component to the shared SWE model. To describe a measured position or
area, Glaux keeps its semantic definition, declared spatial reference system
(`srs`), allowed geometry types and exact coordinates together. It does not turn
that component into a CSAPI feature, swap coordinate axes or transform a CRS.

## Descriptions and values

`glaux_standards::geometry::GeometryContract` compiles source bytes with the
existing offline validator. The immutable contract retains the original source
and exposes a separate typed description. Point, MultiPoint, LineString,
MultiLineString, Polygon and MultiPolygon are distinct value variants; a
GeometryCollection, Feature, scalar or JSON null is not a SWE Geometry value.
An absent inline value remains absent, not a null geometry or invented origin.

Coordinates use the existing exact-number representation, without a binary64
intermediate. Positions retain two or three ordinates, including supplied height
and original numeric spelling. Nested coordinate and ring order is preserved.
Checks cover shape, dimensional consistency, closed rings and applicable type
constraints; they do not claim a topology engine, polygon repair or winding
normalization. Supported empty outer coordinate arrays remain empty, not null.
The original schema's Point and LineString minimum sizes still apply.

The constraint's absence, an empty constraint object and an explicit empty
`geomTypes` list remain distinct. An explicit list restricts accepted geometry
kinds; an empty list admits none. The original schema uses `minLength` on this
array; Glaux does not silently rewrite it into a nonempty-list requirement.
The conceptual allowed-type list also permits zero entries.

Textual `nilValues` declarations retain their sentinel and reason metadata.
They do not make strings or null into geometry values: that mapping belongs to
the later payload encodings. A declared nil is not a supplied geometry.

## Reference systems and bounded interpretation

The declared `srs` URI is mandatory and retained exactly. Known local bindings
check two-dimensional CRS84/EPSG:4326 and three-dimensional CRS84h/EPSG:4979
declarations against coordinate dimensions. Other well-formed identifiers carry
an explicit unresolved result. URI syntax and bounded coordinate checks do not
establish that an arbitrary reference system exists or has been resolved.
No request fetches a CRS definition or transformation parameters.

SWE's explicit `srs` and GeoJSON-shaped value are kept together. This compiler
does not rewrite them into a standalone RFC 7946/WGS84 geometry, infer axis
order from numbers, or claim that a source in an unfamiliar CRS has become
CRS84. Spatial queries, transformations and feature representations remain
separate work. Bounding-box metadata retains its own exact numbers and must
match the checked dimensional shape; it is not recomputed from coordinates.
Bounding-box containment, antimeridian interpretation and full topology checks
are not established by this component compiler.

The existing byte, depth, node, member, string and numeric budgets apply before
semantic traversal. Permitted extension data remains in the retained source;
extension objects do not become component/reference targets. Geometry integrates
with named records, choices and array element descriptions, while numeric-only
Vectors and Matrices continue to reject it. An array element remains a descriptor
without inline payload values.
Component quality metadata remains explicitly unsupported until its owning task.
At the geometry value root, RFC 7946 §7.1's reserved Feature/FeatureCollection
members (`geometry`, `properties`, `features`) are rejected. The same names
inside an ordinary foreign-member object remain extension data, not GeoJSON.

## Sources and verification

Controlling sources are Guide §§4.3/8.1.1, the original packaged
[SWE Geometry schema](https://github.com/opengeospatial/ogcapi-connected-systems/blob/8e03b236a049849f2ccc24b4fd9fdce5ff69bed2/swecommon/schemas/json/Geometry.json),
and [SWE 3.0](https://docs.ogc.org/is/24-014/24-014.html) §§8.6.1/9.5.1 and
the relevant Geometry value rules in §10.2.6. The original corpus is unchanged.
[RFC 7946](https://www.rfc-editor.org/rfc/rfc7946.html) §§3.1/4/5 establish
the JSON geometry shape, coordinate/reference distinctions and bounding-box
rules. Its ring parser guidance does not require rejection solely for winding.
The local dimension bindings are also described in
[OGC JSON-FG](https://docs.ogc.org/is/21-045r1/21-045r1.html) for CRS84/CRS84h
and [the EPSG maintainer's description](https://www.iogp.org/blog/epsg/upgrade-of-epsg-dataset-data-model/)
for EPSG:4326/4979; these citations supply reference facts, not additional
protocols or conformance claims.

Independent fixtures check exact height, precision, type/shape, reference
metadata, constraints, malformed coordinates, nesting, limits and source
retention. Bounded generated cases exercise dimensional boundaries. A disposable
compiled fault must lose height and fail the independently expected assertion
after a passing baseline. Required discovery/execution stays in the existing CI
lanes. Actual runs, earlier failures and separate review are recorded in the
issue and PR, not inferred from test names.

No WKT/WKB codec, complete result-payload codec, spatial database query,
coordinate transformation or complete component-reference resolver is added.
