//! Bounded preflight of the embedded schema catalog, before validator compilation.
//!
//! This is deliberately not a validator for client-supplied JSON Schemas. It
//! indexes schema positions, resolves references only to indexed resources and
//! rejects cycles that never descend into an instance member. Dynamic references
//! are checked at their initial static target; dynamic evaluation belongs to the
//! pinned validator and the tested, fixed corpus.

use std::collections::BTreeMap;

use jsonschema::Uri;
use serde_json::Value;

pub(super) const MAX_SCHEMA_NODES: usize = 20_000;
pub(super) const MAX_SCHEMA_DEPTH: usize = 128;
pub(super) const MAX_SAME_INSTANCE_DEPTH: usize = 256;

#[derive(Clone, Copy)]
struct Limits {
    nodes: usize,
    depth: usize,
    same_instance_depth: usize,
}

struct Node<'a> {
    document: String,
    pointer: String,
    value: &'a Value,
    edges: Vec<usize>,
}

struct Reference {
    source: usize,
    target: String,
    applies: bool,
}

struct Guard<'a> {
    limits: Limits,
    nodes: Vec<Node<'a>>,
    positions: BTreeMap<(String, String), usize>,
    resources: BTreeMap<String, usize>,
    anchors: BTreeMap<(String, String), usize>,
    references: Vec<Reference>,
}

pub(super) fn check_catalog(catalog: &BTreeMap<String, Value>) -> Result<(), String> {
    check_with_limits(
        catalog,
        Limits {
            nodes: MAX_SCHEMA_NODES,
            depth: MAX_SCHEMA_DEPTH,
            same_instance_depth: MAX_SAME_INSTANCE_DEPTH,
        },
    )
}

fn check_with_limits(catalog: &BTreeMap<String, Value>, limits: Limits) -> Result<(), String> {
    let mut guard = Guard {
        limits,
        nodes: Vec::new(),
        positions: BTreeMap::new(),
        resources: BTreeMap::new(),
        anchors: BTreeMap::new(),
        references: Vec::new(),
    };
    for (document, schema) in catalog {
        let uri =
            Uri::parse(document.as_str()).map_err(|_| "invalid catalog resource URI".to_owned())?;
        if uri.fragment().is_some() {
            return Err("catalog resource URI contains a fragment".to_owned());
        }
        guard.visit(document, String::new(), schema, document, 1)?;
    }
    for reference in &guard.references {
        let target = guard.target(&reference.target)?;
        if reference.applies {
            guard.nodes[reference.source].edges.push(target);
        }
    }
    guard.check_cycles()
}

impl<'a> Guard<'a> {
    fn resolve(base: &str, reference: &str) -> Result<String, String> {
        let base = Uri::parse(base).map_err(|_| "invalid schema base URI".to_owned())?;
        // This is the pinned validator's pure RFC 3986 URI operation. It has
        // no retriever, registry contents, network or filesystem access.
        jsonschema::uri::resolve_against(&base, reference)
            .map(|uri| uri.as_str().to_owned())
            .map_err(|_| "invalid schema reference URI".to_owned())
    }

    fn register_resource(&mut self, uri: String, node: usize) -> Result<(), String> {
        if let Some(&existing) = self.resources.get(&uri) {
            // Manifest aliases may contain identical copies of one resource.
            // Conflicting identities are never resolved by insertion order.
            if self.nodes[existing].value != self.nodes[node].value {
                return Err("conflicting schema resource identity".to_owned());
            }
        } else {
            self.resources.insert(uri, node);
        }
        Ok(())
    }

