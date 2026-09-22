//! Bounded deterministic mutation-fuzz driver (not coverage-guided libFuzzer).
use glaux_standards::validation::{Contract, Failure, MAX_BYTES, MAX_DEPTH, StructuralValidator};
use std::time::{Duration, Instant};

fn main() {
    let start = Instant::now();
    let validator = StructuralValidator::new().expect("fixed offline validator");
    let seeds: [&[u8]; 4] = [
        include_bytes!("../crates/glaux-standards/corpus/fixtures/quantity-labelled.json"),
        include_bytes!("corpus/schema-parser/missing-label.json"),
        include_bytes!("corpus/schema-parser/duplicate-label.json"),
        b"{broken",
    ];
    let mut state: u64 = 0x475c_0008_0000_0001;
    let mut accepted = 0;
    let mut rejected = 0;
    for index in 0..1024 {
        assert!(
            start.elapsed() < Duration::from_secs(30),
            "bounded fuzz budget exceeded"
        );
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let mut bytes = seeds[index % seeds.len()].to_vec();
        if index % 4 != 0 {
            let at = state as usize % bytes.len();
            match index % 3 {
                0 => {
                    bytes[at] ^= (state >> 32) as u8;
                }
                1 => {
                    bytes.truncate(at);
                }
                _ => {
                    bytes.insert(at, (state >> 24) as u8);
                }
            }
        }
        let result = validator.validate(Contract::Quantity, &bytes);
        assert_eq!(
            result,
            validator.validate(Contract::Quantity, &bytes),
            "nondeterministic input {bytes:?}"
        );
        if result.is_ok() {
            accepted += 1;
            let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(value["type"], "Quantity");
            assert!(value["label"].as_str().is_some_and(|s| !s.is_empty()));
            assert!(value.get("definition").is_some() && value.get("uom").is_some());
        } else {
            rejected += 1;
        }
    }
    assert!(
        accepted >= 256 && rejected > 0,
        "mutation partitions did not execute"
    );
    assert_eq!(
        validator.validate(Contract::Quantity, &vec![b' '; MAX_BYTES + 1]),
        Err(Failure::Size)
    );
    assert_eq!(
        validator.validate(Contract::Quantity, &vec![b'['; MAX_DEPTH + 1]),
        Err(Failure::Depth)
    );
    println!(
        "Schema-parser fuzz v1: 1024 cases; seed=0x475c000800000001; accepted={accepted}; rejected={rejected}; deterministic verdict, required fields and explicit bounds checked."
    );
    println!("Required schema-parser fuzz invariants passed: 1024 cases.");
}
