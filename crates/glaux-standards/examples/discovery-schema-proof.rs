//! Test-only compilation of the actual downloaded discovery schema and examples.
//! No production API, caller-selected filesystem path or retrieval capability.

use std::io::Read;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::{Value, json};

const MAX_INPUT: usize = 32_768;
const MARKER: &str = "Downloaded discovery schema: 2 valid examples; missing links, unfinished class, invalid schema and retrieval controls detected.";

#[derive(Clone, Default)]
struct DenyRetrieval(Arc<AtomicUsize>);

impl jsonschema::Retrieve for DenyRetrieval {
    fn retrieve(
        &self,
        _uri: &jsonschema::Uri<String>,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        self.0.fetch_add(1, Ordering::Relaxed);
        Err("test schema retrieval denied".into())
    }
}

fn bounded_local(value: &Value, depth: usize, nodes: &mut usize) -> Result<(), &'static str> {
    *nodes += 1;
    if depth > 32 || *nodes > 4096 {
        return Err("schema traversal bound exceeded");
    }
    match value {
        Value::Object(object) => {
            for (name, child) in object {
                match name.as_str() {
                    "$ref"
                        if !child
                            .as_str()
                            .is_some_and(|target| target.starts_with("#/$defs/")) =>
                    {
                        return Err("nonlocal schema reference refused");
                    }
                    "$dynamicRef" | "$recursiveRef" => {
                        return Err("unselected reference kind refused");
                    }
                    "$schema"
                        if depth != 0
                            || child.as_str()
                                != Some("https://json-schema.org/draft/2020-12/schema") =>
                    {
                        return Err("unselected dialect refused");
                    }
                    "$id" if depth != 0 => return Err("nested resource refused"),
                    _ => {}
                }
                bounded_local(child, depth + 1, nodes)?;
            }
        }
        Value::Array(array) => {
            for child in array {
                bounded_local(child, depth + 1, nodes)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn compile(schema: &Value, denial: DenyRetrieval) -> Result<jsonschema::Validator, String> {
    bounded_local(schema, 0, &mut 0).map_err(str::to_owned)?;
    jsonschema::options()
        .with_draft(jsonschema::Draft::Draft202012)
        .with_retriever(denial)
        .build(schema)
        .map_err(|error| error.to_string())
}

fn main() {
    assert_eq!(
        std::env::args().count(),
        1,
        "No path or target overrides accepted"
    );
    let mut input = Vec::new();
    std::io::stdin()
        .take((MAX_INPUT + 1) as u64)
        .read_to_end(&mut input)
        .unwrap();
    assert!(
        input.len() <= MAX_INPUT,
        "schema fixture input bound exceeded"
    );
    let document: Value = serde_json::from_slice(&input).expect("schema fixture JSON invalid");
    assert_eq!(document.as_object().unwrap().len(), 2);
    let schema = &document["schema"];
    assert!(schema.is_object());
    let examples = document["examples"].as_array().unwrap();
    assert_eq!(examples.len(), 2);
    // Independently expected kinds; accepting two arbitrary valid instances is
    // not proof that the actual landing and conformance downloads were checked.
    assert_eq!(examples[0]["title"], json!("Glaux Server"));
    assert_eq!(examples[0]["links"].as_array().unwrap().len(), 7);
    assert_eq!(examples[1]["conformsTo"], json!([]));
    assert_eq!(examples[1]["links"].as_array().unwrap().len(), 2);
    let denial = DenyRetrieval::default();
    let validator =
        compile(schema, denial.clone()).expect("downloaded discovery schema does not compile");
    for example in examples {
        assert!(
            validator.is_valid(example),
            "downloaded discovery example violates downloaded schema"
        );
        let mut missing = example.clone();
        missing.as_object_mut().unwrap().remove("links");
        assert!(
            !validator.is_valid(&missing),
            "downloaded schema accepts missing links"
        );
    }
    let mut unfinished = examples[1].clone();
    unfinished["conformsTo"] =
        json!(["http://www.opengis.net/spec/ogcapi-connectedsystems-1/1.0/conf/api-common"]);
    assert!(
        !validator.is_valid(&unfinished),
        "downloaded schema accepts an unfinished conformance class"
    );
    let mut invalid_schema = schema.clone();
    invalid_schema["type"] = json!("not-a-json-schema-type");
    assert!(
        compile(&invalid_schema, denial.clone()).is_err(),
        "schema compiler accepted invalid type keyword"
    );
    let mut broken_reference = schema.clone();
    broken_reference["$ref"] = json!("#/$defs/missing-fixture-definition");
    assert!(
        compile(&broken_reference, denial.clone()).is_err(),
        "schema compiler accepted unresolved local reference"
    );
    let mut external_schema = schema.clone();
    external_schema["$ref"] = json!("https://unreachable.invalid/no-network.json");
    assert!(
        compile(&external_schema, denial.clone()).is_err(),
        "reference preflight accepted remote retrieval"
    );
    assert_eq!(
        denial.0.load(Ordering::Relaxed),
        0,
        "ordinary schema proof attempted retrieval"
    );
    // Independently test the second barrier without bypassing the preflight in
    // the actual fixture path. This retriever never performs any I/O.
    assert!(
        jsonschema::options()
            .with_draft(jsonschema::Draft::Draft202012)
            .with_retriever(denial.clone())
            .build(&json!({"$ref":"https://unreachable.invalid/no-network.json"}))
            .is_err(),
        "denying retriever failed open"
    );
    assert!(
        denial.0.load(Ordering::Relaxed) > 0,
        "retriever denial control did not execute"
    );
    println!("{MARKER}");
}