    fn visit(
        &mut self,
        document: &str,
        pointer: String,
        value: &'a Value,
        inherited_base: &str,
        depth: usize,
    ) -> Result<usize, String> {
        if depth > self.limits.depth {
            return Err("schema nesting depth limit exceeded".to_owned());
        }
        if self.nodes.len() >= self.limits.nodes {
            return Err("schema node limit exceeded".to_owned());
        }
        if !value.is_object() && !value.is_boolean() {
            return Err("schema position is neither an object nor a boolean".to_owned());
        }
        let mut base = inherited_base.to_owned();
        if let Some(id) = value.get("$id") {
            let id = id
                .as_str()
                .ok_or_else(|| "schema identifier is not a string".to_owned())?;
            base = Self::resolve(inherited_base, id)?;
            let mut uri = Uri::parse(base.as_str())
                .map_err(|_| "invalid schema identifier".to_owned())?
                .to_owned();
            if uri.fragment().is_some_and(|fragment| !fragment.is_empty()) {
                return Err("nonempty schema identifier fragment is unsupported".to_owned());
            }
            uri.set_fragment(None);
            base = uri.as_str().to_owned();
        }
        let node = self.nodes.len();
        self.positions
            .insert((document.to_owned(), pointer.clone()), node);
        self.nodes.push(Node {
            document: document.to_owned(),
            pointer: pointer.clone(),
            value,
            edges: Vec::new(),
        });
        if pointer.is_empty() {
            self.register_resource(document.to_owned(), node)?;
        }
        if value.get("$id").is_some() {
            self.register_resource(base.clone(), node)?;
        }
        let Some(object) = value.as_object() else {
            return Ok(node);
        };
        for keyword in ["$anchor", "$dynamicAnchor"] {
            if let Some(anchor) = object.get(keyword) {
                let anchor = anchor
                    .as_str()
                    .filter(|name| valid_anchor(name))
                    .ok_or_else(|| "invalid schema anchor".to_owned())?;
                let key = (base.clone(), anchor.to_owned());
                if let Some(&existing) = self.anchors.get(&key) {
                    if self.nodes[existing].value != value {
                        return Err("conflicting schema anchor".to_owned());
                    }
                } else {
                    self.anchors.insert(key, node);
                }
            }
        }
        for keyword in ["$ref", "$dynamicRef", "$recursiveRef", "$schema"] {
            if let Some(reference) = object.get(keyword) {
                let reference = reference
                    .as_str()
                    .ok_or_else(|| "schema reference is not a string".to_owned())?;
                self.references.push(Reference {
                    source: node,
                    target: Self::resolve(&base, reference)?,
                    // A dialect declaration identifies a schema but does not
                    // apply that schema to the instance at this position.
                    applies: keyword != "$schema",
                });
            }
        }
        for keyword in [
            "$defs",
            "definitions",
            "properties",
            "patternProperties",
            "dependentSchemas",
            "dependencies",
        ] {
            if let Some(children) = object.get(keyword) {
                let children = children
                    .as_object()
                    .ok_or_else(|| "schema map keyword is not an object".to_owned())?;
                for (name, child) in children {
                    if keyword == "dependencies" && child.is_array() {
                        continue;
                    }
                    let path = pointer_child(&pointer_child(&pointer, keyword), name);
                    let child = self.visit(document, path, child, &base, depth + 1)?;
                    if matches!(keyword, "dependentSchemas" | "dependencies") {
                        self.nodes[node].edges.push(child);
                    }
                }
            }
        }
        for keyword in ["allOf", "anyOf", "oneOf", "prefixItems"] {
            if let Some(children) = object.get(keyword) {
                let children = children
                    .as_array()
                    .ok_or_else(|| "schema array keyword is not an array".to_owned())?;
                for (index, child) in children.iter().enumerate() {
                    let path = pointer_child(&pointer_child(&pointer, keyword), &index.to_string());
                    let child = self.visit(document, path, child, &base, depth + 1)?;
                    if keyword != "prefixItems" {
                        self.nodes[node].edges.push(child);
                    }
                }
            }
        }
        for keyword in [
            "not",
            "if",
            "then",
            "else",
            "items",
            "additionalItems",
            "contains",
            "additionalProperties",
            "unevaluatedItems",
            "unevaluatedProperties",
            "propertyNames",
            "contentSchema",
        ] {
            if let Some(child) = object.get(keyword) {
                let path = pointer_child(&pointer, keyword);
                if keyword == "items"
                    && let Some(children) = child.as_array()
                {
                    for (index, child) in children.iter().enumerate() {
                        self.visit(
                            document,
                            pointer_child(&path, &index.to_string()),
                            child,
                            &base,
                            depth + 1,
                        )?;
                    }
                    continue;
                }
                let child = self.visit(document, path, child, &base, depth + 1)?;
                if matches!(keyword, "not" | "if" | "then" | "else") {
                    self.nodes[node].edges.push(child);
                }
            }
        }
        Ok(node)
    }

