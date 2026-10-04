"""Temporary Step 5 comparison: authored expectations, never peer model types.

This is a selected-field diagnostic, not schema/conformance certification.
Sources and authorized cases: planning Phase-1/evidence/07 proposal, CSAPI
23-001 reqs 5/60/77-82, selected Features Part 4 post-response A/B,
RFC 9110 9.3.2/8.6, and docs/system-create.md and system-read.md.
"""

import copy
import json
import re
from urllib.parse import unquote, urljoin, urlsplit
from uuid import UUID


GEO = "application/geo+json"
FIXTURES = {
    "a": b'{"type":"Feature","geometry":null,"properties":{"uid":"urn:glaux:review:phase1:step5:a","name":"Phase 1 comparison A","featureType":"sosa:Sensor"}}',
    "b": b'{"type":"Feature","geometry":null,"properties":{"uid":"urn:glaux:review:phase1:step5:b","name":"Phase 1 comparison B","featureType":"http://www.w3.org/ns/sosa/Platform"}}',
}
TYPE_EQUIVALENTS = {
    "sosa:Sensor": {"sosa:Sensor", "http://www.w3.org/ns/sosa/Sensor"},
    "http://www.w3.org/ns/sosa/Platform": {"sosa:Platform", "http://www.w3.org/ns/sosa/Platform"},
}
GROUPS = ["discover", "create-read-a", "distinguish-b", "default-head",
          "absent-invalid", "invalid-bodies", "format-boundary", "restart"]


class Mismatch(Exception):
    """Actual complete response disagrees with an authored expectation."""


class Unrun(Exception):
    """A dependency did not produce a usable resource address."""


def require(condition, message):
    if not condition:
        raise Mismatch(message)


def one(response, name, required=True):
    values = response["headers"].get(name.lower(), [])
    require(len(values) == 1 or (not required and not values), f"{name}: expected one header, got {values!r}")
    return values[0] if values else None


def safe_url(base, href, reference=None):
    require(isinstance(href, str) and href and not any(ord(c) <= 32 or ord(c) == 127 for c in href), "invalid link text")
    require("\\" not in href, "backslash link")
    raw = urlsplit(href)
    decoded = unquote(raw.path)
    require(not any(p in (".", "..") for p in decoded.split("/")), "traversal link")
    require("%" not in decoded and "\\" not in decoded, "nested encoding/backslash link")
    result = urljoin(reference or base.rstrip("/") + "/", href)
    root, target = urlsplit(base), urlsplit(result)
    require(target.scheme == root.scheme and target.hostname == root.hostname and target.port == root.port,
            "link leaves approved origin")
    require(target.username is None and target.password is None and not target.fragment, "link user-info/fragment")
    prefix = root.path.rstrip("/")
    require(target.path == prefix or target.path.startswith(prefix + "/"), "link leaves approved API path")
    return result


def location(base, response, peer):
    require(response["status"] == 201, f"creation expected 201, got {response['status']}")
    value = safe_url(base, one(response, "location"), base.rstrip("/") + "/systems")
    parts = urlsplit(value)
    prefix = urlsplit(base).path.rstrip("/") + "/systems/"
    require(parts.path.startswith(prefix) and not parts.query, "Location not canonical System item")
    identity = parts.path[len(prefix):]
    require(bool(re.fullmatch(r"[A-Za-z0-9_-]+", identity)), "Location lacks one safe ID segment")
    if peer == "glaux":
        try:
            parsed = UUID(identity)
        except ValueError as error:
            raise Mismatch("Glaux Location ID is not a UUID") from error
        require(parsed.version == 7 and str(parsed) == identity, "Glaux Location not canonical UUIDv7")
        require(response["body"] == b"", "Glaux creation body must be empty")
        protected(response)
    return value, identity


def decode(response):
    def unique(pairs):
        result = {}
        for key, value in pairs:
            require(key not in result, f"duplicate JSON member {key}")
            result[key] = value
        return result
    def invalid(value):
        raise Mismatch(f"non-JSON numeric constant {value}")
    return json.loads(response["body"].decode("utf-8"), object_pairs_hook=unique, parse_constant=invalid)


def protected(response):
    directives = {x.strip().lower() for v in response["headers"].get("cache-control", []) for x in v.split(",")}
    require({"private", "no-store"} <= directives, "Glaux protected response missing private, no-store")
    require("etag" not in response["headers"] and "last-modified" not in response["headers"], "Glaux initial slice unexpectedly emits validator")


