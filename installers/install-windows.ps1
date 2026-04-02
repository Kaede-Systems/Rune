param(
    [string]$BinaryPath,
    [string]$InstallRoot = "$env:LOCALAPPDATA\\Rune",
    [switch]$MachinePath,
    [string]$Repo = "Kaede-Systems/Rune",
    [string]$Version = "latest"
)

$ErrorActionPreference = "Stop"
$LlvmVersion = "21.1.7"
$WasmtimeVersion = "43.0.0"
$InstallLockPath = Join-Path $env:TEMP "rune-install.lock"

function Get-HostAssetName {
    $arch = $env:PROCESSOR_ARCHITECTURE
    if ($arch -match "ARM64") {
        return "windows-arm64.zip"
    }
    return "windows-x64.zip"
}

function Get-HostBundleName {
    $arch = $env:PROCESSOR_ARCHITECTURE
    if ($arch -match "ARM64") {
        return "windows-arm64"
    }
    return "windows-x64"
}

function Use-InstallLock {
    while ($true) {
        try {
            $script:InstallLockHandle = [System.IO.File]::Open($InstallLockPath, "OpenOrCreate", "ReadWrite", "None")
            break
        } catch {
            Start-Sleep -Seconds 1
        }
    }
}

function Test-LlvmReady {
    param([string]$Root)
    return [bool](Get-ChildItem -LiteralPath $Root -Recurse -Filter opt.exe -ErrorAction SilentlyContinue | Select-Object -First 1)
}

function Test-WasmtimeReady {
    param([string]$Root)
    return [bool](Get-ChildItem -LiteralPath $Root -Recurse -Include wasmtime.exe,wasmtime -ErrorAction SilentlyContinue | Select-Object -First 1)
}

function Test-LlvmCbeReady {
    param([string]$Root)
    return [bool](Get-ChildItem -LiteralPath $Root -Recurse -Include llvm-cbe.exe,llvm-cbe -ErrorAction SilentlyContinue | Select-Object -First 1)
}

function Get-LlvmCbeSourcePath {
    param([string]$ToolsRoot)
    foreach ($candidate in @(
        (Join-Path $ToolsRoot "llvm-cbe")
    )) {
        if (Test-Path -LiteralPath (Join-Path $candidate "CMakeLists.txt")) {
            return $candidate
        }
    }
    return $null
}

function Find-LlvmCmakeDir {
    param([string]$Root)
    $candidate = Get-ChildItem -LiteralPath $Root -Recurse -Directory -Filter llvm -ErrorAction SilentlyContinue |
        Where-Object { $_.FullName -like "*\\lib\\cmake\\llvm" } |
        Select-Object -First 1
    if ($candidate) {
        return $candidate.FullName
    }
    return $null
}

function Download-ReleaseBundle {
    param(
        [string]$RepoName,
        [string]$ReleaseVersion
    )

    $assetSuffix = Get-HostAssetName
    $normalizedVersion = $ReleaseVersion
    if ($normalizedVersion -ne "latest" -and $normalizedVersion -ne "release-branch-latest") {
        if (-not $normalizedVersion.StartsWith("v")) {
            $normalizedVersion = "v$normalizedVersion"
        }
    }
    if ($normalizedVersion -eq "latest" -or $normalizedVersion -eq "release-branch-latest") {
        $tag = "release-branch-latest"
        $assetName = "rune-latest-$assetSuffix"
    } else {
        $tag = $normalizedVersion
        $assetName = "rune-$tag-$assetSuffix"
    }
    $tempDir = Join-Path $env:TEMP ("rune-install-" + [guid]::NewGuid().ToString("N"))
    $archivePath = Join-Path $tempDir $assetName
    $extractDir = Join-Path $tempDir "extract"
    New-Item -ItemType Directory -Path $extractDir -Force | Out-Null

    $url = "https://github.com/$RepoName/releases/download/$tag/$assetName"

    Write-Host "Downloading $url"
    Invoke-WebRequest -Uri $url -OutFile $archivePath
    Expand-Archive -Path $archivePath -DestinationPath $extractDir -Force

    $children = Get-ChildItem -LiteralPath $extractDir
    if ($children.Count -eq 1 -and $children[0].PSIsContainer) {
        return $children[0].FullName
    }
    return $extractDir
}

