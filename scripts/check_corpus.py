"""Check packaged bytes and local schema targets, without validating instances.

The manifest is the reviewed source inventory, not a signature or an upstream
verification service. References are resolved only against packaged resources.
Dynamic references are checked for their initial static target; evaluating their
dynamic scope, schema assertions, formats and fixture expectations belongs to #8.
"""

import argparse
import hashlib
import json
import os
import re
import sys
from datetime import datetime
from decimal import Decimal
from pathlib import Path
from urllib.parse import unquote, urldefrag, urljoin, urlsplit


class CorpusError(ValueError):
    """The corpus does not meet its integrity/reference packaging contract."""


def require(condition, message):
    if not condition:
        raise CorpusError(message)


def deny_socket(event, _args):
    """Audit hook used by the CLI and its offline failure-control test."""
    if event.startswith("socket."):
        raise CorpusError(f"Network access forbidden: {event}")


def _linked(path):
    return path.is_symlink() or getattr(path, "is_junction", lambda: False)()


def _relative(value):
    require(isinstance(value, str) and value, "Expected a nonempty relative path")
    require(not re.search(r'[\\:*?"<>|\x00-\x1f]', value), f"Unsafe path: {value!r}")
    parts = value.split("/")
    require(
        all(part and part not in (".", "..") and part == part.rstrip(" .") for part in parts),
        f"Unsafe relative path: {value!r}",
    )
    return parts


def _path(root, value):
    candidate = root
    for part in _relative(value):
        candidate = candidate / part
        require(not _linked(candidate), f"Symlink/junction forbidden: {value}")
    require(candidate.resolve().is_relative_to(root), f"Path escapes corpus: {value}")
    require(candidate.is_file(), f"Missing regular file: {value}")
    return candidate


def _bytes(path):
    require(path.stat().st_size <= 16 * 1024 * 1024, f"Oversize corpus file: {path}")
    return path.read_bytes()


def strict_json(path):
    """Read UTF-8 JSON, rejecting duplicate members and non-JSON constants."""
    def pairs(items):
        result = {}
        for key, value in items:
            require(key not in result, f"Duplicate JSON key {key!r} in {path}")
            result[key] = value
        return result

    def constant(value):
        raise CorpusError(f"Non-JSON constant {value} in {path}")

    try:
        return json.loads(
            _bytes(path).decode("utf-8"), object_pairs_hook=pairs,
            parse_constant=constant, parse_float=Decimal,
        )
    except (UnicodeError, json.JSONDecodeError, RecursionError, ValueError) as error:
        if isinstance(error, CorpusError):
            raise
        raise CorpusError(f"Invalid JSON in {path}: {error}") from error


def _inventory(root, directory):
    base = root / directory
    require(base.is_dir() and not _linked(base), f"Missing or linked directory: {directory}")
    found = set()
    for current, directories, files in os.walk(base, followlinks=False):
        for name in directories + files:
            path = Path(current) / name
            require(not _linked(path), f"Symlink/junction forbidden: {path}")
        for name in files:
            path = Path(current) / name
            relative = path.relative_to(root).as_posix()
            _path(root, relative)
            found.add(relative)
    return found


def _uri(value, *, fragment=False):
    require(isinstance(value, str) and value, "Expected a nonempty URI")
    require(not re.search(r"[\s\\\x00-\x1f]", value), f"Unsafe URI: {value!r}")
    require(not re.search(r"%(?![0-9a-fA-F]{2})", value), f"Malformed URI escape: {value}")
    parsed = urlsplit(value)
    require(parsed.scheme in ("http", "https") and parsed.netloc, f"URI is not HTTP(S): {value}")
    require(parsed.username is None and parsed.password is None, f"Credentials in URI: {value}")
    require(fragment or not parsed.fragment, f"Unexpected URI fragment: {value}")
    return value if fragment else urldefrag(value)[0]


