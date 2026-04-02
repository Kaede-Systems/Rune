# Versioning

Rune now uses a simple SemVer-style release model.

## Canonical Version

The canonical Rune version comes from:

- [Cargo.toml](../Cargo.toml)

The CLI reports that version with:

```text
rune version
rune --version
```

## Branch Flow

The repository branch policy is:

- `main`: active development branch
- `release`: release-candidate and publishing branch

Normal work lands on `main`.

When a feature set is ready for publishing:

1. `main` is merged into `release`
2. GitHub Actions builds release bundles from `release`
3. release assets are published to GitHub Releases

## Release Channels

Rune uses two release channels on GitHub:

- immutable versioned release tags like `v0.2.0`
- moving release channel tag `release-branch-latest`

The versioned tag is the stable historical release.

The moving tag is the latest bundle built from the `release` branch.

## Asset Naming

Versioned bundles:

- `rune-v0.2.0-windows-x64.zip`
- `rune-v0.2.0-linux-x64.tar.gz`
- `rune-v0.2.0-linux-arm64.tar.gz`
- `rune-v0.2.0-macos-x64.tar.gz`
- `rune-v0.2.0-macos-arm64.tar.gz`

Latest-channel bundles:

- `rune-latest-windows-x64.zip`
- `rune-latest-linux-x64.tar.gz`
- `rune-latest-linux-arm64.tar.gz`
- `rune-latest-macos-x64.tar.gz`
- `rune-latest-macos-arm64.tar.gz`

## Installer Behavior

Installers support:

- latest release-channel install
- specific version install
- local developer install

Examples:

```bash
./installers/install-unix.sh
./installers/install-unix.sh --version 0.2.0
```

```powershell
powershell -ExecutionPolicy Bypass -File .\installers\install-windows.ps1
powershell -ExecutionPolicy Bypass -File .\installers\install-windows.ps1 -Version 0.2.0
```
