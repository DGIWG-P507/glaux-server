# Canonical System retrieval over HTTP

[Issue #25](https://github.com/DGIWG-P507/glaux-server/issues/25) makes the
canonical URL returned by [minimal System creation](system-create.md) readable:
`GET /systems/{id}` and `HEAD /systems/{id}`. This is one GeoJSON retrieval slice
for Systems created through that POST. It is not complete System CRUD, a
collection, a SensorML representation or an OGC conformance-class claim.

## Enabling and access

There is no new setting. The same `system_creation` object that installs
`POST /systems` also installs the canonical item route, behind the same
authentication layer. Omitting it removes both routes and their API description.
The [configuration example](system-create.md#enable-the-bounded-example) already
grants `read` as well as `create`.

A caller needs the `read` action for the System's creating source, and for that
System if the grant lists `resources`. The source is the immutable owner recorded
by the accepted creation, not a request header or submitted field. Authentication
alone grants nothing. A caller with only `create` cannot read.

The serving role needs no permission beyond those already listed for creation:
it reads the identity, label, parent, write-head, artifact, audit and
outgoing-work tables. Ordinary reads write nothing, including denied reads; the
Guide requires durable audit for mutations, not for each denied read.

## Root-to-resource walk

Start with only the configured public root (here `https://api.example.test`):

1. `GET /` returns the landing page. Follow its `service-desc` link.
2. `GET /api` returns the OpenAPI description. `servers[0].url` is the public
   root; `paths["/systems"]` has `post` and `paths["/systems/{id}"]` has `get`
   and `head`.
3. `POST /systems` with the [minimal Feature](system-create.md#request-and-success-contract)
   returns `201` with `Location: https://api.example.test/systems/{id}`.
4. `GET` that Location returns the System.

The landing page does not link to `/systems`: there is no browsable collection
yet, and `GET /systems` returns `405`. A client learns member URLs from Location.

## Representation

```http
GET /systems/0190f5c2-7b5a-7cc3-98c4-dc0c0c220001 HTTP/1.1
Accept: application/geo+json
```

```http
HTTP/1.1 200 OK
Content-Type: application/geo+json
Cache-Control: private, no-store
Vary: Accept
```

```json
{
  "type": "Feature",
  "id": "0190f5c2-7b5a-7cc3-98c4-dc0c0c220001",
  "geometry": null,
  "properties": {
    "uid": "urn:glaux:example:thermometer",
    "name": "Example thermometer",
    "featureType": "sosa:Sensor"
  },
  "links": [{
    "href": "https://api.example.test/systems/0190f5c2-7b5a-7cc3-98c4-dc0c0c220001",
    "rel": "self",
    "type": "application/geo+json",
    "title": "This System"
  }]
}
```

- `id` is the server-generated local identifier, the last segment of Location.
  A client-supplied `id` or `links` in the original POST is never echoed.
- `uid` and `name` come from the stored identity and label.
- `featureType` is the exact spelling retained in the current accepted source,
  so `sosa:Sensor` and `http://www.w3.org/ns/sosa/Sensor` stay distinct.
- `links` has one `self` link to the canonical URL. A System with a parent that
  the caller can see also gets an `ogc-rel:parentSystem` link (Guide §4.1.1).
  The public write path cannot create parents yet, and a child with a hidden
  parent is concealed entirely rather than shown with a missing link.

One database statement authorizes the ID and reads the identity, label, parent
and authoritative write-head artifact together. A later update commits the label
and head together, so a read cannot mix two revisions. Before sending, the
server checks the body against the pinned CSAPI GeoJSON System response
projection; a stored value it cannot represent gives `500`, not a partial body.

## Negotiation, caching and validators

The only representation is `application/geo+json`, also the default when
`Accept` is absent. Other acceptable types, such as `application/json` or
`application/sml+json` alone, give `406`. Negotiation happens before any lookup,
so it reveals nothing about which IDs exist. `HEAD` returns the `GET` headers,
including its `Content-Length`, without a body.

Every authenticated response, success or problem, carries
`Cache-Control: private, no-store` (Guide §6.2). Unauthenticated `401` responses
keep the existing `no-store`. No `ETag` or `Last-Modified` is emitted and
conditional GET is not supported in this increment; representation validators
belong to Roadmap task 2.4.9.

## Missing and concealed Systems

These all return the same `404` Problem Details, identical apart from its fresh
correlation identifier:

- an ID that no stored System has;
- a System from a source the caller cannot read, outside a resource-scoped
  grant, or with only the `create` action;
- a non-canonical spelling, such as upper-case hex, or a value that is not a
  local identifier at all.

The body carries no identifier, UID, name, source or reason. Missing or invalid
credentials give `401` for existing and missing IDs alike.

## Not included

Collections and list queries, SensorML and other representations, conditional
requests, other mutation methods and backup/restore (#26) remain later tasks.

Controlling sources are [Guide §§4.1–4.3, 4.10, 6.2 and 8.2][Guide],
[CSAPI Part 1][Part1], [RFC 9110 §12 (content negotiation)][Negotiation] and
[RFC 9111 §5.2.2 (cache directives)][Cache]. The
[independent actual-binary proof](system-read-tests.md) records the expected
answers, restart, access cases and fault controls. Execution results and separate
review belong in issue #25 and its PR.

[Guide]: https://github.com/DGIWG-P507/glaux/blob/8801afa2a52a617b511e7418206c915b9da78014/Docs/Plans/glaux-server/glaux-server-implementation-guide.md
[Part1]: https://docs.ogc.org/is/23-001/23-001.html
[Negotiation]: https://www.rfc-editor.org/rfc/rfc9110.html#section-12
[Cache]: https://www.rfc-editor.org/rfc/rfc9111.html#section-5.2.2