def _timestamp(value):
    require(isinstance(value, str), "Missing retrieval timestamp")
    try:
        parsed = datetime.fromisoformat(value.replace("Z", "+00:00"))
    except ValueError as error:
        raise CorpusError(f"Invalid retrieval timestamp: {value}") from error
    require(parsed.tzinfo is not None, f"Retrieval timestamp lacks timezone: {value}")


# Only schema-valued keywords are traversed. A property *named* '$ref', or a
# '$ref' inside default/examples/const instance data, is not a schema reference.
_MAPS = {"$defs", "definitions", "properties", "patternProperties", "dependentSchemas"}
_SINGLE = {
    "additionalProperties", "unevaluatedProperties", "propertyNames", "contains",
    "additionalItems", "unevaluatedItems", "not", "if", "then", "else",
    "contentSchema",
}
_ARRAYS = {"allOf", "anyOf", "oneOf", "prefixItems"}


class _Registry:
    def __init__(self, documents):
        self.documents = documents
        self.resources = {}
        self.anchors = {}
        self.references = []
        self.occurrences = set()

    def _register(self, table, uri, location):
        require(uri not in table or table[uri] == location, f"Ambiguous schema URI: {uri}")
        table[uri] = location

    def add(self, path, uri):
        self._register(self.resources, uri, (path, ()))
        self._walk(self.documents[path], path, (), uri, 0)

    def _walk(self, node, path, pointer, base, depth):
        require(depth <= 256, f"Schema nesting exceeds packaging bound: {path}")
        require(isinstance(node, (dict, bool)), f"Non-schema node at {path}#{pointer}")
        if isinstance(node, bool):
            return
        location = (path, pointer)
        if "$id" in node:
            require(isinstance(node["$id"], str), f"Non-string $id in {path}")
            identified = _uri(urljoin(base, node["$id"]), fragment=True)
            resource, anchor = urldefrag(identified)
            if anchor:
                # Draft-07 permits a plain-name fragment identifier. 2020-12
                # uses $anchor; neither case is an instruction to fetch a URI.
                require(not anchor.startswith("/"), f"Pointer in $id: {identified}")
                self._register(self.anchors, identified, location)
            else:
                self._register(self.resources, resource, location)
            base = identified
        for key in ("$anchor", "$dynamicAnchor"):
            if key in node:
                anchor = node[key]
                require(
                    isinstance(anchor, str) and re.fullmatch(r"[A-Za-z_][-A-Za-z0-9._]*", anchor),
                    f"Invalid {key} in {path}",
                )
                self._register(self.anchors, urldefrag(base)[0] + "#" + anchor, location)
        for key in ("$ref", "$dynamicRef", "$schema"):
            if key in node:
                require(isinstance(node[key], str), f"Non-string {key} in {path}")
                target = _uri(urljoin(base, node[key]), fragment=True)
                self.references.append((target, f"{path}#{'/'.join(pointer)} {key}"))
                self.occurrences.add((path, pointer, key))

        def visit(value, *parts):
            self._walk(value, path, pointer + parts, base, depth + 1)

        for key in _MAPS & node.keys():
            require(isinstance(node[key], dict), f"Non-object {key} in {path}")
            for name, value in node[key].items():
                visit(value, key, name)
        for key in _SINGLE & node.keys():
            visit(node[key], key)
        for key in _ARRAYS & node.keys():
            require(isinstance(node[key], list), f"Non-array {key} in {path}")
            for index, value in enumerate(node[key]):
                visit(value, key, str(index))
        if "items" in node:
            if isinstance(node["items"], list):
                for index, value in enumerate(node["items"]):
                    visit(value, "items", str(index))
            else:
                visit(node["items"], "items")
        if "dependencies" in node:
            require(isinstance(node["dependencies"], dict), f"Non-object dependencies in {path}")
            for name, value in node["dependencies"].items():
                if not isinstance(value, list):
                    visit(value, "dependencies", name)

    def _node(self, location):
        path, pointer = location
        node = self.documents[path]
        for key in pointer:
            node = node[int(key)] if isinstance(node, list) else node[key]
        return node

    def complete_aliases(self):
        """An explicit alias identifies the same resource, including its anchors."""
        for uri, location in list(self.anchors.items()):
            resource, anchor = urldefrag(uri)
            require(resource in self.resources, f"Anchor has no packaged resource: {uri}")
            resource_location = self.resources[resource]
            for alias, target in self.resources.items():
                if target == resource_location:
                    self._register(self.anchors, alias + "#" + anchor, location)

    def resolve(self, uri):
        uri = _uri(uri, fragment=True)
        resource, fragment = urldefrag(uri)
        require(resource in self.resources, f"Unpackaged schema URI: {resource}")
        try:
            fragment = unquote(fragment, errors="strict")
        except UnicodeError as error:
            raise CorpusError(f"Invalid fragment encoding: {uri}") from error
        if fragment and not fragment.startswith("/"):
            anchor = resource + "#" + fragment
            require(anchor in self.anchors, f"Missing schema anchor: {uri}")
            node = self._node(self.anchors[anchor])
        else:
            node = self._node(self.resources[resource])
            for token in fragment.split("/")[1:]:
                require(not re.search(r"~(?![01])", token), f"Invalid JSON pointer escape: {uri}")
                key = token.replace("~1", "/").replace("~0", "~")
                if isinstance(node, dict):
                    require(key in node, f"Missing JSON pointer target: {uri}")
                    node = node[key]
                elif isinstance(node, list):
                    require(re.fullmatch(r"0|[1-9][0-9]*", key), f"Invalid pointer array index: {uri}")
                    require(len(key) < 12 and int(key) < len(node), f"Missing pointer array index: {uri}")
                    node = node[int(key)]
                else:
                    raise CorpusError(f"JSON pointer traverses scalar: {uri}")
        require(isinstance(node, (dict, bool)), f"Reference target is not a schema: {uri}")
        return node


