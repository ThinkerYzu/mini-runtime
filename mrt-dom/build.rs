// Builds Firefox's HTML parser objects with ../parser/build.sh, packs them into one static archive
// and links it (plus the C++ runtime). The cc crate is not used so the build needs no downloads.
use std::path::PathBuf;
use std::process::Command;
#[path = "../tools/sdk/build.rs"]
mod sdk;

fn main() {
    let parser = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("../parser");
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());

    let engine = sdk::engine(&parser.join(".."));

    // a cross build (Android) keeps its objects apart from the host's
    let (target, host) = (std::env::var("TARGET").unwrap(), std::env::var("HOST").unwrap());
    let obj_dir = if target == host { out.join("parser") } else { out.join(format!("parser-{target}")) };
    let status = Command::new("sh").arg(parser.join("build.sh")).env("OBJ", &obj_dir).env("MRT_PARSER_INCLUDE", &engine.include).env("MRT_PARSER_CONFDEFS", &engine.confdefs).status().expect("run build.sh");
    assert!(status.success(), "parser/build.sh failed");

    let objs: Vec<PathBuf> = std::fs::read_dir(&obj_dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "o") && p.file_name().unwrap() != "test_sink.o")
        .collect();
    let archive = out.join("libmrtparser.a");
    let _ = std::fs::remove_file(&archive);
    let status = Command::new("ar").arg("crs").arg(&archive).args(&objs).status().expect("run ar");
    assert!(status.success(), "ar failed");

    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rustc-link-lib=static=mrtparser");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("android") {
        // the NDK's libc++_static.a (MRT_CXX_LIBDIR, set by mrt-js/tools/build-android.sh)
        println!("cargo:rerun-if-env-changed=MRT_CXX_LIBDIR");
        println!("cargo:rustc-link-search=native={}", std::env::var("MRT_CXX_LIBDIR").expect("MRT_CXX_LIBDIR"));
        println!("cargo:rustc-link-lib=static=c++_static");
        println!("cargo:rustc-link-lib=static=c++abi");
    } else {
        println!("cargo:rustc-link-lib=dylib=stdc++");
    }
    println!("cargo:rerun-if-env-changed=CXX");
    println!("cargo:rerun-if-changed={}", parser.join("shim").display());
    println!("cargo:rerun-if-changed={}", parser.join("vendor").display());
    println!("cargo:rerun-if-changed={}", parser.join("build.sh").display());
}
