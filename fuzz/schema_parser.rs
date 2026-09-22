//! Bounded deterministic mutation-fuzz driver (not coverage-guided libFuzzer).
//!
//! Constructive verdicts come from the published fixture expectations and the
//! Quantity required-label rules. Generated input is never parsed through the
//! production parser, or serde_json::Value, to manufacture its expected answer.
use glaux_standards::validation::{Contract, Failure, MAX_BYTES, MAX_DEPTH, StructuralValidator};
use std::collections::BTreeSet;
use std::time::{Duration, Instant};

const SEED: u64 = 0x475c_0008_0000_0001;
const QUANTITY: &[u8] =
    include_bytes!("../crates/glaux-standards/corpus/fixtures/quantity-labelled.json");
const MISSING_LABEL: &[u8] = include_bytes!("corpus/schema-parser/missing-label.json");
const DUPLICATE_LABEL: &[u8] = include_bytes!("corpus/schema-parser/duplicate-label.json");
const WRAPPERS: [(Contract, &[u8]); 4] = [
    (
        Contract::ObservationSwe,
        include_bytes!("../crates/glaux-standards/corpus/fixtures/quantity-wrapper-labelled.json"),
    ),
    (
        Contract::ObservationSwe,
        include_bytes!("../crates/glaux-standards/corpus/fixtures/binary-wrapper-valid.json"),
    ),
    (
        Contract::SweRecord,
        include_bytes!("../crates/glaux-standards/corpus/fixtures/swe-recursive.json"),
    ),
    (
        Contract::PhysicalSystem,
        include_bytes!("../crates/glaux-standards/corpus/fixtures/sensorml-recursive.json"),
    ),
];
const PARTITIONS: [&str; 8] = [
    "valid-quantity-variants",
    "valid-recursive-wrapper-variants",
    "invalid-required-label-variants",
    "invalid-nested-label-or-encoding",
    "malformed-valid-seeds",
    "byte-mutations-of-valid-seeds",
    "byte-mutations-of-invalid-seeds",
    "concrete-regressions",
];

struct Case {
    contract: Contract,
    bytes: Vec<u8>,
    expected: Option<Result<(), Failure>>,
}

fn random(state: &mut u64) -> u64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}

