# Rune Installers

Rune now has release-oriented installers.

They support two modes:

1. local developer install from a built binary
2. release install by downloading the matching Rune bundle for the current OS/arch from GitHub Releases

In release-install mode, the installers also fetch the matching packaged toolchain/runtime assets needed by Rune under `share/rune/tools` if the downloaded release bundle does not already contain them.

## Release Bundle Layout

The installers expect release assets shaped like this:

```text
bin/
  rune            # or rune.exe on Windows
share/
  rune/
    tools/
```

Release bundle asset names:

- `rune-bundle-windows-x64.zip`
- `rune-bundle-windows-arm64.zip`
- `rune-bundle-linux-x64.tar.gz`
- `rune-bundle-linux-arm64.tar.gz`
- `rune-bundle-macos-x64.tar.gz`
- `rune-bundle-macos-arm64.tar.gz`

## Windows

Install from the latest GitHub release:

```powershell
powershell -ExecutionPolicy Bypass -File .\installers\install-windows.ps1
```

Install from a specific repository/tag:

```powershell
powershell -ExecutionPolicy Bypass -File .\installers\install-windows.ps1 -Repo Kaede-Systems/Rune -Version v0.1.0
```

Developer/local install:

```powershell
powershell -ExecutionPolicy Bypass -File .\installers\install-windows.ps1 -BinaryPath .\target\release\rune.exe
```

## Linux / macOS

Install from the latest GitHub release:

```bash
chmod +x ./installers/install-unix.sh
./installers/install-unix.sh
```

Install from a specific repository/tag:

```bash
./installers/install-unix.sh --repo Kaede-Systems/Rune --version v0.1.0
```

Developer/local install:

```bash
./installers/install-unix.sh ./rune
```

## Notes

- The release-install mode downloads the correct bundle for the current host.
- The local-install mode is still useful when developing Rune from source.
- These installers are intended to pair with release assets published from CI, not with giant toolchain blobs committed into the source repository.
