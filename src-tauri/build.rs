use std::fs;
use std::io::Write;
use std::path::PathBuf;

use object::write::Object;
use object::{Architecture, BinaryFormat, Endianness};

fn build_host_object_kind() -> Option<(&'static str, BinaryFormat, Architecture)> {
    if cfg!(all(target_arch = "aarch64", target_os = "macos")) {
        Some((
            "aarch64-apple-darwin",
            BinaryFormat::MachO,
            Architecture::Aarch64,
        ))
    } else if cfg!(all(target_arch = "x86_64", target_os = "macos")) {
        Some((
            "x86_64-apple-darwin",
            BinaryFormat::MachO,
            Architecture::X86_64,
        ))
    } else if cfg!(all(target_arch = "aarch64", target_os = "linux")) {
        Some((
            "aarch64-unknown-linux-gnu",
            BinaryFormat::Elf,
            Architecture::Aarch64,
        ))
    } else if cfg!(all(target_arch = "x86_64", target_os = "linux")) {
        Some((
            "x86_64-unknown-linux-gnu",
            BinaryFormat::Elf,
            Architecture::X86_64,
        ))
    } else if cfg!(all(target_arch = "aarch64", target_os = "windows")) {
        Some((
            "aarch64-pc-windows-msvc",
            BinaryFormat::Coff,
            Architecture::Aarch64,
        ))
    } else if cfg!(all(target_arch = "x86_64", target_os = "windows")) {
        Some((
            "x86_64-pc-windows-msvc",
            BinaryFormat::Coff,
            Architecture::X86_64,
        ))
    } else {
        None
    }
}

fn ensure_workflow_runner_binary_placeholder() {
    let Ok(manifest_dir) = fs::canonicalize(PathBuf::from(env!("CARGO_MANIFEST_DIR"))) else {
        return;
    };
    let Some((target_triple, binary_format, architecture)) = build_host_object_kind() else {
        return;
    };

    let binaries_dir = manifest_dir.join("binaries");
    // The manifest root is embedded by Cargo and `binaries` is a fixed child.
    // codeql[rust/path-injection]
    if fs::create_dir_all(&binaries_dir).is_err() {
        return;
    }
    // codeql[rust/path-injection]
    let Ok(binaries_dir) = fs::canonicalize(binaries_dir) else {
        return;
    };
    if !binaries_dir.starts_with(&manifest_dir) {
        return;
    }

    let binary_path = binaries_dir.join(format!("ralphx-workflow-runner-{target_triple}"));
    if binary_path.exists() {
        return;
    }

    let Ok(object_bytes) = Object::new(binary_format, architecture, Endianness::Little).write()
    else {
        return;
    };
    // Production build hooks replace this coverage-safe object with the real sidecar.
    // `create_new` refuses an existing file or symlink at the contained destination.
    // codeql[rust/path-injection]
    let Ok(mut destination) = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&binary_path)
    else {
        return;
    };
    if destination.write_all(&object_bytes).is_err() {
        drop(destination);
        // codeql[rust/path-injection]
        let _ = fs::remove_file(&binary_path);
        return;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        if let Ok(metadata) = destination.metadata() {
            let mut permissions = metadata.permissions();
            permissions.set_mode(0o755);
            let _ = destination.set_permissions(permissions);
        }
    }
}

fn main() {
    ensure_workflow_runner_binary_placeholder();
    tauri_build::build()
}