    fn target(&self, target: &str) -> Result<usize, String> {
        let uri = Uri::parse(target).map_err(|_| "invalid resolved schema URI".to_owned())?;
        let fragment = uri
            .fragment()
            .map(|fragment| {
                fragment
                    .decode()
                    .to_string()
                    .map(|value| value.into_owned())
            })
            .transpose()
            .map_err(|_| "schema fragment is not UTF-8".to_owned())?
            .unwrap_or_default();
        let mut resource = uri.to_owned();
        resource.set_fragment(None);
        let resource = resource.as_str();
        let &root = self
            .resources
            .get(resource)
            .ok_or_else(|| "schema reference is outside the embedded catalog".to_owned())?;
        if fragment.is_empty() {
            return Ok(root);
        }
        if !fragment.starts_with('/') {
            return self
                .anchors
                .get(&(resource.to_owned(), fragment))
                .copied()
                .ok_or_else(|| "schema anchor target is missing".to_owned());
        }
        // Validate pointer escaping as well as existence; serde_json's pointer
        // lookup alone accepts malformed '~' escapes as literal characters.
        if fragment
            .split('/')
            .skip(1)
            .any(|part| !valid_pointer_part(part))
        {
            return Err("invalid schema JSON Pointer escape".to_owned());
        }
        let root = &self.nodes[root];
        if root.value.pointer(&fragment).is_none() {
            return Err("schema JSON Pointer target is missing".to_owned());
        }
        self.positions
            .get(&(root.document.clone(), format!("{}{fragment}", root.pointer)))
            .copied()
            .ok_or_else(|| "reference target is not an indexed schema position".to_owned())
    }

    fn check_cycles(&self) -> Result<(), String> {
        let mut colors = vec![0_u8; self.nodes.len()];
        let mut heights = vec![0_usize; self.nodes.len()];
        // Start from every schema position, including definitions and schemas
        // below progress edges; otherwise an unreachable/subtree cycle escapes.
        for start in 0..self.nodes.len() {
            if colors[start] != 0 {
                continue;
            }
            colors[start] = 1;
            let mut stack = vec![(start, 0_usize)];
            while let Some(&(node, next)) = stack.last() {
                if stack.len() > self.limits.same_instance_depth {
                    return Err("same-instance schema depth limit exceeded".to_owned());
                }
                if let Some(&child) = self.nodes[node].edges.get(next) {
                    if let Some(frame) = stack.last_mut() {
                        frame.1 += 1;
                    }
                    match colors[child] {
                        0 => {
                            colors[child] = 1;
                            stack.push((child, 0));
                        }
                        1 => return Err("non-progressing schema reference cycle".to_owned()),
                        _ => {}
                    }
                } else {
                    heights[node] = self.nodes[node]
                        .edges
                        .iter()
                        .map(|&child| heights[child])
                        .max()
                        .unwrap_or(0)
                        + 1;
                    if heights[node] > self.limits.same_instance_depth {
                        return Err("same-instance schema depth limit exceeded".to_owned());
                    }
                    colors[node] = 2;
                    stack.pop();
                }
            }
        }
        Ok(())
    }
}

