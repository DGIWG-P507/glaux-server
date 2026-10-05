//! Source-derived association positions and independently counted graph walks.
//! SWE basicTypes defines fragment-addressed IDs; binary name paths are separate.
use super::{
    ComponentGraph, MAX_COMPONENTS, MAX_REFERENCES, MAX_TRAVERSAL_DEPTH,
    MAX_TRAVERSAL_STEPS, ReferenceError, ReferenceKind, ReferenceTarget,
};
use crate::{
    array::{ArrayOptions, CountReferenceSchema, SourceValidation},
    validation::{self, Contract, Failure, StructuralValidator},
};
use serde_json::{Value, json};
use std::sync::OnceLock;

const LOCAL: &[u8] = include_bytes!("../../fixtures/references/local-graph.json");
const NONLOCAL: &[u8] = include_bytes!("../../fixtures/references/nonlocal-graph.json");

fn validator() -> &'static StructuralValidator {
    static VALIDATOR: OnceLock<StructuralValidator> = OnceLock::new();
    VALIDATOR.get_or_init(|| StructuralValidator::new().expect("pinned offline corpus"))
}

fn bytes(value: &Value) -> Vec<u8> {
    serde_json::to_vec(value).unwrap()
}

fn resolve(value: &Value) -> Result<ComponentGraph, ReferenceError> {
    ComponentGraph::resolve(validator(), &bytes(value), ArrayOptions::default())
}

fn correction() -> ArrayOptions {
    ArrayOptions {
        count_reference_schema: CountReferenceSchema::DisjointReferenceCorrection,
    }
}

fn count(name: &str, id: &str) -> Value {
    json!({"type":"Count","name":name,"id":id,"definition":"urn:example:count","label":"Count"})
}

fn record(fields: Vec<Value>) -> Value {
    json!({"type":"DataRecord","fields":fields})
}

fn linked(name: &str, href: &str) -> Value {
    json!({"name":name,"href":href})
}

fn assert_error(value: &Value, expected: ReferenceError) {
    assert_eq!(resolve(value).err(), Some(expected));
}