def feature(response, base, address, identity, fixture, peer):
    require(response["status"] == 200, f"item GET expected 200, got {response['status']}")
    require(one(response, "content-type").split(";")[0].strip().lower() == GEO, "item is not GeoJSON")
    body, expected = decode(response), json.loads(FIXTURES[fixture])
    require(isinstance(body, dict), "Feature is not object")
    require(body.get("type") == "Feature", "wrong Feature type")
    require(body.get("id") == identity, "wrong or missing generated ID")
    require("geometry" in body and body["geometry"] is None, "changed/missing geometry")
    props = body.get("properties")
    require(isinstance(props, dict), "missing properties")
    for key in ("uid", "name"):
        require(props.get(key) == expected["properties"][key], f"changed/missing {key}")
    expected_type = expected["properties"]["featureType"]
    actual_type = props.get("featureType")
    allowed = {expected_type} if peer == "glaux" else TYPE_EQUIVALENTS[expected_type]
    require(isinstance(actual_type, str) and actual_type in allowed, "changed/missing featureType")
    links = body.get("links")
    require(isinstance(links, list), "missing links")
    selves = [x for x in links if isinstance(x, dict) and x.get("rel") == "self"]
    require(bool(selves), "missing self link")
    for link in selves:
        require(safe_url(base, link.get("href"), address) == address, "self link does not identify created System")
    if peer == "glaux":
        protected(response)
        require("accept" in {x.strip().lower() for v in response["headers"].get("vary", []) for x in v.split(",")}, "Glaux Vary omits Accept")
        require(len(links) == 1 and len(selves) == 1, "Glaux parentless slice has unexpected association")
    return {"id": identity, "uid": props["uid"], "name": props["name"],
            "submitted_type": expected_type, "returned_type": actual_type,
            "type_spelling_equivalent": actual_type != expected_type}


def head_check(response, get_response):
    require(response["status"] == 200, f"supported HEAD expected 200, got {response['status']}")
    require(response["body"] == b"", "HEAD has an illegal body")
    length = one(response, "content-length", required=False)
    if length is not None:
        require(length.isdigit() and int(length) == len(get_response["body"]), "HEAD Content-Length differs from selected GET bytes")


def controls():
    """Use only independently authored canned wire observations, not server output."""
    base = "http://127.0.0.1:18888/sensorhub/api"
    address = base + "/systems/control-id"
    document = json.loads(FIXTURES["a"])
    document.update(id="control-id", links=[{"rel": "self", "href": address}])
    good = {"status": 200, "headers": {"content-type": [GEO]}, "body": json.dumps(document).encode()}
    feature(good, base, address, "control-id", "a", "osh")
    outcomes = [{"control": "independent-good", "result": "accepted"}]
    changes = {
        "wrong-id": lambda d: d.update(id="wrong"),
        "missing-id": lambda d: d.pop("id"),
        "changed-uid": lambda d: d["properties"].update(uid="urn:changed"),
        "changed-name": lambda d: d["properties"].update(name="wrong"),
        "changed-type": lambda d: d["properties"].update(featureType="sosa:Actuator"),
        "changed-geometry": lambda d: d.update(geometry={"type": "Point", "coordinates": [0, 0]}),
        "unsafe-self": lambda d: d["links"][0].update(href="http://example.com/systems/control-id"),
    }
    def rejects(name, operation):
        try:
            operation()
        except Mismatch as error:
            outcomes.append({"control": name, "result": "rejected", "reason": str(error)})
        else:
            raise Mismatch(f"checker false green: {name}")
    for name, mutate in changes.items():
        bad = copy.deepcopy(document)
        mutate(bad)
        response = dict(good, body=json.dumps(bad).encode())
        rejects(name, lambda: feature(response, base, address, "control-id", "a", "osh"))
    rejects("a-returned-for-b", lambda: feature(good, base, address, "control-id", "b", "osh"))
    for name, href in (("external-location", "http://example.com/systems/id"),
                       ("traversal-location", "../systems/id"),
                       ("prefix-escape", "http://127.0.0.1:18888/sensorhub/api-other/systems/id"),
                       ("userinfo", "http://u@127.0.0.1:18888/sensorhub/api/systems/id")):
        rejects(name, lambda: location(base, {"status": 201, "headers": {"location": [href]}, "body": b""}, "osh"))
    valid_head = {"status": 200, "headers": {"content-length": [str(len(good["body"]))]}, "body": b""}
    head_check(valid_head, good)
    outcomes.append({"control": "good-head", "result": "accepted"})
    rejects("head-body", lambda: head_check(dict(valid_head, body=b"x"), good))
    rejects("head-length", lambda: head_check(dict(valid_head, headers={"content-length": ["1"]}), good))
    extra = copy.deepcopy(document)
    extra["peerMember"] = True
    extra["links"].append({"rel": "alternate", "href": address + "?f=json"})
    feature(dict(good, body=json.dumps(extra).encode()), base, address, "control-id", "a", "osh")
    outcomes.append({"control": "additional-peer-link-member", "result": "accepted"})
    extra["properties"]["featureType"] = "http://www.w3.org/ns/sosa/Sensor"
    feature(dict(good, body=json.dumps(extra).encode()), base, address, "control-id", "a", "osh")
    outcomes.append({"control": "explicit-uri-curie-equivalence", "result": "accepted"})
    return outcomes