fn pointer_child(parent: &str, name: &str) -> String {
    format!("{parent}/{}", name.replace('~', "~0").replace('/', "~1"))
}

fn valid_pointer_part(part: &str) -> bool {
    let mut bytes = part.bytes();
    while let Some(byte) = bytes.next() {
        if byte == b'~' && !matches!(bytes.next(), Some(b'0' | b'1')) {
            return false;
        }
    }
    true
}

fn valid_anchor(name: &str) -> bool {
    let mut bytes = name.bytes();
    bytes
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == b'_')
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
}

#[cfg(test)]
mod tests {
    use super::{Limits, check_catalog, check_with_limits};
    use serde_json::{Value, json};
    use std::collections::BTreeMap;

    const ROOT: &str = "https://schemas.example/root.json";

    fn catalog(schema: Value) -> BTreeMap<String, Value> {
        BTreeMap::from([(ROOT.to_owned(), schema)])
    }

    #[test]
    fn rejects_direct_and_mutual_nonprogress_cycles() {
        for schema in [
            json!({"$ref": "#"}),
            json!({"$ref": ""}),
            json!({"$defs": {"a": {"$ref": "#/$defs/b"}, "b": {"$ref": "#/$defs/a"}}}),
            json!({"properties": {"child": {"$ref": "#/properties/child"}}}),
        ] {
            assert_eq!(
                check_catalog(&catalog(schema)),
                Err("non-progressing schema reference cycle".to_owned())
            );
        }
        let mut schemas = catalog(json!({"$ref": "peer.json"}));
        schemas.insert(
            "https://schemas.example/peer.json".to_owned(),
            json!({"$ref": "root.json"}),
        );
        assert_eq!(
            check_catalog(&schemas),
            Err("non-progressing schema reference cycle".to_owned())
        );
    }

    #[test]
    fn rejects_nonprogress_applicator_cycles() {
        for keyword in ["allOf", "anyOf", "oneOf"] {
            assert_eq!(
                check_catalog(&catalog(json!({keyword: [{"$ref": "#"}]}))),
                Err("non-progressing schema reference cycle".to_owned())
            );
        }
        for keyword in ["not", "if", "then", "else"] {
            assert_eq!(
                check_catalog(&catalog(json!({keyword: {"$ref": "#"}}))),
                Err("non-progressing schema reference cycle".to_owned())
            );
        }
        for keyword in ["dependentSchemas", "dependencies"] {
            assert_eq!(
                check_catalog(&catalog(json!({keyword: {"x": {"$ref": "#"}}}))),
                Err("non-progressing schema reference cycle".to_owned())
            );
        }
    }

    #[test]
    fn accepts_progressing_recursion_and_ignores_instance_data() {
        for schema in [
            json!({"properties": {"child": {"$ref": "#"}}}),
            json!({"items": {"$ref": "#"}}),
            json!({"prefixItems": [{"$ref": "#"}]}),
            json!({"allOf": [{"properties": {"child": {"$ref": "#"}}}]}),
            json!({"default": {"$ref": "file:///not-a-schema"}, "examples": [{"$ref": "#"}]}),
            json!({"const": {"$ref": "#"}, "enum": [{"$ref": "#"}]}),
            json!({"dependencies": {"x": ["y"]}}),
        ] {
            assert_eq!(check_catalog(&catalog(schema)), Ok(()));
        }
    }

    #[test]
    fn rejects_missing_resources_pointers_and_anchors() {
        for (reference, expected) in [
            (
                "missing.json",
                "schema reference is outside the embedded catalog",
            ),
            ("#/$defs/missing", "schema JSON Pointer target is missing"),
            ("#missing", "schema anchor target is missing"),
            ("#/$defs/a~2b", "invalid schema JSON Pointer escape"),
            (
                "#/default",
                "reference target is not an indexed schema position",
            ),
        ] {
            assert_eq!(
                check_catalog(&catalog(json!({"$ref": reference, "default": {}}))),
                Err(expected.to_owned())
            );
        }
    }

