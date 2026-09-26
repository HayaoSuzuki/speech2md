[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)] [string]$Archive,
    [string]$Model,
    [string]$Fixture
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$archivePath = (Resolve-Path -LiteralPath $Archive).Path
$work = Join-Path ([IO.Path]::GetTempPath()) ("speech2md-whisper-verify-" + [guid]::NewGuid().ToString('N'))
$oldNoProxy = [Environment]::GetEnvironmentVariable('NO_PROXY', 'Process')
$oldNoProxyLower = [Environment]::GetEnvironmentVariable('no_proxy', 'Process')

try {
    New-Item -ItemType Directory -Path $work | Out-Null
    $entries = tar -tf $archivePath
    if ($LASTEXITCODE -ne 0) { throw 'could not list archive' }
    $allowed = @('bin/whisper-cli.exe', 'LICENSE', 'build-metadata.json')
    foreach ($entry in $entries) {
        $normalized = $entry.TrimStart('./').Replace('\', '/')
        if ($normalized -and $normalized -notin $allowed -and $normalized -notin @('bin', 'bin/')) {
            throw "unexpected archive entry: $entry"
        }
    }
    foreach ($required in $allowed) {
        if ($entries.TrimStart('./').Replace('\', '/') -notcontains $required) {
            throw "required archive entry is missing: $required"
        }
    }
    tar -xf $archivePath -C $work
    if ($LASTEXITCODE -ne 0) { throw 'could not extract archive' }
    $exe = Join-Path $work 'bin\whisper-cli.exe'
    & $exe --help 2>&1 | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'whisper-cli --help failed' }

    $sentinel = 'speech2md-secret-prompt-7e18b1'
    $prompt = Join-Path $work 'prompt.txt'
    Set-Content -LiteralPath $prompt -Value $sentinel -Encoding utf8NoBOM -NoNewline
    $missing = Join-Path $work 'missing.wav'
    $output = (& $exe --prompt-file $prompt --file $missing 2>&1 | Out-String)
    if ($LASTEXITCODE -ne 2) { throw "--prompt-file contract failed with exit code $LASTEXITCODE" }
    if ($output.Contains($sentinel)) { throw 'prompt contents leaked to process output' }
    $output = (& $exe --prompt $sentinel --prompt-file $prompt --file $missing 2>&1 | Out-String)
    if ($LASTEXITCODE -ne 1) { throw '--prompt and --prompt-file were accepted together' }
    if ($output.Contains($sentinel)) { throw 'prompt contents leaked from conflicting arguments' }

    if (($Model -eq '') -xor ($Fixture -eq '')) { throw 'Model and Fixture must be provided together' }
    if ($Model -and $Fixture) {
        $modelPath = (Resolve-Path -LiteralPath $Model).Path
        $fixturePath = (Resolve-Path -LiteralPath $Fixture).Path
        $env:NO_PROXY = '*'
        $env:no_proxy = '*'
        & $exe --model $modelPath --file $fixturePath --language ja --output-json --no-prints
        if ($LASTEXITCODE -ne 0) { throw 'offline fixture transcription failed' }
    }
    Write-Output "verified: $archivePath"
}
finally {
    [Environment]::SetEnvironmentVariable('NO_PROXY', $oldNoProxy, 'Process')
    [Environment]::SetEnvironmentVariable('no_proxy', $oldNoProxyLower, 'Process')
    if (Test-Path -LiteralPath $work) {
        $resolved = (Resolve-Path -LiteralPath $work).Path
        if (-not $resolved.StartsWith([IO.Path]::GetFullPath([IO.Path]::GetTempPath()), [StringComparison]::OrdinalIgnoreCase)) {
            throw "refusing to remove non-temporary path: $resolved"
        }
        Remove-Item -LiteralPath $resolved -Recurse -Force
    }
}