def run(runtime, evidence_dir):
    rows = []
    resources = {peer: {} for peer in runtime.bases}

    def save():
        (evidence_dir / "case-results.json").write_text(json.dumps(rows, indent=2) + "\n", encoding="utf-8")

    def case(peer, group, name, operation):
        row = {"peer": peer, "group": group, "case": name, "status": "started"}
        rows.append(row)
        save()
        try:
            row["observation"] = operation()
            row["status"] = "accounted"
        except Unrun as error:
            row.update(status="unrun", reason=str(error))
        except Mismatch as error:
            row.update(status="behavior-difference-unresolved", reason=str(error))
        except Exception as error:
            row.update(status="execution-failed", reason=f"{type(error).__name__}: {error}")
        save()

    def request(peer, method, target, headers=None, body=b""):
        result = runtime.request(peer, method, target, headers=headers, body=body)
        # Runtime preserves original wire bytes; this index also survives an assertion failure.
        rows[-1].setdefault("responses", []).append({"status": result["status"], "headers": result["headers"], "raw_path": result["raw_path"], "body_bytes": len(result["body"])})
        save()
        return result

    def item(peer, key):
        if key not in resources[peer]:
            raise Unrun(f"System {key} has no usable successful-creation Location")
        return resources[peer][key]

    def get_item(peer, key, accept=True):
        address, identity = item(peer, key)
        response = request(peer, "GET", address, {"Accept": GEO} if accept else None)
        return feature(response, runtime.bases[peer], address, identity, key, peer)

    def discover(peer):
        base = runtime.bases[peer]
        response = request(peer, "GET", base + "/", {"Accept": "application/json"})
        require(response["status"] == 200, "landing GET not 200")
        document = decode(response)
        require(isinstance(document, dict) and isinstance(document.get("links"), list), "landing links absent")
        notes = {"links": document["links"], "navigation": []}
        for label, relations in (("description", {"service-desc"}),
                                 ("conformance", {"http://www.opengis.net/def/rel/ogc/1.0/conformance"})):
            matches = [x for x in document["links"] if isinstance(x, dict) and x.get("rel") in relations]
            if not matches:
                require(peer != "glaux", f"Glaux lacks {label} link")
                notes["navigation"].append({"target": label, "outcome": "not-advertised; fixed systems route used, not navigation success"})
                continue
            link = matches[0]
            try:
                target = safe_url(base, link.get("href"), base + "/")
            except Mismatch:
                require(peer != "glaux", f"Glaux {label} leaves approved origin/path")
                notes["navigation"].append({"target": label, "outcome": "advertised destination not fetched outside approved boundary", "href": link.get("href")})
                continue
            actual = request(peer, "GET", target, {"Accept": "application/json"})
            require(actual["status"] == 200, f"advertised {label} not 200")
            content = decode(actual)
            notes["navigation"].append({"target": label, "url": target, "document": content})
            if label == "description" and peer == "glaux":
                require(isinstance(content, dict) and isinstance(content.get("paths"), dict), "Glaux API paths absent")
                paths = content["paths"]
                require("post" in paths.get("/systems", {}) and "get" not in paths.get("/systems", {}), "Glaux description misstates collection scope")
                require({"get", "head"} <= set(paths.get("/systems/{id}", {})), "Glaux description misses item GET/HEAD")
            if label == "conformance":
                require(isinstance(content, dict) and isinstance(content.get("conformsTo"), list), "conformance declarations absent")
                if peer == "glaux":
                    require(content["conformsTo"] == [], "Glaux initial slice claims conformance classes")
        return notes

    def create(peer, key):
        base = runtime.bases[peer]
        response = request(peer, "POST", base + "/systems", {"Content-Type": GEO}, FIXTURES[key])
        address, identity = location(base, response, peer)
        resources[peer][key] = (address, identity)
        if key == "b" and "a" in resources[peer]:
            require(identity != resources[peer]["a"][1], "B reused A identity")
        return {"location": address, "id": identity}

    def head(peer):
        address, identity = item(peer, "a")
        selected = request(peer, "GET", address, {"Accept": GEO})
        feature(selected, runtime.bases[peer], address, identity, "a", peer)
        response = request(peer, "HEAD", address, {"Accept": GEO})
        if peer == "osh" and response["status"] in (405, 501):
            require(response["body"] == b"", "unsupported HEAD has illegal body")
            return {"outcome": "HEAD unsupported by peer; not Glaux parity obligation"}
        head_check(response, selected)
        if peer == "glaux":
            protected(response)
        return {"outcome": "supported HEAD has no body and any declared length matches explicit GeoJSON GET"}

    def default(peer):
        if peer == "glaux":
            return get_item(peer, "a", accept=False)
        address, _ = item(peer, "a")
        response = request(peer, "GET", address)
        require(response["status"] == 200, "peer default GET not 200")
        media = one(response, "content-type")
        if media.split(";")[0].strip().lower() == GEO:
            feature(response, runtime.bases[peer], address, item(peer, "a")[1], "a", peer)
        return {"outcome": "peer default recorded; only GeoJSON receives selected-field check", "media": media}

    def negative(peer, name, method, target, headers, body, glaux_status, peer_statuses):
        if name == "valid-absent":
            absent = runtime.missing_ids[peer]
            require(all(saved[1] != absent for saved in resources[peer].values()), "selected absent ID was allocated in this fixture")
        response = request(peer, method, target, headers, body)
        allowed = {glaux_status} if peer == "glaux" else peer_statuses
        require(response["status"] in allowed, f"{name}: expected {sorted(allowed)}, got {response['status']}")
        if peer == "glaux":
            protected(response)
            require(one(response, "content-type").split(";")[0].strip().lower() == "application/problem+json", "Glaux rejection not Problem Details")
            problem = decode(response)
            require(isinstance(problem, dict) and problem.get("status") == glaux_status, "Glaux problem status disagrees")
        return {"outcome": "rejected", "status": response["status"], "expected_statuses": sorted(allowed)}

    for peer in runtime.bases:
        base = runtime.bases[peer]
        case(peer, 1, "root-description-conformance", lambda p=peer: discover(p))
        case(peer, 2, "create-a", lambda p=peer: create(p, "a"))
        case(peer, 2, "read-a", lambda p=peer: get_item(p, "a"))
        case(peer, 3, "create-b", lambda p=peer: create(p, "b"))
        case(peer, 3, "read-b", lambda p=peer: get_item(p, "b"))
        case(peer, 3, "a-not-replaced", lambda p=peer: get_item(p, "a"))
        case(peer, 4, "default", lambda p=peer: default(p))
        case(peer, 4, "head", lambda p=peer: head(p))
        for name, identity, statuses in (("valid-absent", runtime.missing_ids[peer], {404}), ("invalid-id", "!invalid!", {400, 404})):
            case(peer, 5, name, lambda p=peer, n=name, i=identity, s=statuses: negative(p, n, "GET", runtime.bases[p] + "/systems/" + i, {"Accept": GEO}, b"", 404, s))
        missing = json.loads(FIXTURES["a"])
        del missing["properties"]["uid"]
        for name, payload in (("malformed-json", b"{"), ("missing-uid", json.dumps(missing, separators=(",", ":")).encode())):
            case(peer, 6, name, lambda p=peer, n=name, b=payload: negative(p, n, "POST", runtime.bases[p] + "/systems", {"Content-Type": GEO}, b, 400, {400, 422}))
        for key in ("a", "b"):
            case(peer, 6, "unchanged-" + key, lambda p=peer, k=key: get_item(p, k))
        case(peer, 7, "plain-text-post", lambda p=peer: negative(p, "plain text", "POST", runtime.bases[p] + "/systems", {"Content-Type": "text/plain"}, b"not a System", 415, {400, 415}))
        def sensor(p=peer):
            address, _ = item(p, "a")
            if p == "glaux":
                return negative(p, "SensorML Accept", "GET", address, {"Accept": "application/sml+json"}, b"", 406, {406})
            response = request(p, "GET", address, {"Accept": "application/sml+json"})
            require(response["status"] in (200, 406), "unexpected peer SensorML status")
            if response["status"] == 200:
                require(one(response, "content-type").split(";")[0].strip().lower() == "application/sml+json", "peer ignored SensorML-only Accept")
                require(isinstance(decode(response), dict), "SensorML response not JSON object")
            return {"outcome": "peer richer-format boundary observed; not SensorML semantic validation", "status": response["status"]}
        case(peer, 7, "sensorml-get", sensor)

    for peer in runtime.bases:
        def restart(p=peer):
            runtime.restart(p)
            return {"outcome": "ordinary process restart completed; same configured datastore"}
        case(peer, 8, "restart-process", restart)
        for key in ("a", "b"):
            case(peer, 8, "retained-" + key, lambda p=peer, k=key: get_item(p, k))
    return rows
