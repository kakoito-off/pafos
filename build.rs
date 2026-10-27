//! Builds the ring 3 programs and generates the program registry.
//!
//! Every folder in `programs/` with a `Cargo.toml` is a program, named after
//! the folder; it goes into the image when the kernel's feature of the same
//! name is on. Each one is built by a nested cargo in its own target
//! directory (`target/programs`), since this cargo holds the lock on the main
//! one. The ELF files land in `OUT_DIR`, together with `programs.rs`, the
//! registry that `src/programs.rs` includes.

use std::env;
use std::fmt::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    for path in ["abi", "libpafos", "programs", "Cargo.toml", "x86_64-pafos.json", ".cargo/config.toml"] {
        println!("cargo:rerun-if-changed={}", root.join(path).display());
    }

    let features = declared_features(&root.join("Cargo.toml"));
    let programs = find_programs(&root.join("programs"));
    for feature in &features {
        if feature != "default" && enabled(feature) && !programs.contains(feature) {
            panic!("feature `{feature}` is on, but there is no programs/{feature}");
        }
    }

    let mut registry = String::from("&[\n");
    for name in &programs {
        if !features.contains(name) {
            println!("cargo:warning=programs/{name} is not in the image: add `{name} = []` to [features] in Cargo.toml");
            continue;
        }
        if !enabled(name) {
            continue;
        }
        let elf = out_dir.join(name);
        build(&root, name, &elf);
        writeln!(registry, "    ({name:?}, include_bytes!({:?})),", elf.display().to_string()).unwrap();
    }
    registry.push_str("]\n");
    std::fs::write(out_dir.join("programs.rs"), registry).unwrap();
}

/// Folders of `programs/` that hold a crate, sorted by name.
fn find_programs(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .expect("no programs/ folder")
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.join("Cargo.toml").is_file())
        .map(|path| path.file_name().unwrap().to_str().expect("non-UTF-8 program name").to_owned())
        .collect();
    names.sort();
    names
}

/// Feature names from the `[features]` table of the kernel's manifest.
fn declared_features(manifest: &Path) -> Vec<String> {
    let text = std::fs::read_to_string(manifest).unwrap();
    text.lines()
        .map(str::trim)
        .skip_while(|line| *line != "[features]")
        .skip(1)
        .take_while(|line| !line.starts_with('['))
        .filter_map(|line| line.split_once('=').map(|(name, _)| name.trim().to_owned()))
        .filter(|name| !name.is_empty() && !name.starts_with('#'))
        .collect()
}

fn enabled(feature: &str) -> bool {
    env::var_os(format!("CARGO_FEATURE_{}", feature.to_uppercase().replace('-', "_"))).is_some()
}

fn build(root: &Path, name: &str, dest: &Path) {
    let target_dir = root.join("target").join("programs");
    let package = format!("pafos-{name}");
    let mut cargo = Command::new(env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
    cargo
        .current_dir(root)
        .args(["rustc", "--release", "--package", &package, "--bin", &package])
        .arg("--target-dir")
        .arg(&target_dir)
        .arg("--")
        .arg(format!("-Clink-arg=--image-base={:#x}", pafos_abi::layout::USER_BASE));
    // Settings meant for this build script must not leak into the nested build.
    for (key, _) in env::vars() {
        let leaks = (key.starts_with("CARGO_") && key != "CARGO_HOME")
            || key.starts_with("RUSTC")
            || matches!(
                key.as_str(),
                "RUSTFLAGS" | "RUSTDOC" | "OUT_DIR" | "TARGET" | "HOST" | "PROFILE" | "OPT_LEVEL" | "DEBUG" | "NUM_JOBS"
            );
        if leaks {
            cargo.env_remove(key);
        }
    }

    let status = cargo.status().expect("failed to run cargo for a program");
    assert!(status.success(), "building program `{name}` failed (is its package named `{package}`?)");
    let built = target_dir.join("x86_64-pafos").join("release").join(&package);
    std::fs::copy(&built, dest).unwrap_or_else(|e| panic!("copying {}: {e}", built.display()));
}
