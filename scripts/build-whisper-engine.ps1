[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$OutputDirectory
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$upstreamUrl = 'https://github.com/ggml-org/whisper.cpp.git'
$upstreamCommit = '927cfce34f31707e17f2bff35c349632fb9e2c3a'
$upstreamVersion = 'v1.9.4'
$repositoryRoot = Split-Path -Parent $PSScriptRoot
$patchPath = Join-Path $repositoryRoot 'vendor\whisper.cpp-patches\0001-add-prompt-file.patch'
$resolvedOutput = [IO.Path]::GetFullPath($OutputDirectory)
$work = Join-Path ([IO.Path]::GetTempPath()) ("yasumaro-whisper-build-" + [guid]::NewGuid().ToString('N'))
$source = Join-Path $work 'source'
$build = Join-Path $work 'build'
$stage = Join-Path $work 'stage'

function Find-CMake {
    $command = Get-Command cmake -ErrorAction SilentlyContinue
    if ($command) { return $command.Source }
    $known = 'C:\Program Files\CMake\bin\cmake.exe'
    if (Test-Path -LiteralPath $known) { return $known }
    throw 'cmake was not found. Install CMake or add it to PATH.'
}

try {
    New-Item -ItemType Directory -Path $resolvedOutput, $work, $stage -Force | Out-Null
    git clone --filter=blob:none --no-checkout $upstreamUrl $source
    git -C $source checkout --detach $upstreamCommit
    $actualCommit = (git -C $source rev-parse HEAD).Trim()
    if ($actualCommit -ne $upstreamCommit) { throw "unexpected upstream commit: $actualCommit" }
    git -C $source apply --check $patchPath
    git -C $source apply $patchPath

    $cmake = Find-CMake
    & $cmake -S $source -B $build -A x64 `
        -DBUILD_SHARED_LIBS=OFF `
        -DGGML_NATIVE=OFF `
        -DGGML_OPENMP=OFF `
        -DGGML_CUDA=OFF `
        "-DCMAKE_C_FLAGS=/experimental:deterministic /Brepro /pathmap:$source=/yasumaro-whisper" `
        "-DCMAKE_CXX_FLAGS=/experimental:deterministic /Brepro /pathmap:$source=/yasumaro-whisper /EHsc" `
        -DCMAKE_EXE_LINKER_FLAGS=/Brepro `
        -DWHISPER_BUILD_TESTS=ON `
        -DWHISPER_BUILD_EXAMPLES=ON `
        -DWHISPER_FFMPEG=OFF
    if ($LASTEXITCODE -ne 0) { throw 'CMake configuration failed' }
    & $cmake --build $build --config Release --target whisper-cli
    if ($LASTEXITCODE -ne 0) { throw 'whisper-cli build failed' }
    & (Join-Path (Split-Path $cmake) 'ctest.exe') --test-dir $build -C Release -R test-whisper-cli-prompt-file --output-on-failure
    if ($LASTEXITCODE -ne 0) { throw 'prompt-file contract test failed' }

    $binary = Get-ChildItem -LiteralPath $build -Filter 'whisper-cli.exe' -Recurse | Select-Object -First 1
    if (-not $binary) { throw 'built whisper-cli.exe was not found' }
    New-Item -ItemType Directory -Path (Join-Path $stage 'bin') | Out-Null
    Copy-Item -LiteralPath $binary.FullName -Destination (Join-Path $stage 'bin\whisper-cli.exe')
    Copy-Item -LiteralPath (Join-Path $source 'LICENSE') -Destination (Join-Path $stage 'LICENSE')

    $patchHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $patchPath).Hash.ToLowerInvariant()
    $compilerDefinition = Get-ChildItem -LiteralPath (Join-Path $build 'CMakeFiles') -Filter 'CMakeCXXCompiler.cmake' -Recurse | Select-Object -First 1
    if (-not $compilerDefinition) { throw 'CMake compiler metadata was not found' }
    $versionMatch = Select-String -LiteralPath $compilerDefinition.FullName -Pattern 'set\(CMAKE_CXX_COMPILER_VERSION "([^"]+)"\)'
    if (-not $versionMatch) { throw 'C++ compiler version was not found' }
    $compiler = "MSVC $($versionMatch.Matches[0].Groups[1].Value)"
    [ordered]@{
        schema_version = 1
        upstream_url = $upstreamUrl
        upstream_version = $upstreamVersion
        upstream_commit = $upstreamCommit
        patch_sha256 = $patchHash
        platform = 'windows-x86_64'
        compiler = $compiler
        cmake_options = @('BUILD_SHARED_LIBS=OFF','GGML_NATIVE=OFF','GGML_OPENMP=OFF','GGML_CUDA=OFF','CMAKE_C_FLAGS=/experimental:deterministic /Brepro /pathmap:SOURCE=/yasumaro-whisper','CMAKE_CXX_FLAGS=/experimental:deterministic /Brepro /pathmap:SOURCE=/yasumaro-whisper /EHsc','CMAKE_EXE_LINKER_FLAGS=/Brepro','WHISPER_BUILD_TESTS=ON','WHISPER_BUILD_EXAMPLES=ON','WHISPER_FFMPEG=OFF')
    } | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $stage 'build-metadata.json') -Encoding utf8NoBOM

    $archive = Join-Path $resolvedOutput "yasumaro-whispercpp-$upstreamVersion-windows-x86_64.zip"
    if (Test-Path -LiteralPath $archive) { Remove-Item -LiteralPath $archive -Force }
    $zip = [IO.Compression.ZipFile]::Open($archive, [IO.Compression.ZipArchiveMode]::Create)
    try {
        foreach ($relative in @('bin/whisper-cli.exe', 'LICENSE', 'build-metadata.json')) {
            $entry = $zip.CreateEntry($relative, [IO.Compression.CompressionLevel]::Optimal)
            $entry.LastWriteTime = [DateTimeOffset]::FromUnixTimeSeconds(315532800)
            $input = [IO.File]::OpenRead((Join-Path $stage $relative))
            $output = $entry.Open()
            try { $input.CopyTo($output) } finally { $output.Dispose(); $input.Dispose() }
        }
    }
    finally { $zip.Dispose() }
    $hash = (Get-FileHash -Algorithm SHA256 -LiteralPath $archive).Hash.ToLowerInvariant()
    $size = (Get-Item -LiteralPath $archive).Length
    Write-Output ([ordered]@{ path = $archive; size = $size; sha256 = $hash } | ConvertTo-Json -Compress)
}
finally {
    if (Test-Path -LiteralPath $work) {
        $resolvedWork = (Resolve-Path -LiteralPath $work).Path
        $tempRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath())
        if (-not $resolvedWork.StartsWith($tempRoot, [StringComparison]::OrdinalIgnoreCase)) {
            throw "refusing to remove non-temporary path: $resolvedWork"
        }
        Remove-Item -LiteralPath $resolvedWork -Recurse -Force
    }
}
