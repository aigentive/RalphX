use std::env;
use std::fs;
use std::io;
use std::path::PathBuf;

fn build_host_target_triple() -> Option<&'static str> {
    if cfg!(all(target_arch = "aarch64", target_os = "macos")) {
        Some("aarch64-apple-darwin")
    } else if cfg!(all(target_arch = "x86_64", target_os = "macos")) {
        Some("x86_64-apple-darwin")
    } else if cfg!(all(target_arch = "aarch64", target_os = "linux")) {
        Some("aarch64-unknown-linux-gnu")
    } else if cfg!(all(target_arch = "x86_64", target_os = "linux")) {
        Some("x86_64-unknown-linux-gnu")
    } else if cfg!(all(target_arch = "aarch64", target_os = "windows")) {
        Some("aarch64-pc-windows-msvc")
    } else if cfg!(all(target_arch = "x86_64", target_os = "windows")) {
        Some("x86_64-pc-windows-msvc")
    } else {
        None
    }
}

fn ensure_workflow_runner_binary_placeholder() {
    let Ok(manifest_dir) = fs::canonicalize(PathBuf::from(env!("CARGO_MANIFEST_DIR"))) else {
        return;
    };
    let Some(target_triple) = build_host_target_triple() else {
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

    let Ok(build_script_binary) = env::current_exe() else {
        return;
    };
    // `current_exe` is process-owned and the destination is a contained, fixed entry.
    // `create_new` also refuses an existing file or symlink at the destination.
    // codeql[rust/path-injection]
    let Ok(mut source) = fs::File::open(build_script_binary) else {
        return;
    };
    // codeql[rust/path-injection]
    let Ok(mut destination) = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&binary_path)
    else {
        return;
    };
    if io::copy(&mut source, &mut destination).is_err() {
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
