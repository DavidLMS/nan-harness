use std::{env, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=native");
    println!("cargo:rerun-if-env-changed=NAN_DESKTOP_BUILD_PYTHON");
    println!("cargo:rerun-if-env-changed=NAN_DESKTOP_CMAKE");
    assert_eq!(
        env::var_os("HOST"),
        env::var_os("TARGET"),
        "the Desktop visual helper must be built on its native target"
    );
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo output directory"));
    let python = env::var_os("NAN_DESKTOP_BUILD_PYTHON")
        .unwrap_or_else(|| if cfg!(windows) { "python" } else { "python3" }.into());
    let status = Command::new(python)
        .args(["native/build.py", "--output"])
        .arg(&output)
        .env_remove("NAN_API_KEY")
        .status()
        .expect("Python 3 and CMake are required for Desktop visual checks");
    assert!(status.success(), "Desktop native helper build failed");
    let helper = output.join(if cfg!(windows) {
        "nanh-desktop-native.exe"
    } else {
        "nanh-desktop-native"
    });
    println!(
        "cargo:rustc-env=NAN_DESKTOP_NATIVE_HELPER={}",
        helper.display()
    );
    println!(
        "cargo:rustc-env=NAN_DESKTOP_OCR_MODEL={}",
        output.join("eng.traineddata").display()
    );
}
