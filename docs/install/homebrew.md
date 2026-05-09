# Homebrew Installation

RalphX ships as a signed macOS app and can be installed with Homebrew Cask.

## Install

```bash
brew tap aigentive/ralphx
brew install --cask ralphx
```

## Upgrade

```bash
brew update
brew upgrade --cask ralphx
```

## Refresh Tap Metadata

If a new RalphX release exists but Homebrew still reports that `ralphx` is already up to date, refresh the tap metadata and retry:

```bash
brew update-reset aigentive/ralphx
brew upgrade --cask ralphx
```

## Repair A Missing App

If `/Applications/RalphX.app` was deleted manually and `brew upgrade --cask ralphx` fails with `App source '/Applications/RalphX.app' is not there`, repair the Homebrew cask receipt and reinstall:

```bash
brew uninstall --cask --force ralphx
brew install --cask ralphx
```

## Uninstall

```bash
brew uninstall --cask ralphx
```

Do not use `--zap` unless you intentionally want to remove local RalphX app data.

## GitHub Releases

If you do not want to use Homebrew, download signed builds from the [GitHub Releases page](https://github.com/aigentive/ralphx.app/releases).
