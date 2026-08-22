#!/usr/bin/env python3
"""Publish raw docs-capture baselines as compressed guide images."""

from __future__ import annotations

from pathlib import Path
import hashlib
import subprocess
import sys


REPO_ROOT = Path(__file__).resolve().parents[2]
SNAPSHOTS = REPO_ROOT / "frontend/tests/docs-capture/snapshots"
OUTPUT = REPO_ROOT / "assets/public/guides"
MANIFEST = OUTPUT / "baselines.sha256"
SUFFIX = "-docs-capture-darwin.png"
# Publish at 1:1 logical pixels for the docs-capture viewport width in
# frontend/playwright.config.ts. Baselines are captured at deviceScaleFactor 2,
# so halving keeps UI text at its native rendered size; going lower resamples
# below 1x and fails the legibility gate in .claude/rules/assets.md.
PUBLISH_MAX_WIDTH = 1728


def manifest_lines(images: list[Path]) -> list[str]:
    """Hash the *baselines*, not the published PNGs.

    Compression shells out to ImageMagick, whose output is not byte-stable
    across versions, so hashing published images would make CI fail on
    unrelated toolchain drift. Baseline hashes are deterministic and still
    catch the real staleness case: a capture re-baselined without re-running
    this script.
    """
    return [
        f"{hashlib.sha256(image.read_bytes()).hexdigest()}  {image.name.removesuffix(SUFFIX)}.png"
        for image in images
    ]


def main() -> int:
    images = sorted(SNAPSHOTS.rglob(f"*{SUFFIX}"), key=lambda p: p.name)
    if not images:
        print("No docs-capture baselines found.")
        return 1
    OUTPUT.mkdir(parents=True, exist_ok=True)

    published = set()
    for image in images:
        slug = image.name.removesuffix(SUFFIX)
        subprocess.run(
            [sys.executable, str(REPO_ROOT / "assets/scripts/compress-assets.py"), str(image), "--output-dir", str(OUTPUT), "--max-width", str(PUBLISH_MAX_WIDTH)],
            check=True,
        )
        (OUTPUT / image.name).rename(OUTPUT / f"{slug}.png")
        published.add(f"{slug}.png")

    for stale in sorted(OUTPUT.glob("*.png")):
        if stale.name not in published:
            print(f"Removing stale published image: {stale.name}")
            stale.unlink()

    MANIFEST.write_text("\n".join(manifest_lines(images)) + "\n")
    print(f"Published {len(published)} guide images and wrote {MANIFEST.name}.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
