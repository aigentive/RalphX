use std::env;
use std::fs;

fn ensure_workflow_runner_binary_placeholder() {
    let Ok(manifest_dir) = env::current_dir().and_then(fs::canonicalize) else {
        return;
    };
    let Ok(target_triple) = env::var("TARGET") else {
        return;
    };
    if target_triple.is_empty()
        || !target_triple
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return;
    }

    let binaries_dir = manifest_dir.join("binaries");
    if fs::create_dir_all(&binaries_dir).is_err() {
        return;
    }
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
    let _ = fs::copy(build_script_binary, &binary_path);

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
