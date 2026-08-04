---
paths:
  - "assets/**"
  - "README.md"
  - "docs/**/*.md"
---

> **Maintainer note:** This file optimizes for LLM context efficiency. Rules: (1) Tables > prose (2) One example max per concept (3) No redundant explanations (4) Use symbols: → = leads to, | = or, ❌/✅ = wrong/right (5) Before adding content, ask: "Can this be a single line?" If yes, make it one line.

# Asset Publishing

## Paths

| Path | Role |
|---|---|
| `assets/raw/**` | Local source captures, experiments, and unpublished variants → gitignored |
| `assets/public/**` | Tracked publishable assets used by README/docs |
| `assets/scripts/frame-screenshots.py` | Frames screenshots from `assets/raw/**` and publishes to `assets/public/**` |
| `assets/scripts/compress-assets.py` | Reusable compressor/publish helper |

## Rules

| Rule | Detail |
|---|---|
| Raw captures stay out of Git | Put new screenshots and source exports under `assets/raw/**`; do not commit them |
| Published assets only | Commit only files under `assets/public/**` |
| Compression required | Publish via `assets/scripts/frame-screenshots.py` or `assets/scripts/compress-assets.py` before commit |
| Legibility gate | Do not accept a smaller file if text/UI details become hard to read; manually inspect before commit |
| Public markdown uses public assets | README/docs should reference `assets/public/**`, never `assets/raw/**` |
| Variations stay local | Gradient tests and unpublished alternates belong under `assets/raw/variations/**` or `assets/raw/unpublished/**` |
| Prefer targeted runs | Use `--single <palette-key>` for one screenshot and `--skip-existing` to resume a partial batch instead of rerunning everything |

## Commands

```bash
python3 assets/scripts/frame-screenshots.py --list
python3 assets/scripts/frame-screenshots.py
python3 assets/scripts/frame-screenshots.py --single welcome
python3 assets/scripts/frame-screenshots.py --skip-existing
python3 assets/scripts/compress-assets.py assets/raw/some-image.png --output-dir assets/public
```

## Screenshot Framing Checklist

| Step | Rule |
|---|---|
| Inspect source | Check for macOS system bar (date/time/Wi-Fi/battery) at top of screenshot |
| Crop if needed | `assets/scripts/frame-screenshots.py` auto-crops via variance detection; manual fallback: ~50px standard display \| ~100px Retina 2x |
| Frame | `python3 assets/scripts/frame-screenshots.py --single <palette-key>` for one screenshot \| `--skip-existing` to resume a partial batch |
| Verify output | Confirm no system bar, no personal info, and legible text in `assets/public/framed-*.png` |
