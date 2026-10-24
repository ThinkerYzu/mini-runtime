// Compiles Firefox's HarfBuzz (vendor/harfbuzz, the files in sources.txt) and shim/mrt_text.cc into a
// static library, by calling the C++ compiler directly (no `cc` crate: the parser build in
// ../parser/build.sh works the same way). The defines are the ones in gfx/harfbuzz/src/moz.build,
// except that HarfBuzz's own Unicode tables (hb-ucd.cc) are used instead of Firefox's intl component.
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, Mutex};

fn compiler() -> String {
    if let Ok(c) = std::env::var("CXX") {
        return c;
    }
    "c++".to_string()
}

fn main() {
    let dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let hb = dir.join("vendor/harfbuzz");
    let list = std::fs::read_to_string(dir.join("sources.txt")).unwrap();
    let mut sources: Vec<PathBuf> = list.lines().filter(|l| !l.is_empty()).map(|l| hb.join(l)).collect();
    sources.push(dir.join("shim/mrt_text.cc"));

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=sources.txt");
    println!("cargo:rerun-if-changed=shim");
    println!("cargo:rerun-if-changed=vendor/harfbuzz");
    println!("cargo:rerun-if-env-changed=CXX");

    let cxx = compiler();
    let flags: Vec<String> = [
        "-std=c++17", "-O2", "-fPIC", "-fno-exceptions", "-fno-rtti", "-fno-threadsafe-statics",
        "-fvisibility=hidden", "-w", "-pthread",
        "-DPACKAGE_VERSION=\"moz\"", "-DPACKAGE_BUGREPORT=\"http://bugzilla.mozilla.org/\"",
        "-DHAVE_OT=1", "-DHAVE_ROUND=1", "-DHB_NO_BUFFER_VERIFY", "-DHB_NO_FALLBACK_SHAPE",
        "-DHB_NO_PAINT", "-DHB_NO_VAR_COMPOSITES",
        "-DHAVE_MMAP=1", "-DHAVE_SYS_MMAN_H=1", "-DHAVE_UNISTD_H=1",
    ]
    .iter().map(|s| s.to_string()).collect();
    let includes = [format!("-I{}", hb.display()), format!("-I{}", dir.join("shim").display())];

    // Compile in parallel: a worker pool pulling from a shared list.
    let queue = Arc::new(Mutex::new(sources.clone()));
    let objects = Arc::new(Mutex::new(Vec::<PathBuf>::new()));
    let failed = Arc::new(Mutex::new(false));
    let nthreads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
    let mut handles = vec![];
    for _ in 0..nthreads {
        let (queue, objects, failed) = (queue.clone(), objects.clone(), failed.clone());
        let (cxx, flags, includes, out) = (cxx.clone(), flags.clone(), includes.clone(), out.clone());
        handles.push(std::thread::spawn(move || loop {
            let src = match queue.lock().unwrap().pop() { Some(s) => s, None => break };
            let obj = out.join(format!("{}.o", src.file_stem().unwrap().to_string_lossy()));
            let status = Command::new(&cxx).args(&flags).args(&includes).arg("-c").arg(&src).arg("-o").arg(&obj).status();
            match status {
                Ok(s) if s.success() => objects.lock().unwrap().push(obj),
                _ => { eprintln!("failed to compile {}", src.display()); *failed.lock().unwrap() = true; }
            }
        }));
    }
    for h in handles { h.join().unwrap(); }
    if *failed.lock().unwrap() { panic!("C++ compile failed"); }

    let lib = out.join("libmrt_text_hb.a");
    let _ = std::fs::remove_file(&lib);
    let mut objs = objects.lock().unwrap().clone();
    objs.sort();
    let status = Command::new("ar").arg("crs").arg(&lib).args(&objs).status().expect("ar");
    assert!(status.success(), "ar failed");
    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rustc-link-lib=static=mrt_text_hb");
    // the NDK's static libc++ on Android, as libmozjs links its own
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("android") {
        // the NDK's libc++_static.a (MRT_CXX_LIBDIR, set by mrt-js/tools/build-android.sh)
        println!("cargo:rerun-if-env-changed=MRT_CXX_LIBDIR");
        println!("cargo:rustc-link-search=native={}", std::env::var("MRT_CXX_LIBDIR").expect("MRT_CXX_LIBDIR"));
        println!("cargo:rustc-link-lib=static=c++_static");
        println!("cargo:rustc-link-lib=static=c++abi");
    } else {
        println!("cargo:rustc-link-lib=stdc++");
    }
}
