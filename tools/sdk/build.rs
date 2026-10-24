// Shared SDK selection for the JS and parser build scripts.
use std::path::{Path, PathBuf};
use std::process::Command;

#[allow(dead_code)] // parser consumes only headers from the shared selection
pub struct Engine {
    pub include: PathBuf,
    pub lib: PathBuf,
    pub confdefs: PathBuf,
    pub name: String,
    pub sdk: bool,
}

pub fn engine(root: &Path) -> Engine {
    for key in ["MRT_SDK_DIR", "MRT_ENGINE_SOURCE", "MRT_JS_OBJDIR", "JS", "CXX", "PYTHON"] {
        println!("cargo:rerun-if-env-changed={key}");
    }
    println!("cargo:rerun-if-changed={}", root.join("tools/sdk").display());
    if let Some(sdk) = std::env::var_os("MRT_SDK_DIR") {
        assert!(std::env::var("MRT_ENGINE_SOURCE").as_deref() != Ok("1"),
                "choose either MRT_SDK_DIR or MRT_ENGINE_SOURCE=1, not both");
        let sdk = PathBuf::from(sdk);
        let output = Command::new(std::env::var("PYTHON").unwrap_or_else(|_| "python3".into()))
            .arg(root.join("tools/sdk/sdk.py")).arg("validate").arg(&sdk)
            .arg("--target").arg(std::env::var("TARGET").unwrap())
            .arg("--build-fields").output().expect("Python 3 is required to validate MRT_SDK_DIR");
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        let fields = String::from_utf8(output.stdout).unwrap();
        let fields: Vec<_> = fields.lines().collect();
        assert_eq!(fields.len(), 4, "SDK validator returned invalid fields");
        watch(&sdk);
        return Engine { include: fields[0].into(), lib: fields[1].into(),
                        confdefs: fields[2].into(), name: fields[3].into(), sdk: true };
    }
    assert!(std::env::var("MRT_ENGINE_SOURCE").as_deref() == Ok("1"),
            "set MRT_SDK_DIR to the pinned SDK (see docs/build.md); maintainers may explicitly select MRT_ENGINE_SOURCE=1 and MRT_JS_OBJDIR");
    let obj = PathBuf::from(std::env::var_os("MRT_JS_OBJDIR").or_else(|| std::env::var_os("JS"))
        .expect("MRT_ENGINE_SOURCE=1 requires MRT_JS_OBJDIR (or JS)"));
    let lib = obj.join("dist/bin");
    let names: Vec<_> = std::fs::read_dir(&lib).expect("read engine dist/bin").filter_map(|e| {
        let name = e.ok()?.file_name().into_string().ok()?;
        (name.starts_with("libmozjs-") && name.ends_with(".so")).then_some(name)
    }).collect();
    assert_eq!(names.len(), 1, "expected exactly one shared libmozjs in dist/bin");
    Engine { include: obj.join("dist/include"), confdefs: obj.join("js/src/js-confdefs.h"),
             lib, name: names[0][3..names[0].len()-3].into(), sdk: false }
}

fn watch(path: &Path) {
    println!("cargo:rerun-if-changed={}", path.display());
    if path.is_dir() {
        for entry in std::fs::read_dir(path).unwrap() { watch(&entry.unwrap().path()); }
    }
}