function Install-BundleRoot {
    param(
        [string]$SourceRoot,
        [string]$DestinationRoot
    )

    $binDir = Join-Path $DestinationRoot "bin"
    $shareDir = Join-Path $DestinationRoot "share\\rune"
    New-Item -ItemType Directory -Path $binDir -Force | Out-Null
    New-Item -ItemType Directory -Path $shareDir -Force | Out-Null

    $sourceExe = Join-Path $SourceRoot "bin\\rune.exe"
    if (-not (Test-Path -LiteralPath $sourceExe)) {
        throw "Release bundle is missing bin\\rune.exe"
    }

    Copy-Item -LiteralPath $sourceExe -Destination (Join-Path $binDir "rune.exe") -Force

    $sourceShare = Join-Path $SourceRoot "share\\rune"
    if (Test-Path -LiteralPath $sourceShare) {
        Remove-Item -LiteralPath $shareDir -Recurse -Force -ErrorAction SilentlyContinue
        New-Item -ItemType Directory -Path $shareDir -Force | Out-Null
        Copy-Item -LiteralPath $sourceShare -Destination $shareDir -Recurse -Force
    }
}

function Ensure-HostTools {
    param(
        [string]$DestinationRoot
    )

    $bundleName = Get-HostBundleName
    if ($bundleName -eq "windows-arm64") {
        throw "Automatic packaged LLVM bootstrap is not implemented yet for Windows ARM64 hosts."
    }

    $toolsRoot = Join-Path $DestinationRoot "share\\rune\\tools"
    $llvmDest = Join-Path $toolsRoot "llvm21\\$bundleName"
    $wasmtimeDest = Join-Path $toolsRoot "wasmtime\\extract\\$bundleName"
    $llvmCbeDest = Join-Path $toolsRoot "llvm-cbe\\$bundleName"
    New-Item -ItemType Directory -Path $toolsRoot -Force | Out-Null

    if (-not (Test-Path -LiteralPath $llvmDest) -or -not (Test-LlvmReady -Root $llvmDest)) {
        $tempDir = Join-Path $env:TEMP ("rune-tools-" + [guid]::NewGuid().ToString("N"))
        New-Item -ItemType Directory -Path $tempDir -Force | Out-Null
        $llvmInstaller = Join-Path $tempDir "llvm-installer.exe"
        $llvmUrl = "https://github.com/llvm/llvm-project/releases/download/llvmorg-$LlvmVersion/LLVM-$LlvmVersion-win64.exe"
        Write-Host "Downloading LLVM toolchain from $llvmUrl"
        Invoke-WebRequest -Uri $llvmUrl -OutFile $llvmInstaller
        $llvmStage = Join-Path $tempDir "llvm-stage"
        New-Item -ItemType Directory -Path $llvmStage -Force | Out-Null
        Start-Process -FilePath $llvmInstaller -ArgumentList "/S", "/D=$llvmStage" -Wait
        if (-not (Test-LlvmReady -Root $llvmStage)) {
            throw "Downloaded LLVM bundle is missing opt.exe"
        }
        Remove-Item -LiteralPath $llvmDest -Recurse -Force -ErrorAction SilentlyContinue
        Move-Item -LiteralPath $llvmStage -Destination $llvmDest
    }

    if (-not (Test-Path -LiteralPath $wasmtimeDest) -or -not (Test-WasmtimeReady -Root $wasmtimeDest)) {
        $tempDir = Join-Path $env:TEMP ("rune-tools-" + [guid]::NewGuid().ToString("N"))
        New-Item -ItemType Directory -Path $tempDir -Force | Out-Null
        $wasmtimeZip = Join-Path $tempDir "wasmtime.zip"
        $extractRoot = Join-Path $tempDir "extract"
        $wasmtimeUrl = "https://github.com/bytecodealliance/wasmtime/releases/download/v$WasmtimeVersion/wasmtime-v$WasmtimeVersion-x86_64-windows.zip"
        Write-Host "Downloading Wasmtime from $wasmtimeUrl"
        Invoke-WebRequest -Uri $wasmtimeUrl -OutFile $wasmtimeZip
        Expand-Archive -Path $wasmtimeZip -DestinationPath $extractRoot -Force
        $children = Get-ChildItem -LiteralPath $extractRoot
        $source = if ($children.Count -eq 1 -and $children[0].PSIsContainer) { $children[0].FullName } else { $extractRoot }
        if (-not (Test-WasmtimeReady -Root $source)) {
            throw "Downloaded Wasmtime bundle is missing wasmtime.exe"
        }
        Remove-Item -LiteralPath $wasmtimeDest -Recurse -Force -ErrorAction SilentlyContinue
        Move-Item -LiteralPath $source -Destination $wasmtimeDest
    }

    if (-not (Test-Path -LiteralPath $llvmCbeDest) -or -not (Test-LlvmCbeReady -Root $llvmCbeDest)) {
        $llvmCbeSource = Get-LlvmCbeSourcePath -ToolsRoot $toolsRoot
        if (-not $llvmCbeSource) {
            throw "Packaged llvm-cbe source is missing, and no bundled llvm-cbe executable was found."
        }

        $llvmCmake = Find-LlvmCmakeDir -Root $llvmDest
        if (-not $llvmCmake) {
            throw "Downloaded LLVM bundle is missing lib\\cmake\\llvm"
        }

        $tempDir = Join-Path $env:TEMP ("rune-cbe-" + [guid]::NewGuid().ToString("N"))
        $buildDir = Join-Path $tempDir "build"
        New-Item -ItemType Directory -Path $buildDir -Force | Out-Null
        cmake -S $llvmCbeSource -B $buildDir -G "Visual Studio 17 2022" -A x64 -DLLVM_DIR=$llvmCmake
        if ($LASTEXITCODE -ne 0) {
            throw "Failed to configure llvm-cbe"
        }
        cmake --build $buildDir --config Release --target llvm-cbe
        if ($LASTEXITCODE -ne 0) {
            throw "Failed to build llvm-cbe"
        }
        $builtCbe = Join-Path $buildDir "tools\\llvm-cbe\\Release\\llvm-cbe.exe"
        if (-not (Test-Path -LiteralPath $builtCbe)) {
            throw "Built llvm-cbe binary not found"
        }
        New-Item -ItemType Directory -Path (Join-Path $llvmCbeDest "bin") -Force | Out-Null
        Copy-Item -LiteralPath $builtCbe -Destination (Join-Path $llvmCbeDest "bin\\llvm-cbe.exe") -Force
    }
}

