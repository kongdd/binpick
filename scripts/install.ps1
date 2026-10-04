[CmdletBinding()]
param(
    [string]$Version = 'latest',
    [string]$InstallDir = '',
    [string]$Repository = 'kongdd/prex',
    [switch]$AddToPath,
    [switch]$Help
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

if ($Help) {
    Write-Output 'Install prex after SHA-256 verification. Requires PowerShell 5.1+ on Windows.'
    Write-Output '.\install.ps1 [-Version v0.1.1] [-InstallDir DIR] [-AddToPath]'
    Write-Output 'Defaults: latest release; $HOME\.prex\bin (or $env:PREX_ROOT\bin).'
    return
}
if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT) {
    throw 'Use install.sh on Linux/macOS; this script is for Windows.'
}
if ($Repository -notmatch '^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$') {
    throw 'Repository must be a GitHub owner/repository.'
}
if (-not $InstallDir) {
    $root = if ($env:PREX_ROOT) { $env:PREX_ROOT } else { Join-Path $HOME '.prex' }
    $InstallDir = Join-Path $root 'bin'
}
$InstallDir = [IO.Path]::GetFullPath($InstallDir)
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
try {
    $architecture = [Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString()
} catch {
    $architecture = if ($env:PROCESSOR_ARCHITEW6432) {
        $env:PROCESSOR_ARCHITEW6432
    } else {
        $env:PROCESSOR_ARCHITECTURE
    }
}
switch ($architecture.ToLowerInvariant()) {
    { $_ -in @('x64', 'amd64') } { $platform = 'windows-amd64' }
    'arm64' { $platform = 'windows-arm64' }
    default { throw "Unsupported Windows architecture: $architecture" }
}
$headers = @{ 'User-Agent' = 'prex-installer' }
if ($Version -eq 'latest') {
    $release = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repository/releases/latest" -Headers $headers
    $Version = $release.tag_name
} else {
    $Version = 'v' + $Version.TrimStart('v')
}
if ($Version -notmatch '^v[0-9]+\.[0-9]+\.[0-9]+$') {
    throw "Invalid release version: $Version"
}
$assetName = "prex-$($Version.Substring(1))-$platform.exe"
$base = "https://github.com/$Repository/releases/download/$Version"
$work = Join-Path ([IO.Path]::GetTempPath()) ('prex-install-' + [Guid]::NewGuid().ToString('N'))
$staged = $null
New-Item -ItemType Directory -Path $work | Out-Null
try {
    Write-Output "Downloading prex $Version ($platform)..."
    $binary = Join-Path $work $assetName
    $sums = Join-Path $work 'SHA256SUMS.txt'
    Invoke-WebRequest -Uri "$base/$assetName" -Headers $headers -UseBasicParsing -OutFile $binary
    Invoke-WebRequest -Uri "$base/SHA256SUMS.txt" -Headers $headers -UseBasicParsing -OutFile $sums
    $expected = @(
        foreach ($line in Get-Content -LiteralPath $sums) {
            $match = [regex]::Match($line, '^([a-fA-F0-9]{64})\s+\*?(.+)$')
            if ($match.Success -and $match.Groups[2].Value -ceq $assetName) {
                $match.Groups[1].Value
            }
        }
    )
    if ($expected.Count -ne 1) { throw "Missing or ambiguous checksum for $assetName" }
    $actual = (Get-FileHash -LiteralPath $binary -Algorithm SHA256).Hash
    if ($actual -ine $expected[0]) { throw 'SHA-256 mismatch; nothing was installed' }
    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
    $destination = Join-Path $InstallDir 'prex.exe'
    $staged = Join-Path $InstallDir ('.prex-install-' + [Guid]::NewGuid().ToString('N') + '.exe')
    Copy-Item -LiteralPath $binary -Destination $staged
    if (Test-Path -LiteralPath $destination) {
        # Replace atomically; an in-use executable fails without removing the old copy.
        [IO.File]::Replace($staged, $destination, $null)
    } else {
        [IO.File]::Move($staged, $destination)
    }
    $staged = $null
    if ($AddToPath) {
        $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
        $entries = @($userPath -split ';' | Where-Object { $_ })
        if (-not ($entries | Where-Object { $_.TrimEnd('\') -ieq $InstallDir.TrimEnd('\') })) {
            $entries += $InstallDir
            [Environment]::SetEnvironmentVariable('Path', ($entries -join ';'), 'User')
        }
        $env:Path = "$InstallDir;$env:Path"
    }
    Write-Output "Installed $Version to $destination"
    if (-not $AddToPath) {
        Write-Output "Add $InstallDir to PATH, or rerun with -AddToPath."
    }
    Write-Output 'Then run: prex init'
} finally {
    if ($staged -and (Test-Path -LiteralPath $staged)) {
        Remove-Item -LiteralPath $staged -Force
    }
    Remove-Item -LiteralPath $work -Recurse -Force
}
