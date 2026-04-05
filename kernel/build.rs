use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("Failed to get manifest dir");
    let project_root = Path::new(&manifest_dir).parent().expect("Failed to find project root");
    let arch = env::var("CARGO_CFG_TARGET_ARCH").expect("Failed to get target arch");

    let linker_script = project_root.join(format!("linker-{}.ld", arch));
    if linker_script.exists() {
        println!("cargo:rustc-link-arg=-T{}", linker_script.display());
        println!("cargo:rerun-if-changed={}", linker_script.display());
    } else {
        println!("cargo:warning=Linker script not found at {}", linker_script.display());
    }

    let mut nasm_build = nasm_rs::Build::new();
    let mut found_asm = false;

    let asm_files = find_files_with_extension(project_root, "asm");

    for path in asm_files {
        println!("cargo:warning=Compiling assembly: {}", path.display());
        nasm_build.file(&path);
        println!("cargo:rerun-if-changed={}", path.display());
        found_asm = true;
    }

    if found_asm {
        let _ = nasm_build.compile("asmlib");
        println!("cargo:rustc-link-lib=static=asmlib");
    }

    println!("cargo:rerun-if-changed={}", project_root.display());
}

fn find_files_with_extension(dir: &Path, ext: &str) -> Vec<PathBuf> {
    let mut files = Vec::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                // Skip build artifacts and hidden directories
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if name == "target" || name.starts_with('.') { continue; }
                
                files.extend(find_files_with_extension(&path, ext));
            } else if path.extension().map_or(false, |e| e == ext) {
                files.push(path);
            }
        }
    }
    files
}
