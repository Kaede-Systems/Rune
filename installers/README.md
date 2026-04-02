# Rune Installers

Rune now has release-oriented installers.

They support two modes:

1. local developer install from a built binary
2. release install by downloading the matching Rune bundle for the current OS/arch from GitHub Releases

In release-install mode, the installers also fetch or build the matching packaged toolchain/runtime assets needed by Rune under `share/rune/tools` if the downloaded release bundle does not already contain them.

## Release Bundle Layout

The installers expect release assets shaped like this:

```text
bin/
  rune            # or rune.exe on Windows
share/
  rune/
    tools/
```

Release bundle asset names now follow two channels:

- immutable versioned assets like `rune-v0.2.0-linux-x64.tar.gz`
- moving latest-channel assets like `rune-latest-linux-x64.tar.gz`

## Windows

Install from the latest GitHub release:

```powershell
powershell -ExecutionPolicy Bypass -File .\installers\install-windows.ps1
```

Install from a specific repository/version:

```powershell
powershell -ExecutionPolicy Bypass -File .\installers\install-windows.ps1 -Repo Kaede-Systems/Rune -Version 0.2.0
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

Install from a specific repository/version:

```bash
./installers/install-unix.sh --repo Kaede-Systems/Rune --version 0.2.0
```

Developer/local install:

```bash
./installers/install-unix.sh ./rune
```

## Notes

- The release-install mode downloads the correct bundle for the current host.
- `latest` resolves to the moving `release-branch-latest` channel.
- explicit versions resolve to immutable tags like `v0.2.0`.
- The local-install mode is still useful when developing Rune from source.
- Release bundles now include a host `llvm-cbe` binary and the vendored `llvm-cbe` source tree under `tools/llvm-cbe`.
- If a bundle does not already contain a host `llvm-cbe` binary, the installers build it locally against the packaged LLVM toolchain.
- These installers are intended to pair with release assets published from CI, not with giant toolchain blobs committed into the source repository.