Use-InstallLock

$bundleRoot = $null
if ($BinaryPath) {
    if (-not (Test-Path -LiteralPath $BinaryPath)) {
        throw "Rune binary not found: $BinaryPath"
    }
    $tempDir = Join-Path $env:TEMP ("rune-install-" + [guid]::NewGuid().ToString("N"))
    $bundleRoot = Join-Path $tempDir "bundle"
    New-Item -ItemType Directory -Path (Join-Path $bundleRoot "bin") -Force | Out-Null
    New-Item -ItemType Directory -Path (Join-Path $bundleRoot "share\\rune") -Force | Out-Null
    Copy-Item -LiteralPath $BinaryPath -Destination (Join-Path $bundleRoot "bin\\rune.exe") -Force
    $repoTools = Join-Path (Get-Location) "tools"
    if (Test-Path -LiteralPath $repoTools) {
        Copy-Item -LiteralPath $repoTools -Destination (Join-Path $bundleRoot "share\\rune\\tools") -Recurse -Force
    }
} else {
    $bundleRoot = Download-ReleaseBundle -RepoName $Repo -ReleaseVersion $Version
}

Install-BundleRoot -SourceRoot $bundleRoot -DestinationRoot $InstallRoot
Ensure-HostTools -DestinationRoot $InstallRoot

$binDir = Join-Path $InstallRoot "bin"
$scope = if ($MachinePath) { "Machine" } else { "User" }
$currentPath = [Environment]::GetEnvironmentVariable("Path", $scope)
if ([string]::IsNullOrWhiteSpace($currentPath)) {
    $currentPath = ""
}
$paths = $currentPath -split ';' | Where-Object { $_ -and $_.Trim() -ne "" }
if ($paths -notcontains $binDir) {
    $newPath = if ($currentPath.Trim().Length -eq 0) { $binDir } else { "$currentPath;$binDir" }
    [Environment]::SetEnvironmentVariable("Path", $newPath, $scope)
}

Write-Host "Installed Rune to $(Join-Path $binDir 'rune.exe')"
if (Test-Path -LiteralPath (Join-Path $InstallRoot 'share\\rune')) {
    Write-Host "Installed Rune shared assets to $(Join-Path $InstallRoot 'share\\rune')"
}
Write-Host "Added $binDir to $scope PATH"
if ($script:InstallLockHandle) {
    $script:InstallLockHandle.Dispose()
}