    #[test]
    fn rejects_http_file_data_and_uri_escape_canaries() {
        // No retriever or filesystem API receives these URIs: every target
        // must match an indexed in-memory resource before compilation.
        for reference in [
            "http://127.0.0.1:9/schema.json",
            "http://169.254.169.254/latest/meta-data/",
            "file:///etc/passwd",
            "file:///C:/Windows/win.ini",
            "data:application/json,%7B%7D",
            "../escape.json",
            "root.json?unlisted=1",
            "https://schemas.example.evil/root.json",
        ] {
            assert_eq!(
                check_catalog(&catalog(json!({"$ref": reference}))),
                Err("schema reference is outside the embedded catalog".to_owned())
            );
        }
    }

    #[test]
    fn resolves_relative_ids_anchors_aliases_and_escaped_pointers() {
        let schema = json!({
            "$id": "root.json",
            "$defs": {
                "sub": {"$id": "nested/child.json", "$anchor": "child", "type": "string"},
                "a/b~c": {"type": "number"}
            },
            "properties": {
                "a": {"$ref": "nested/child.json#child"},
                "b": {"$ref": "#/$defs/a~1b~0c"},
                "c": {"$ref": "#/%24defs/a~1b~0c"}
            }
        });
        assert_eq!(check_catalog(&catalog(schema)), Ok(()));
        let schema =
            json!({"$id": ROOT, "$dynamicAnchor": "meta", "items": {"$dynamicRef": "#meta"}});
        let mut schemas = catalog(schema.clone());
        schemas.insert("https://aliases.example/root.json".to_owned(), schema);
        assert_eq!(check_catalog(&schemas), Ok(()));
    }

    #[test]
    fn resolves_embedded_resource_pointers_and_rejects_conflicting_ids() {
        let schema = json!({
            "$defs": {"sub": {"$id": "child.json", "$defs": {"leaf": {"type": "string"}}}},
            "$ref": "child.json#/$defs/leaf"
        });
        assert_eq!(check_catalog(&catalog(schema)), Ok(()));
        let schema = json!({"$defs": {
            "a": {"$id": "child.json", "type": "string"},
            "b": {"$id": "child.json", "type": "number"}
        }});
        assert_eq!(
            check_catalog(&catalog(schema)),
            Err("conflicting schema resource identity".to_owned())
        );
    }

    #[test]
    fn enforces_schema_node_and_depth_boundaries() {
        let limits = Limits {
            nodes: 3,
            depth: 3,
            same_instance_depth: 3,
        };
        assert_eq!(
            check_with_limits(&catalog(json!({"allOf": [true, false]})), limits),
            Ok(())
        );
        assert_eq!(
            check_with_limits(&catalog(json!({"allOf": [true, false, true]})), limits),
            Err("schema node limit exceeded".to_owned())
        );
        assert_eq!(
            check_with_limits(&catalog(json!({"items": {"items": true}})), limits),
            Ok(())
        );
        assert_eq!(
            check_with_limits(
                &catalog(json!({"items": {"items": {"items": true}}})),
                limits
            ),
            Err("schema nesting depth limit exceeded".to_owned())
        );
    }

    #[test]
    fn enforces_reference_depth_including_previously_finished_branches() {
        let limits = Limits {
            nodes: 20,
            depth: 5,
            same_instance_depth: 3,
        };
        let schema = json!({"$defs": {
            "a": true,
            "b": {"$ref": "#/$defs/a"},
            "c": {"$ref": "#/$defs/b"}
        }});
        assert_eq!(check_with_limits(&catalog(schema.clone()), limits), Ok(()));
        let mut too_deep = schema;
        too_deep["$defs"]["d"] = json!({"$ref": "#/$defs/c"});
        assert_eq!(
            check_with_limits(&catalog(too_deep), limits),
            Err("same-instance schema depth limit exceeded".to_owned())
        );
    }
}
