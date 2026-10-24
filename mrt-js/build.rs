// Compiles glue/mrt_js.cpp against the pinned SDK (or explicit maintainer source build)
// and links its matching shared libmozjs. The shared library, not libjs_static.a: the static archive
// carries SpiderMonkey's own Rust code with its own copy of the Rust standard library, which clashes
// with ours in one binary. See docs/build.md for SDK selection.
use std::path::{Path, PathBuf};
use std::process::Command;

#[path = "../tools/sdk/build.rs"]
mod sdk;

fn compiler() -> String {
    std::env::var("CXX").unwrap_or_else(|_| "c++".to_string())
}

fn main() {
    let dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let engine = sdk::engine(&dir.join(".."));
    let include = &engine.include;
    let bin = &engine.lib;
    let libname = &engine.name;
    let objdir = std::env::var("MRT_JS_OBJDIR").unwrap_or_default();

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=glue/mrt_js.cpp");
    println!("cargo:rerun-if-changed=glue/mrt_js.h");
    println!("cargo:rerun-if-env-changed=MRT_JS_OBJDIR");
    println!("cargo:rerun-if-env-changed=CXX");

    let obj = out.join("mrt_js.o");
    let android = std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("android");
    // brotli's headers: the system's on the host, the tree's own copy (built by build-android.sh) on Android
    let brotli_include: Vec<String> = if android {
        vec!["-isystem".into(), Path::new(&objdir).join("../modules/brotli/include").display().to_string()]
    } else {
        vec![]
    };
    let status = Command::new(compiler())
        .args(["-std=gnu++20", "-O2", "-fPIC", "-fno-exceptions", "-fno-rtti", "-Wall", "-Wno-unused-parameter"])
        .args(&brotli_include)
        .arg("-isystem").arg(&include)
        // the defines the tree's own js build compiles with (XP_UNIX, XP_LINUX, ...)
        .arg("-include").arg(&engine.confdefs)
        .arg(format!("-I{}", dir.join("glue").display()))
        // the tree's zlib.h, whose zconf.h includes mozzconf.h: zlib's functions as libz-rs-sys's
        // MOZ_Z_ symbols, as Gecko's C++ calls them
        .arg("-iquote").arg(dir.join("../vendor/zlib-headers"))
        .arg("-c").arg(dir.join("glue/mrt_js.cpp")).arg("-o").arg(&obj)
        .status()
        .expect("run the C++ compiler");
    assert!(status.success(), "compiling glue/mrt_js.cpp failed");
    let archive = out.join("libmrt_js_glue.a");
    let _ = std::fs::remove_file(&archive);
    assert!(Command::new("ar").arg("crs").arg(&archive).arg(&obj).status().expect("ar").success());

    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rustc-link-lib=static=mrt_js_glue");
    println!("cargo:rustc-link-search=native={}", bin.display());
    println!("cargo:rustc-link-lib=dylib={libname}");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("android") {
        // on the phone libmozjs sits next to the binary; the NDK's static libc++, and brotli as
        // static archives built from the tree's modules/brotli (MRT_BROTLI_LIB, IMPLEMENTATION-GUIDE)
        println!("cargo:rustc-link-arg=-Wl,-rpath,$ORIGIN");
        // the NDK's libc++_static.a (MRT_CXX_LIBDIR, set by mrt-js/tools/build-android.sh)
        println!("cargo:rerun-if-env-changed=MRT_CXX_LIBDIR");
        println!("cargo:rustc-link-search=native={}", std::env::var("MRT_CXX_LIBDIR").expect("MRT_CXX_LIBDIR"));
        println!("cargo:rustc-link-lib=static=c++_static");
        println!("cargo:rustc-link-lib=static=c++abi");
        println!("cargo:rerun-if-env-changed=MRT_BROTLI_LIB");
        let brotli = std::env::var("MRT_BROTLI_LIB").expect("MRT_BROTLI_LIB: brotli's static archives for Android");
        println!("cargo:rustc-link-search=native={brotli}");
        for lib in ["brotlienc", "brotlidec", "brotlicommon"] {
            println!("cargo:rustc-link-lib=static={lib}");
        }
        return;
    }
    if engine.sdk {
        // Binaries in release/ and tests in release/deps/ share a relocatable lib/ directory.
        let profile = out.ancestors().nth(3).expect("Cargo profile directory");
        let libs = profile.join("lib");
        std::fs::create_dir_all(&libs).unwrap();
        let soname = format!("lib{libname}.so");
        let staged = out.join(&soname);
        std::fs::copy(bin.join(&soname), &staged).expect("stage SDK engine library");
        std::fs::rename(staged, libs.join(&soname)).expect("install SDK engine library atomically");
        println!("cargo:rustc-link-arg=-Wl,-rpath,$ORIGIN/lib:$ORIGIN/../lib");
    } else {
        println!("cargo:rustc-link-arg=-Wl,-rpath,{}", bin.display());
    }
    println!("cargo:rustc-link-lib=dylib=stdc++");
    // CompressionStream's brotli (dom/compression); its zlib is libz-rs-sys, a dependency
    for lib in ["brotlienc", "brotlidec"] {
        println!("cargo:rustc-link-lib=dylib={lib}");
    }
}
