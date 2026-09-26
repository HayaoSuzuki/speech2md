[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)] [string]$Archive,
    [string]$Model,
    [string]$Fixture,
    [switch]$ContractOnly
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$archivePath = (Resolve-Path -LiteralPath $Archive).Path
$work = Join-Path ([IO.Path]::GetTempPath()) ("yasumaro-whisper-verify-" + [guid]::NewGuid().ToString('N'))
$oldNoProxy = [Environment]::GetEnvironmentVariable('NO_PROXY', 'Process')
$oldNoProxyLower = [Environment]::GetEnvironmentVariable('no_proxy', 'Process')
$proxyNames = @('HTTP_PROXY', 'HTTPS_PROXY', 'ALL_PROXY', 'http_proxy', 'https_proxy', 'all_proxy')
$oldProxies = @{}
foreach ($name in $proxyNames) { $oldProxies[$name] = [Environment]::GetEnvironmentVariable($name, 'Process') }

try {
    if (-not $ContractOnly -and (-not $Model -or -not $Fixture)) {
        throw 'Model and Fixture are required unless -ContractOnly is specified'
    }
    if ($ContractOnly -and ($Model -or $Fixture)) {
        throw 'Do not combine -ContractOnly with Model or Fixture'
    }
    New-Item -ItemType Directory -Path $work | Out-Null
    $entries = tar -tf $archivePath
    if ($LASTEXITCODE -ne 0) { throw 'could not list archive' }
    $allowed = @('bin/whisper-cli.exe', 'LICENSE', 'build-metadata.json')
    foreach ($entry in $entries) {
        if ($entry -notin $allowed) {
            throw "unexpected archive entry: $entry"
        }
    }
    foreach ($required in $allowed) {
        if ($entries -notcontains $required) {
            throw "required archive entry is missing: $required"
        }
    }
    tar -xf $archivePath -C $work
    if ($LASTEXITCODE -ne 0) { throw 'could not extract archive' }
    $exe = Join-Path $work 'bin\whisper-cli.exe'
    if ((Get-Item -LiteralPath $exe -Force).Attributes.HasFlag([IO.FileAttributes]::ReparsePoint)) {
        throw 'whisper-cli is a reparse point'
    }
    & $exe --help 2>&1 | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'whisper-cli --help failed' }

    $sentinel = 'yasumaro-secret-prompt-7e18b1'
    $prompt = Join-Path $work 'prompt.txt'
    Set-Content -LiteralPath $prompt -Value $sentinel -Encoding utf8NoBOM -NoNewline
    $missing = Join-Path $work 'missing.wav'
    $output = (& $exe --prompt-file $prompt --file $missing 2>&1 | Out-String)
    if ($LASTEXITCODE -ne 2) { throw "--prompt-file contract failed with exit code $LASTEXITCODE" }
    if ($output.Contains($sentinel)) { throw 'prompt contents leaked to process output' }
    $output = (& $exe --prompt $sentinel --prompt-file $prompt --file $missing 2>&1 | Out-String)
    if ($LASTEXITCODE -ne 1) { throw '--prompt and --prompt-file were accepted together' }
    if ($output.Contains($sentinel)) { throw 'prompt contents leaked from conflicting arguments' }

    if ($Model -and $Fixture) {
        $modelPath = (Resolve-Path -LiteralPath $Model).Path
        $fixturePath = (Resolve-Path -LiteralPath $Fixture).Path
        foreach ($name in $proxyNames) { [Environment]::SetEnvironmentVariable($name, 'http://127.0.0.1:9', 'Process') }
        $env:NO_PROXY = ''
        $env:no_proxy = ''
        & $exe --model $modelPath --file $fixturePath --language ja --output-json --output-file (Join-Path $work 'verified') --no-prints
        if ($LASTEXITCODE -ne 0) { throw 'offline fixture transcription failed' }

        $startInfo = [Diagnostics.ProcessStartInfo]::new()
        $startInfo.FileName = $exe
        $startInfo.UseShellExecute = $false
        foreach ($argument in @('--model', $modelPath, '--file', $fixturePath, '--language', 'ja', '--output-json', '--output-file', (Join-Path $work 'cancelled'), '--no-prints')) {
            $startInfo.ArgumentList.Add($argument)
        }
        $process = [Diagnostics.Process]::Start($startInfo)
        Start-Sleep -Milliseconds 100
        if ($process.HasExited) { throw 'fixture completed before cancellation could be exercised' }
        $process.Kill($true)
        $process.WaitForExit()
        if (-not $process.HasExited) { throw 'cancelled whisper process remained alive' }
    }
    Write-Output "verified: $archivePath"
}
finally {
    [Environment]::SetEnvironmentVariable('NO_PROXY', $oldNoProxy, 'Process')
    [Environment]::SetEnvironmentVariable('no_proxy', $oldNoProxyLower, 'Process')
    foreach ($name in $proxyNames) { [Environment]::SetEnvironmentVariable($name, $oldProxies[$name], 'Process') }
    if (Test-Path -LiteralPath $work) {
        $resolved = (Resolve-Path -LiteralPath $work).Path
        if (-not $resolved.StartsWith([IO.Path]::GetFullPath([IO.Path]::GetTempPath()), [StringComparison]::OrdinalIgnoreCase)) {
            throw "refusing to remove non-temporary path: $resolved"
        }
        Remove-Item -LiteralPath $resolved -Recurse -Force
    }
}