fn label(round: usize) -> String {
    match round % 4 {
        0 => format!(r#""Temperature-{round}""#),
        1 => format!(r#""Température-{round}""#),
        2 => format!(r#""Quoted\"label-{round}""#),
        _ => format!(r#""\u0054emperature-{round}""#),
    }
}

fn quantity(round: usize, bits: u64, label: Option<&str>) -> Vec<u8> {
    // Only nonempty string labels are valid. Unknown extension members remain
    // instance data, including strings that resemble schema retrieval requests.
    let mut members = vec![
        r#""type":"Quantity""#.to_owned(),
        format!(r#""definition":"urn:glaux:fuzz:quantity:{round}""#),
        r#""uom":{"code":"K"}"#.to_owned(),
        format!(
            r#""extension":{{"case":{round},"$ref":"http://127.0.0.1:9/canary","nested":[true,null,{{"href":"file:///not-a-schema"}}]}}"#
        ),
    ];
    if let Some(label) = label {
        members.push(format!(r#""label":{label}"#));
    }
    let rotation = bits as usize % members.len();
    members.rotate_left(rotation);
    if bits & 1 == 0 {
        members.reverse();
    }
    let separator = if bits & 2 == 0 { "," } else { ",\n\t" };
    format!(" \n{{{}}}\t ", members.join(separator)).into_bytes()
}

fn wrapper(round: usize, replacement: &str) -> (Contract, Vec<u8>) {
    let (contract, seed) = WRAPPERS[round % WRAPPERS.len()];
    let source = std::str::from_utf8(seed).expect("UTF-8 source fixture");
    assert_eq!(source.matches("\"Temperature\"").count(), 1);
    let changed = source.replace("\"Temperature\"", replacement).replace(
        "urn:glaux:fixture:temperature",
        &format!("urn:glaux:fuzz:temperature:{round}"),
    );
    (contract, format!("\t{changed}\n ").into_bytes())
}

fn valid_seed(index: usize) -> (Contract, &'static [u8]) {
    if index.is_multiple_of(5) {
        (Contract::Quantity, QUANTITY)
    } else {
        WRAPPERS[index % 5 - 1]
    }
}

fn malformed(seed: &[u8], operation: usize) -> Vec<u8> {
    let end = seed
        .iter()
        .rposition(|byte| !byte.is_ascii_whitespace())
        .expect("nonempty seed");
    let mut bytes = seed[..=end].to_vec();
    match operation % 4 {
        0 => {
            assert_eq!(bytes.pop(), Some(b'}'));
        }
        1 => bytes.push(0),
        2 => bytes.insert(0, 0xff),
        _ => bytes.extend_from_slice(b" false"),
    }
    bytes
}

fn mutate(seed: &[u8], bits: u64, operation: usize) -> Vec<u8> {
    let mut bytes = seed.to_vec();
    let at = bits as usize % bytes.len();
    match operation % 6 {
        0 => bytes[at] ^= ((bits >> 32) as u8).max(1),
        1 => bytes.truncate(at),
        2 => bytes.insert(at, (bits >> 24) as u8),
        3 => {
            bytes.remove(at);
        }
        4 => {
            let length = (1 + (bits >> 16) as usize % 16).min(bytes.len() - at);
            let repeated = bytes[at..at + length].to_vec();
            drop(bytes.splice(at..at, repeated));
        }
        _ => {
            let token: &[u8] = match bits % 4 {
                0 => b"\\",
                1 => b"\"$ref\"",
                2 => b"null",
                _ => b"\xff]",
            };
            drop(bytes.splice(at..at, token.iter().copied()));
        }
    }
    assert_ne!(bytes, seed, "mutation left its source seed unchanged");
    bytes
}

fn regression(round: usize) -> (Vec<u8>, Failure) {
    match round % 8 {
        0 => (MISSING_LABEL.to_vec(), Failure::Structure),
        1 => (DUPLICATE_LABEL.to_vec(), Failure::DuplicateKey),
        2 => (
            br#"{"label":"a","l\u0061bel":"b"}"#.to_vec(),
            Failure::DuplicateKey,
        ),
        3 => (malformed(QUANTITY, 3), Failure::Malformed),
        4 => (b"\"\\u12".to_vec(), Failure::Malformed),
        // An object with serde's private number marker is still a JSON object,
        // not a number satisfying Quantity.value. The expected type comes from
        // Quantity.json / basicTypes.json, not serde's Value deserializer.
        5 => (
            br#"{"type":"Quantity","definition":"urn:fixture:q","label":"valid","uom":{"code":"K"},"value":{"$serde_json::private::Number":"1"}}"#.to_vec(),
            Failure::Structure,
        ),
        6 => (b"{\"label\":\"\xff\"}".to_vec(), Failure::Malformed),
        _ => (vec![b'['; MAX_DEPTH + 1], Failure::Depth),
    }
}

fn generate(partition: usize, round: usize, bits: u64) -> Case {
    let (contract, bytes, expected) = match partition {
        0 => (
            Contract::Quantity,
            quantity(round, bits, Some(&label(round))),
            Some(Ok(())),
        ),
        1 => {
            let (contract, bytes) = wrapper(round, &label(round));
            (contract, bytes, Some(Ok(())))
        }
        2 => {
            let invalid_label = [None, Some("null"), Some("\"\""), Some("17"), Some("{}")];
            (
                Contract::Quantity,
                quantity(round, bits, invalid_label[round % invalid_label.len()]),
                Some(Err(Failure::Structure)),
            )
        }
        3 => {
            let (contract, mut bytes) = wrapper(round, ["null", "\"\"", "false", "17"][round % 4]);
            if round % 8 == 1 {
                let (_, valid) = wrapper(round, &label(round));
                let valid = String::from_utf8(valid).expect("UTF-8 source variant");
                assert_eq!(valid.matches("\"BinaryEncoding\"").count(), 1);
                bytes = valid
                    .replace("\"BinaryEncoding\"", "\"JSONEncoding\"")
                    .into_bytes();
            }
            (contract, bytes, Some(Err(Failure::Structure)))
        }
        4 => {
            let (contract, seed) = if round.is_multiple_of(5) {
                (Contract::Quantity, quantity(round, bits, Some(&label(round))))
            } else {
                wrapper(round, &label(round))
            };
            (contract, malformed(&seed, round), Some(Err(Failure::Malformed)))
        }
        5 => {
            let (contract, seed) = valid_seed(round);
            (contract, mutate(seed, bits, round), None)
        }
        6 => {
            let seeds: [&[u8]; 4] = [MISSING_LABEL, DUPLICATE_LABEL, b"{broken", b"[1,]"];
            let seed = seeds[round % seeds.len()];
            (Contract::Quantity, mutate(seed, bits, round), None)
        }
        7 => {
            let (bytes, failure) = regression(round);
            (Contract::Quantity, bytes, Some(Err(failure)))
        }
        _ => unreachable!("fixed campaign partition"),
    };
    Case {
        contract,
        bytes,
        expected,
    }
}

fn main() {
    let start = Instant::now();
    let validator = StructuralValidator::new().expect("fixed offline validator");
    let mut state = SEED;
    let mut outcomes = [[0_usize; 2]; 8];
    let mut distinct = BTreeSet::new();
    let mut distinct_accepted = BTreeSet::new();
    for index in 0..1024 {
        assert!(
            start.elapsed() < Duration::from_secs(30),
            "bounded fuzz budget exceeded"
        );
        let partition = index % PARTITIONS.len();
        let case = generate(partition, index / PARTITIONS.len(), random(&mut state));
        assert!(
            case.bytes.len() + 4 <= MAX_BYTES,
            "generator exceeded its input budget"
        );
        let result = validator.validate(case.contract, &case.bytes);
        assert_eq!(
            result,
            validator.validate(case.contract, &case.bytes),
            "nondeterministic case {index}: {:?}",
            case.bytes
        );
        if let Some(expected) = case.expected {
            assert_eq!(
                result, expected,
                "source-derived case {index}: {:?}",
                case.bytes
            );
        }
        // JSON's insignificant outer whitespace cannot change acceptance. This
        // metamorphic check does not share the parser's internal representation.
        let mut spaced = Vec::with_capacity(case.bytes.len() + 4);
        spaced.extend_from_slice(b" \n");
        spaced.extend_from_slice(&case.bytes);
        spaced.extend_from_slice(b"\t ");
        assert_eq!(
            result.is_ok(),
            validator.validate(case.contract, &spaced).is_ok(),
            "outer whitespace changed acceptance for case {index}"
        );
        let outcome = usize::from(result.is_err());
        outcomes[partition][outcome] += 1;
        if result.is_ok() {
            distinct_accepted.insert((case.contract, case.bytes.clone()));
        }
        distinct.insert((case.contract, case.bytes));
    }
    for (name, [accepted, rejected]) in PARTITIONS.iter().zip(outcomes) {
        assert_eq!(
            accepted + rejected,
            128,
            "partition did not execute: {name}"
        );
        println!("Fuzz partition {name}: accepted={accepted}; rejected={rejected}");
    }
    let accepted: usize = outcomes.iter().map(|counts| counts[0]).sum();
    let rejected: usize = outcomes.iter().map(|counts| counts[1]).sum();
    assert!(
        accepted >= 256 && rejected >= 512,
        "expected verdict partitions did not execute"
    );
    assert!(
        distinct_accepted.len() >= 256,
        "accepted seeds were repeated unchanged"
    );
    assert!(distinct.len() >= 640, "mutation diversity collapsed");
    assert_eq!(MAX_DEPTH, 32, "reviewed parser depth budget drifted");
    assert_eq!(
        validator.validate(Contract::Quantity, &vec![b' '; MAX_BYTES + 1]),
        Err(Failure::Size)
    );
    assert_eq!(
        validator.validate(Contract::Quantity, &vec![b'['; MAX_DEPTH + 1]),
        Err(Failure::Depth)
    );
    assert!(
        start.elapsed() < Duration::from_secs(30),
        "bounded fuzz budget exceeded"
    );
    println!(
        "Schema-parser fuzz v2: 1024 cases; seed=0x{SEED:016x}; accepted={accepted}; rejected={rejected}; distinct={}; distinct_accepted={}; source-derived verdicts, determinism, required labels, nested encoding binding, whitespace and explicit bounds checked.",
        distinct.len(),
        distinct_accepted.len()
    );
    println!("Required schema-parser fuzz invariants passed: 1024 cases.");
}
