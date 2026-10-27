//! Builds the ring 3 programs and leaves their ELF files in `OUT_DIR`,
//! where `src/programs.rs` embeds them with `include_bytes!`.
//!
//! Each program is built by a nested cargo in its own target directory
//! (`target/programs`), since this cargo holds the lock on the main one.

use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Programs built as executables; each sits behind a feature of the same name.
const PROGRAMS: &[&str] = &["calc", "time", "timer", "fault"];

fn main() {
    let root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    for path in ["abi", "libpafos", "programs", "x86_64-pafos.json", ".cargo/config.toml"] {
        println!("cargo:rerun-if-changed={}", root.join(path).display());
    }
    for name in PROGRAMS {
        if env::var_os(format!("CARGO_FEATURE_{}", name.to_uppercase())).is_some() {
            build(&root, name, &out_dir.join(name));
        }
    }
}

fn build(root: &Path, name: &str, dest: &Path) {
    let target_dir = root.join("target").join("programs");
    let mut cargo = Command::new(env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
    cargo
        .current_dir(root)
        .args(["rustc", "--release", "--package", &format!("pafos-{name}"), "--bin", name, "--features", "rt"])
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
    assert!(status.success(), "building program `{name}` failed");
    let built = target_dir.join("x86_64-pafos").join("release").join(name);
    std::fs::copy(&built, dest).unwrap_or_else(|e| panic!("copying {}: {e}", built.display()));
}