def _check(root):
    require(root.is_dir() and not _linked(root), f"Missing or linked corpus root: {root}")
    root = root.resolve()
    manifest = strict_json(_path(root, "manifest.json"))
    require(isinstance(manifest, dict) and type(manifest.get("format_version")) is int
            and manifest["format_version"] == 1, "Unsupported corpus manifest format")
    artifacts = manifest.get("artifacts")
    require(isinstance(artifacts, list) and artifacts, "Empty/missing artifact inventory")
    by_path, identities, documents = {}, {}, {}
    aliases = 0
    for artifact in artifacts:
        require(isinstance(artifact, dict), "Artifact must be an object")
        path = artifact.get("path")
        _relative(path)
        require(path.startswith("originals/"), f"Artifact outside originals: {path}")
        require(path not in by_path, f"Duplicate artifact path: {path}")
        require(artifact.get("kind") in ("schema", "licence", "source-header"), f"Unknown artifact kind: {path}")
        payload = _bytes(_path(root, path))
        require(type(artifact.get("bytes")) is int and artifact["bytes"] == len(payload), f"Byte count mismatch: {path}")
        digest = artifact.get("sha256")
        require(isinstance(digest, str) and re.fullmatch(r"[0-9a-f]{64}", digest), f"Invalid SHA-256: {path}")
        require(hashlib.sha256(payload).hexdigest() == digest, f"SHA-256 mismatch: {path}")
        _uri(artifact.get("source_url"))
        _uri(artifact.get("effective_source_url"))
        _timestamp(artifact.get("retrieved_at"))
        require("source_revision" in artifact, f"Missing source revision/snapshot marker: {path}")
        revision = artifact["source_revision"]
        require(revision is None or (isinstance(revision, str) and re.fullmatch(r"[0-9a-f]{40}", revision)), f"Invalid source revision: {path}")
        artifact_aliases = artifact.get("aliases")
        require(isinstance(artifact_aliases, list), f"Missing alias list: {path}")
        for uri in [artifact.get("uri"), *artifact_aliases]:
            uri = _uri(uri)
            require(uri not in identities, f"Duplicate artifact URI/alias: {uri}")
            identities[uri] = path
        aliases += len(artifact_aliases)
        mirrors = artifact.get("verified_mirrors")
        require(isinstance(mirrors, list), f"Missing verified mirror list: {path}")
        for mirror in mirrors:
            require(isinstance(mirror, dict), f"Invalid mirror record: {path}")
            _uri(mirror.get("url"))
            _timestamp(mirror.get("retrieved_at"))
            require(type(mirror.get("bytes")) is int and mirror["bytes"] == len(payload)
                    and mirror.get("sha256") == digest, f"Mirror evidence disagrees: {path}")
        by_path[path] = artifact
        if artifact["kind"] == "schema":
            documents[path] = strict_json(_path(root, path))
    require(documents, "Corpus has no schemas")
    require(_inventory(root, "originals") == set(by_path), "Originals inventory differs from manifest")
    for path, artifact in by_path.items():
        licence = artifact.get("licence")
        _relative(licence)
        require(licence in by_path and by_path[licence]["kind"] == "licence", f"Missing licence artifact: {path}")
    registry = _Registry(documents)
    for uri, path in identities.items():
        if path in documents:
            registry.add(path, uri)
    registry.complete_aliases()
    for target, source in registry.references:
        try:
            registry.resolve(target)
        except CorpusError as error:
            raise CorpusError(f"{source}: {error}") from error

    fixture_index = strict_json(_path(root, "fixtures/cases.json"))
    cases = fixture_index.get("cases") if isinstance(fixture_index, dict) else None
    require(isinstance(cases, list) and cases, "Empty/missing fixture cases")
    ids, inputs = set(), {"fixtures/cases.json"}
    positive = 0
    for case in cases:
        require(isinstance(case, dict), "Fixture case must be an object")
        identity = case.get("id")
        require(isinstance(identity, str) and identity.strip() == identity and identity, "Invalid fixture ID")
        require(identity not in ids, f"Duplicate fixture ID: {identity}")
        ids.add(identity)
        expected = case.get("expected_valid")
        require(type(expected) is bool, f"Fixture expectation must be boolean: {identity}")
        positive += int(expected)
        _relative(case.get("input"))
        fixture_path = "fixtures/" + case["input"]
        require(fixture_path != "fixtures/cases.json", f"Fixture input is its index: {identity}")
        strict_json(_path(root, fixture_path))
        inputs.add(fixture_path)
        registry.resolve(case.get("schema_uri"))
        _uri(case.get("source"), fragment=True)
        require(isinstance(case.get("reason"), str) and case["reason"].strip(), f"Missing fixture reason: {identity}")
    require(_inventory(root, "fixtures") == inputs, "Fixture file inventory differs from cases")
    return {
        "artifacts": len(artifacts), "schemas": len(documents),
        "references": len(registry.occurrences), "fixtures": len(cases),
        "expectations_true": positive, "expectations_false": len(cases) - positive,
        "aliases": aliases,
    }


def check(root):
    """Return exact inventory counts or raise CorpusError; perform no network IO."""
    try:
        return _check(Path(root))
    except CorpusError:
        raise
    except (OSError, ValueError, TypeError, KeyError, RecursionError) as error:
        raise CorpusError(f"Cannot check corpus: {error}") from error


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("root", nargs="?", type=Path,
                        default=Path(__file__).resolve().parents[1] / "crates/glaux-standards/corpus")
    args = parser.parse_args()
    sys.addaudithook(deny_socket)
    try:
        counts = check(args.root)
    except CorpusError as error:
        print(f"Corpus check failed: {error}", file=sys.stderr)
        return 1
    print(json.dumps(counts, sort_keys=True))
    print("Original bytes, local reference targets and fixture inventory checked offline.")
    print("Fixture expectations are recorded, NOT executed validation or conformance results.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
