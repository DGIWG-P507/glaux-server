use std::{env, fs, path::PathBuf};

fn main() {
    let root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("manifest directory"));
    let corpus = root.join("corpus");
    let manifest = corpus.join("manifest.json");
    println!("cargo:rerun-if-changed={}", manifest.display());
    let value: serde_json::Value =
        serde_json::from_slice(&fs::read(manifest).expect("corpus manifest"))
            .expect("manifest JSON");
    let mut generated = String::from("const DOCUMENTS: &[(&str, &str)] = &[\n");
    for artifact in value["artifacts"].as_array().expect("artifact inventory") {
        if artifact["kind"] != "schema" {
            continue;
        }
        let relative = artifact["path"].as_str().expect("artifact path");
        assert!(relative.starts_with("originals/"));
        assert!(
            relative
                .split('/')
                .all(|part| !part.is_empty() && part != ".." && part != ".")
        );
        let file = corpus.join(relative).canonicalize().expect("original file");
        assert!(file.starts_with(corpus.canonicalize().expect("corpus root")));
        println!("cargo:rerun-if-changed={}", file.display());
        for uri in std::iter::once(&artifact["uri"])
            .chain(artifact["aliases"].as_array().expect("aliases").iter())
        {
            generated.push_str(&format!(
                "({:?}, include_str!({:?})),\n",
                uri.as_str().expect("schema URI"),
                file.to_str().expect("UTF-8 source path")
            ));
        }
    }
    generated.push_str("];\n");
    fs::write(
        PathBuf::from(env::var("OUT_DIR").expect("build output")).join("corpus.rs"),
        generated,
    )
    .expect("generated embedded catalog");
}
