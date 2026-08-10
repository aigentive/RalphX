use anyhow::{bail, Context, Result};
use event_manifest_scanner::build_manifest;
use std::env;
use std::fs;
use std::path::PathBuf;

fn main() -> Result<()> {
    let mut args = env::args().skip(1);
    let mode = args.next().unwrap_or_else(|| "--check".into());
    let root = args
        .next()
        .map(PathBuf::from)
        .unwrap_or(env::current_dir()?);
    if args.next().is_some() {
        bail!("usage: event-manifest-scanner [--check|--write] [repository-root]");
    }

    let output = root.join("scripts/event-manifest.json");
    let rendered = serde_json::to_string_pretty(&build_manifest(&root)?)? + "\n";
    match mode.as_str() {
        "--write" => {
            fs::write(&output, rendered).with_context(|| format!("write {}", output.display()))
        }
        "--check" => {
            let checked = fs::read_to_string(&output)
                .with_context(|| format!("read {}; run with --write", output.display()))?;
            if checked == rendered {
                Ok(())
            } else {
                bail!(
                    "{} is stale; regenerate with: cargo run --manifest-path scripts/event-manifest-scanner/Cargo.toml -- --write",
                    output.display()
                );
            }
        }
        _ => bail!("usage: event-manifest-scanner [--check|--write] [repository-root]"),
    }
}
