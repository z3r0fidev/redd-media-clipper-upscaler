//! Links the prebuilt ncnn (static, Vulkan) fetched by `cargo xtask fetch-ncnn` into
//! `third_party/ncnn/<target>/`, and compiles the small C++ shim for what ncnn's C API lacks
//! (GPU listing, device type, heap budget). The C/C++ runtimes are linked statically on Linux so
//! the shipped worker needs only glibc; Vulkan itself is loaded at run time by ncnn.

use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let target = std::env::var("TARGET").expect("TARGET");
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let root = manifest.parent().and_then(Path::parent).expect("workspace root");
    let ncnn = std::env::var_os("RMEDIA_NCNN_DIR")
        .map_or_else(|| root.join("third_party/ncnn").join(&target), PathBuf::from);
    println!("cargo:rerun-if-env-changed=RMEDIA_NCNN_DIR");
    println!("cargo:rerun-if-changed=src/shim.cpp");
    println!("cargo:rerun-if-changed={}", ncnn.join(".sha256").display());

    // The Windows packages carry one folder per architecture (x64/, x86/, arm64/…).
    let base = if ncnn.join("x64/lib").is_dir() { ncnn.join("x64") } else { ncnn.clone() };
    let (include, lib) = (base.join("include"), base.join("lib"));
    if !include.join("ncnn/c_api.h").is_file() {
        panic!(
            "\n\nncnn for {target} not found in {}.\nrun `cargo xtask fetch-ncnn{}` first (or set RMEDIA_NCNN_DIR).\n\n",
            ncnn.display(),
            if target.contains("windows") { " --target x86_64-pc-windows-msvc" } else { "" }
        );
    }

    let mut shim = cc::Build::new();
    shim.cpp(true).file("src/shim.cpp").include(&include).warnings(false);
    if target.contains("msvc") {
        // The prebuilt ncnn.lib uses the DLL C runtime (/MD), like Rust's own default; mixing in
        // the static one fails to link (LNK2038 RuntimeLibrary mismatch).
        shim.flag("/std:c++17").static_crt(false);
    } else {
        shim.flag("-std=c++17").cpp_link_stdlib(None);
    }
    shim.compile("rmedia_sr_shim");

    if target.contains("msvc") {
        // Every static library of the package (ncnn and glslang's parts); OpenMP from the MSVC
        // runtime (vcomp).
        println!("cargo:rustc-link-search=native={}", lib.display());
        let mut libs: Vec<String> = std::fs::read_dir(&lib)
            .expect("ncnn lib dir")
            .flatten()
            .filter_map(|e| e.file_name().to_str().and_then(|n| n.strip_suffix(".lib")).map(str::to_owned))
            .collect();
        libs.sort_by_key(|n| (n != "ncnn", n.clone()));
        for l in libs {
            println!("cargo:rustc-link-lib=static={l}");
        }
        println!("cargo:rustc-link-lib=dylib=vcomp");
    } else {
        // ncnn first, then glslang's parts, in one group (they refer to each other), then the
        // static C++ and OpenMP runtimes.
        let mut group = vec!["-Wl,--start-group".to_owned()];
        for l in [
            "libncnn.a",
            "libglslang.a",
            "libSPIRV.a",
            "libMachineIndependent.a",
            "libOSDependent.a",
            "libGenericCodeGen.a",
            "libglslang-default-resource-limits.a",
        ] {
            let p = lib.join(l);
            if p.is_file() {
                group.push(p.display().to_string());
            }
        }
        for l in ["libgomp.a", "libstdc++.a"] {
            group.push(runtime_lib(l).unwrap_or_else(|| format!("-l{}", &l[3..l.len() - 2])));
        }
        group.push("-Wl,--end-group".to_owned());
        for a in group {
            println!("cargo:rustc-link-arg-bins={a}");
        }
        for l in ["dl", "pthread", "m"] {
            println!("cargo:rustc-link-arg-bins=-l{l}");
        }
    }
}

/// The full path of a static runtime library from the C++ compiler, when it has one.
fn runtime_lib(name: &str) -> Option<String> {
    let cxx = std::env::var("CXX").unwrap_or_else(|_| "c++".to_owned());
    let out = Command::new(cxx).arg(format!("-print-file-name={name}")).output().ok()?;
    let path = String::from_utf8(out.stdout).ok()?.trim().to_owned();
    Path::new(&path).is_absolute().then_some(path)
}