#[test]
fn reference_local_ids_resolve_exact_targets() {
    let graph = ComponentGraph::resolve(validator(), LOCAL, ArrayOptions::default()).unwrap();
    assert_eq!(graph.source(), LOCAL);
    assert_eq!(graph.source_validation(), SourceValidation::Original);
    let nodes = graph.components();
    assert_eq!(
        nodes.iter().map(|node| node.id.as_deref()).collect::<Vec<_>>(),
        [Some("ROOT"), Some("TARGET_NEAR"), Some("BOX"), Some("TARGET")]
    );
    assert_eq!(
        nodes.iter().map(|node| node.path.as_str()).collect::<Vec<_>>(),
        ["", "/fields/0", "/fields/2", "/fields/2/fields/0"]
    );
    assert_eq!(nodes[3].name.as_deref(), Some("target"));
    assert_eq!(nodes[3].kind, Contract::Count);
    let refs = graph.references();
    assert_eq!(refs.len(), 3);
    assert_eq!(refs[0].owner, 0);
    assert_eq!(refs[0].name.as_deref(), Some("forward"));
    assert_eq!(refs[0].kind, ReferenceKind::Component);
    assert_eq!(refs[0].target, ReferenceTarget::Local(3));
    assert_eq!(graph.traversal(), [0, 1, 3, 2, 3, 1, 2, 3, 1]);
    assert_eq!(refs[1].owner, 2);
    assert_eq!(refs[1].target, ReferenceTarget::Local(1));
    assert_eq!(refs[2].target, ReferenceTarget::Local(2));
    assert_eq!(refs[0].role.as_deref(), Some("urn:example:role"));
    assert_eq!(refs[0].arcrole.as_deref(), Some("urn:example:arc"));
    assert_eq!(refs[0].title.as_deref(), Some(" Exact target "));
    assert_eq!(refs[1].path, "/fields/2/fields/1");
    assert_eq!(
        graph.component_source(3),
        Some(br#"{ "name": "target", "type": "Count", "id": "TARGET", "definition": "urn:example:target", "label": "Target", "value": 9007199254740993 }"#.as_slice())
    );
    assert_eq!(
        graph.reference_source(1),
        Some(br#"{ "name": "back", "href": "#TARGET_NEAR" }"#.as_slice())
    );
    for (index, node) in nodes.iter().enumerate() {
        assert_eq!(graph.component_source(index), Some(&LOCAL[node.source.clone()]));
    }
    for (index, reference) in refs.iter().enumerate() {
        assert_eq!(graph.reference_source(index), Some(&LOCAL[reference.source.clone()]));
    }
    assert_eq!(graph.component_source(nodes.len()), None);
    assert_eq!(graph.reference_source(refs.len()), None);
}

#[test]
fn reference_fragments_decode_once_without_name_paths() {
    for (id, href) in [
        ("PRESS_QC", "#PRESS%5FQC"),
        ("a/b", "#a%2Fb"),
        ("a%2Fb", "#a%252Fb"),
        ("a+b", "#a+b"),
        ("a b", "#a%20b"),
        ("é", "#%C3%A9"),
    ] {
        let graph = resolve(&record(vec![count("differentName", id), linked("alias", href)]))
            .unwrap();
        assert_eq!(graph.references()[0].href, href);
        assert_eq!(graph.references()[0].target, ReferenceTarget::Local(1));
        assert_eq!(graph.components()[1].id.as_deref(), Some(id));
    }
    for href in ["#differentName", "#/fields/0", "#press_qc"] {
        assert_error(
            &record(vec![count("differentName", "PRESS_QC"), linked("alias", href)]),
            ReferenceError::UnresolvedLocal,
        );
    }
    for href in ["", "#", "#%", "#%ZZ", "#%FF", "not a URI"] {
        assert!(resolve(&record(vec![linked("invalid", href)])).is_err(), "{href}");
    }
}

#[test]
fn reference_nonlocal_metadata_preserved_without_fetch() {
    let graph = ComponentGraph::resolve(validator(), NONLOCAL, ArrayOptions::default()).unwrap();
    assert_eq!(graph.source(), NONLOCAL);
    assert_eq!(graph.components().len(), 1);
    assert_eq!(graph.traversal(), [0]);
    assert_eq!(
        graph.references().iter().map(|reference| reference.href.as_str()).collect::<Vec<_>>(),
        [
            "https://example.invalid/protected.json#TARGET",
            "file:///forbidden/component.json#TARGET",
            "data:application/json,%7B%22id%22%3A%22TARGET%22%7D",
            "../unavailable/component.json#TARGET",
            "urn:example:unavailable:component",
        ]
    );
    for reference in graph.references() {
        assert_eq!(reference.target, ReferenceTarget::Nonlocal);
        assert_eq!(reference.kind, ReferenceKind::Component);
    }
    let first = &graph.references()[0];
    assert_eq!(first.role.as_deref(), Some("urn:example:remote"));
    assert_eq!(first.arcrole.as_deref(), Some("urn:example:association"));
    assert_eq!(first.title.as_deref(), Some(" Network metadata "));
    let second = &graph.references()[1];
    assert_eq!(second.title.as_deref(), Some(" File metadata "));
    assert_eq!(second.role, None);
    assert_eq!(second.arcrole, None);
}

#[test]
fn reference_duplicate_ids_and_unresolved_targets_fail() {
    assert_error(
        &record(vec![count("a", "SAME"), count("b", "SAME")]),
        ReferenceError::DuplicateId,
    );
    let mut root_duplicate = record(vec![count("a", "SAME")]);
    root_duplicate["id"] = json!("SAME");
    assert_error(&root_duplicate, ReferenceError::DuplicateId);
    for href in ["#MISSING", "#HIDDEN"] {
        let mut input: Value = serde_json::from_slice(LOCAL).unwrap();
        input["fields"][1]["href"] = json!(href);
        assert_error(&input, ReferenceError::UnresolvedLocal);
    }
    let input = record(vec![
        json!({"name":"alias","id":"ALIAS","href":"https://example.invalid/target"}),
        linked("aliasReference", "#ALIAS"),
    ]);
    assert_error(&input, ReferenceError::UnresolvedLocal);
}

#[test]
fn reference_cycles_fail() {
    let mut self_cycle = record(vec![linked("self", "#SELF")]);
    self_cycle["id"] = json!("SELF");
    assert_error(&self_cycle, ReferenceError::Cycle);
    let a = json!({"name":"a","type":"DataRecord","id":"A","fields":[{"name":"toB","href":"#B"}]});
    let b = json!({"name":"b","type":"DataRecord","id":"B","fields":[{"name":"toA","href":"#A"}]});
    assert_error(&record(vec![a, b]), ReferenceError::Cycle);
    let parent_cycle = json!({"type":"DataRecord","id":"ROOT","fields":[{
        "name":"nested","type":"DataRecord","fields":[{"name":"parent","href":"#ROOT"}]
    }]});
    assert_error(&parent_cycle, ReferenceError::Cycle);
}

#[test]
fn reference_permitted_slots_and_count_schema_qualification() {
    let input = record(vec![
        count("size", "SIZE"),
        json!({"name":"choice","type":"DataChoice","choiceValue":{
            "type":"Category","id":"SELECTOR","definition":"urn:example:selector","label":"Selector"
        },"items":[{"name":"one","href":"#SIZE"},{"name":"two","href":"#SELECTOR"}]}),
        json!({"name":"samples","type":"DataArray","elementCount":{"href":"#SIZE"},
            "elementType":{"name":"sample","href":"#SIZE"}}),
    ]);
    assert_eq!(validator().validate(Contract::DataRecord, &bytes(&input)), Err(Failure::Structure));
    assert_error(&input, ReferenceError::Structure);
    let graph = ComponentGraph::resolve(validator(), &bytes(&input), correction()).unwrap();
    assert_eq!(graph.source_validation(), SourceValidation::CountReferenceCorrection);
    assert_eq!(graph.components()[3].path, "/fields/1/choiceValue");
    assert_eq!(graph.components()[3].kind, Contract::Category);
    assert_eq!(graph.references()[0].target, ReferenceTarget::Local(1));
    assert_eq!(graph.references()[1].target, ReferenceTarget::Local(3));
    assert_eq!(graph.references()[2].kind, ReferenceKind::ElementCount);
    assert_eq!(graph.references()[2].name, None);
    assert_eq!(graph.references()[2].path, "/fields/2/elementCount");
    assert_eq!(graph.references()[2].target, ReferenceTarget::Local(1));
    assert_eq!(graph.references()[3].target, ReferenceTarget::Local(1));
    assert_eq!(graph.traversal(), [0, 1, 2, 3, 1, 3, 4, 1, 1]);
    let inline_count = record(vec![
        json!({"name":"samples","type":"DataArray","elementCount":{"id":"DIM","value":3},
            "elementType":count("sample", "SAMPLE")}),
        linked("dimension", "#DIM"),
    ]);
    let graph = resolve(&inline_count).unwrap();
    assert_eq!(graph.components()[2].kind, Contract::Count);
    assert_eq!(graph.components()[2].path, "/fields/0/elementCount");
    assert_eq!(graph.references()[0].target, ReferenceTarget::Local(2));
    let mut duplicate = inline_count;
    duplicate["fields"][0]["elementType"]["id"] = json!("DIM");
    assert_error(&duplicate, ReferenceError::DuplicateId);
}

#[test]
fn reference_target_restrictions_survive_resolution() {
    let text = json!({"type":"Text","name":"text","id":"TEXT","definition":"urn:example:text","label":"Text"});
    let wrong_count = record(vec![
        text.clone(),
        json!({"name":"samples","type":"DataArray","elementCount":{"href":"#TEXT"},"elementType":count("sample", "SAMPLE")}),
    ]);
    assert_eq!(
        ComponentGraph::resolve(validator(), &bytes(&wrong_count), correction()).err(),
        Some(ReferenceError::InvalidTarget)
    );
    let wrong_matrix = record(vec![
        text,
        json!({"name":"matrix","type":"Matrix","elementCount":{"value":2},"elementType":{"name":"entry","href":"#TEXT"}}),
    ]);
    assert_error(&wrong_matrix, ReferenceError::InvalidTarget);
    let valid_matrix = record(vec![
        count("coefficient", "COEFFICIENT"),
        json!({"name":"matrix","type":"Matrix","elementCount":{"value":2},"elementType":{"name":"entry","href":"#COEFFICIENT"}}),
    ]);
    assert_eq!(resolve(&valid_matrix).unwrap().references()[0].target, ReferenceTarget::Local(1));
    let mut valued = valid_matrix;
    valued["fields"][0]["value"] = json!(9);
    assert_error(&valued, ReferenceError::InlineElementValue);
    let nested_value = record(vec![
        json!({"name":"template","type":"DataRecord","id":"TEMPLATE","fields":[{
            "name":"value","type":"Count","definition":"urn:example:value","label":"Value","value":7
        }]}),
        json!({"name":"samples","type":"DataArray","elementCount":{"value":2},"elementType":{"name":"entry","href":"#TEMPLATE"}}),
    ]);
    assert_error(&nested_value, ReferenceError::InlineElementValue);
}

#[test]
fn reference_nested_array_counts_remain_descriptor_metadata() {
    for kind in ["DataArray", "Matrix"] {
        let inner = json!({"type":kind,"name":"row","id":"ROW","elementCount":{"id":"INNER_COUNT","value":7},
            "elementType":count("sample", "SAMPLE")});
        let linked_outer = json!({"type":kind,"name":"outer","elementCount":{"value":3},
            "elementType":{"name":"row","href":"#ROW"}});
        let graph = resolve(&record(vec![inner.clone(), linked_outer])).unwrap();
        assert_eq!(graph.references()[0].target, ReferenceTarget::Local(1));
        assert_eq!(graph.components()[2].kind, Contract::Count);
        assert_eq!(graph.components()[2].id.as_deref(), Some("INNER_COUNT"));
        assert_eq!(graph.traversal(), [0, 1, 2, 3, 4, 5, 1, 2, 3]);
        let nested = json!({"type":kind,"elementCount":{"value":3},"elementType":inner});
        let graph = resolve(&nested).unwrap();
        assert_eq!(graph.traversal(), [0, 1, 2, 3, 4]);
    }
    let linked_selector_value = record(vec![
        json!({"name":"template","type":"DataChoice","id":"CHOICE","choiceValue":{
            "type":"Category","definition":"urn:example:selection","label":"Selection","value":"a"
        },"items":[count("a", "A"),count("b", "B")]}),
        json!({"name":"samples","type":"DataArray","elementCount":{"value":2},
            "elementType":{"name":"entry","href":"#CHOICE"}}),
    ]);
    assert_error(&linked_selector_value, ReferenceError::InlineElementValue);
}

#[test]
fn reference_all_component_families_are_valid_targets() {
    let cases = [
        (Contract::Boolean, json!({"type":"Boolean","definition":"urn:example:b","label":"B"})),
        (Contract::Text, json!({"type":"Text","definition":"urn:example:t","label":"T"})),
        (Contract::Category, json!({"type":"Category","definition":"urn:example:c","label":"C"})),
        (Contract::Count, count("count", "COUNT")),
        (Contract::Quantity, json!({"type":"Quantity","definition":"urn:example:q","label":"Q","uom":{"code":"m"}})),
        (Contract::Time, json!({"type":"Time","definition":"urn:example:t","label":"T","uom":{"code":"s"}})),
        (Contract::CategoryRange, json!({"type":"CategoryRange","definition":"urn:example:cr","label":"CR"})),
        (Contract::CountRange, json!({"type":"CountRange","definition":"urn:example:cr","label":"CR"})),
        (Contract::QuantityRange, json!({"type":"QuantityRange","definition":"urn:example:qr","label":"QR","uom":{"code":"m"}})),
        (Contract::TimeRange, json!({"type":"TimeRange","definition":"urn:example:tr","label":"TR","uom":{"code":"s"}})),
        (Contract::Geometry, json!({"type":"Geometry","definition":"urn:example:g","label":"G","srs":"urn:example:frame"})),
        (Contract::DataRecord, record(vec![count("field", "FIELD")])),
        (Contract::Vector, json!({"type":"Vector","definition":"urn:example:v","label":"V","referenceFrame":"urn:example:frame",
            "coordinates":[{"type":"Count","name":"x","definition":"urn:example:x","label":"X","axisID":"X"}]})),
        (Contract::DataChoice, json!({"type":"DataChoice","items":[count("a", "A"),count("b", "B")]})),
        (Contract::DataArray, json!({"type":"DataArray","elementCount":{"value":2},"elementType":count("sample", "SAMPLE")})),
        (Contract::Matrix, json!({"type":"Matrix","elementCount":{"value":2},"elementType":count("sample", "SAMPLE")})),
    ];
    for (kind, mut target) in cases {
        target["name"] = json!("target");
        target["id"] = json!("TARGET");
        let graph = resolve(&record(vec![target, linked("alias", "#TARGET")])).unwrap();
        assert_eq!(graph.components()[1].kind, kind);
        assert_eq!(graph.references()[0].target, ReferenceTarget::Local(1));
    }
}

#[test]
fn reference_structure_and_metadata_are_not_bypassed() {
    let mut root_extension = count("root", "ROOT");
    root_extension["name"] = json!(7);
    let graph = resolve(&root_extension).unwrap();
    assert_eq!(graph.components()[0].id.as_deref(), Some("ROOT"));
    assert_eq!(graph.source(), bytes(&root_extension));
    for extension in [json!(7), json!("not a URI")] {
        let mut inline = count("inline", "INLINE");
        inline["href"] = extension;
        let input = record(vec![inline]);
        let graph = resolve(&input).unwrap();
        assert_eq!(graph.components().len(), 2);
        assert_eq!(graph.components()[1].id.as_deref(), Some("INLINE"));
        assert!(graph.references().is_empty());
        assert_eq!(graph.source(), bytes(&input));
    }
    for kind_extension in ["Count", "Vendor"] {
        let input = record(vec![json!({"name":"association","href":"https://example.invalid/target",
            "type":kind_extension,"value":7,"id":"METADATA_ONLY"})]);
        let graph = resolve(&input).unwrap();
        assert_eq!(graph.components().len(), 1);
        assert_eq!(graph.references().len(), 1);
        assert_eq!(graph.references()[0].target, ReferenceTarget::Nonlocal);
        assert_eq!(graph.source(), bytes(&input));
    }
    let vector = json!({"type":"Vector","definition":"urn:example:vector","label":"Vector",
        "referenceFrame":"urn:example:frame","coordinates":[{"name":"x","href":"#X"}]});
    assert!(resolve(&vector).is_err());
    for field in [
        json!({"href":"https://example.invalid/target"}),
        json!({"name":"bad name","href":"https://example.invalid/target"}),
        json!({"name":"bad","href":7}),
        json!({"name":"bad","href":"https://example.invalid/target","title":""}),
        json!({"name":"bad","href":"https://example.invalid/target","role":"relative"}),
        json!({"name":"bad","href":"https://example.invalid/target","arcrole":"not a URI"}),
        json!({"name":"mixed","href":"#X","type":"Count","definition":"urn:example:count","label":"Count"}),
    ] {
        assert!(resolve(&record(vec![field])).is_err());
    }
    let input = br#"{"type":"DataRecord","fields":[{"name":"a","href":"#A","href":"#B"}]}"#;
    assert_eq!(
        ComponentGraph::resolve(validator(), input, ArrayOptions::default()).err(),
        Some(ReferenceError::Syntax(Failure::DuplicateKey))
    );
}

#[test]
fn reference_component_limit_boundaries() {
    assert_eq!(MAX_COMPONENTS, 512);
    // Three aggregate nodes, plus N leaves. Splitting keeps fields below 512.
    for total in [511, 512, 513] {
        let leaves = total - 3;
        let mut left = record((0..leaves / 2).map(|i| count(&format!("a{i}"), &format!("A{i}"))).collect());
        let mut right = record((0..leaves - leaves / 2).map(|i| count(&format!("b{i}"), &format!("B{i}"))).collect());
        left["name"] = json!("left");
        right["name"] = json!("right");
        let input = record(vec![left, right]);
        assert!(validation::parse(&bytes(&input)).is_ok());
        if total <= 512 {
            let graph = resolve(&input).unwrap();
            assert_eq!(graph.components().len(), total);
            assert_eq!(graph.traversal().len(), total);
        } else {
            assert_error(&input, ReferenceError::ComponentLimit);
        }
    }
}

#[test]
fn reference_reference_limit_boundaries() {
    assert_eq!(MAX_REFERENCES, 512);
    for total in [511, 512, 513] {
        let mut left = record((0..total / 2).map(|i| linked(&format!("a{i}"), "https://example.invalid/a")).collect());
        let mut right = record((0..total - total / 2).map(|i| linked(&format!("b{i}"), "file:///forbidden/b")).collect());
        left["name"] = json!("left");
        right["name"] = json!("right");
        let input = record(vec![left, right]);
        assert!(validation::parse(&bytes(&input)).is_ok());
        if total <= 512 {
            let graph = resolve(&input).unwrap();
            assert_eq!(graph.references().len(), total);
            assert_eq!(graph.traversal(), [0, 1, 2]);
        } else {
            assert_error(&input, ReferenceError::ReferenceLimit);
        }
    }
}

fn chain(depth: usize) -> Value {
    let mut fields = Vec::new();
    for index in 0..depth - 2 {
        fields.push(json!({"type":"DataRecord","name":format!("n{index}"),"id":format!("N{index}"),
            "fields":[{"name":"next","href":format!("#N{}", index + 1)}]}));
    }
    fields.push(count("leaf", &format!("N{}", depth - 2)));
    record(fields)
}

#[test]
fn reference_depth_limit_boundaries() {
    assert_eq!(MAX_TRAVERSAL_DEPTH, 32);
    for depth in [31, 32, 33] {
        let input = chain(depth);
        assert!(validation::parse(&bytes(&input)).is_ok());
        if depth <= 32 {
            let graph = resolve(&input).unwrap();
            assert_eq!(&graph.traversal()[..depth], (0..depth).collect::<Vec<_>>());
        } else {
            assert_error(&input, ReferenceError::DepthLimit);
        }
    }
}

fn shared_subtree(steps: usize) -> Value {
    let mut subtree = record((0..8).map(|i| count(&format!("leaf{i}"), &format!("LEAF{i}"))).collect());
    subtree["id"] = json!("SUBTREE");
    subtree["name"] = json!("template");
    let mut fields = vec![subtree];
    // Each whole subtree contributes exactly nine visits. Root contributes one.
    let whole = (steps - 1) / 9;
    for index in 1..whole {
        fields.push(linked(&format!("subtree{index}"), "#SUBTREE"));
    }
    for index in 0..(steps - 1) % 9 {
        fields.push(linked(&format!("extra{index}"), "#LEAF0"));
    }
    record(fields)
}

#[test]
fn reference_traversal_limit_boundaries() {
    assert_eq!(MAX_TRAVERSAL_STEPS, 4096);
    for steps in [4095, 4096, 4097] {
        let input = shared_subtree(steps);
        assert!(validation::parse(&bytes(&input)).is_ok());
        if steps <= 4096 {
            let graph = resolve(&input).unwrap();
            assert_eq!(graph.traversal().len(), steps);
            assert_eq!(&graph.traversal()[..10], [0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
            assert_eq!(&graph.traversal()[10..19], [1, 2, 3, 4, 5, 6, 7, 8, 9]);
        } else {
            assert_error(&input, ReferenceError::TraversalLimit);
        }
    }
}

#[test]
fn reference_generated_graphs_preserve_targets() {
    // Deterministic bounded model: names are deliberately unrelated to IDs.
    for size in 1..=24 {
        let mut fields = (0..size)
            .map(|index| count(&format!("display{}", size - index), &format!("ID{index}")))
            .collect::<Vec<_>>();
        for index in (0..size).rev() {
            fields.push(linked(&format!("alias{index}"), &format!("#ID{index}")));
        }
        let input = record(fields);
        let graph = resolve(&input).unwrap();
        let repeated = resolve(&input).unwrap();
        let expected = (0..=size).chain((1..=size).rev()).collect::<Vec<_>>();
        assert_eq!(graph.traversal(), expected);
        assert_eq!(graph.traversal(), repeated.traversal());
        for (index, reference) in graph.references().iter().enumerate() {
            assert_eq!(reference.target, ReferenceTarget::Local(size - index));
        }
        let mut missing = input.clone();
        missing["fields"][size]["href"] = json!("#MISSING");
        assert_error(&missing, ReferenceError::UnresolvedLocal);
        let mut duplicate = input;
        duplicate["id"] = json!("ID0");
        assert_error(&duplicate, ReferenceError::DuplicateId);
    }
}
