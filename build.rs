use std::{env, fs, path::PathBuf};

fn main() {
    let directory = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("manifests");
    // Watch the directory for additions/removals, and each YAML for content edits.
    println!("cargo:rerun-if-changed={}", directory.display());
    let mut paths: Vec<_> = fs::read_dir(&directory)
        .expect("cannot read manifests directory")
        .map(|entry| entry.expect("cannot read manifest entry").path())
        .filter(|path| path.is_file() && path.extension().is_some_and(|ext| ext == "yaml"))
        .collect();
    paths.sort();

    let mut generated = String::from("pub(crate) const BUNDLED: &[(&str, &str)] = &[\n");
    for path in paths {
        println!("cargo:rerun-if-changed={}", path.display());
        let filename = path
            .file_name()
            .unwrap()
            .to_str()
            .expect("non-UTF8 manifest filename");
        let source = path.to_str().expect("non-UTF8 manifest path");
        generated.push_str(&format!("    ({filename:?}, include_str!({source:?})),\n"));
    }
    generated.push_str("];\n");
    let output = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("catalog.rs");
    fs::write(output, generated).expect("cannot write generated catalog");
}
