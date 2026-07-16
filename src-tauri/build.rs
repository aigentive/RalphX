use std::env;
use std::fs;
use std::path::PathBuf;

fn ensure_workflow_runner_binary_placeholder() {
    let Ok(manifest_dir) = env::var("CARGO_MANIFEST_DIR") else {
        return;
    };
    let Ok(target_triple) = env::var("TARGET") else {
        return;
    };

    let binary_path = PathBuf::from(manifest_dir)
        .join("binaries")
        .join(format!("ralphx-workflow-runner-{target_triple}"));
    if binary_path.exists() {
        return;
    }

    if let Some(parent) = binary_path.parent() {
        if fs::create_dir_all(parent).is_err() {
            return;
        }
    }

    let _ = fs::write(
        &binary_path,
        b"placeholder generated for direct Cargo checks; Tauri builds replace this with the compiled workflow runner\n",
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        if let Ok(metadata) = fs::metadata(&binary_path) {
            let mut permissions = metadata.permissions();
            permissions.set_mode(0o755);
            let _ = fs::set_permissions(&binary_path, permissions);
        }
    }
}

fn main() {
    ensure_workflow_runner_binary_placeholder();
    tauri_build::build()
}
