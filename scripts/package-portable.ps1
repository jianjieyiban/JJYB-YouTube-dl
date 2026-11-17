param(
  [Parameter(Mandatory = $true)][string]$Executable,
  [Parameter(Mandatory = $true)][string]$OutputDirectory
)
$ErrorActionPreference = 'Stop'
$version = (Get-Content -Raw package.json | ConvertFrom-Json).version
$exeName = "JJYB-YouTube-dl_${version}_x64.exe"
$layout = Join-Path $env:RUNNER_TEMP "jjyb-portable-$([guid]::NewGuid())"
New-Item -ItemType Directory -Path "$layout/jjyb-portable/bin" -Force | Out-Null
New-Item -ItemType Directory -Path $OutputDirectory -Force | Out-Null
Copy-Item -LiteralPath $Executable -Destination "$OutputDirectory/$exeName"
Copy-Item -LiteralPath "$OutputDirectory/$exeName" -Destination "$layout/$exeName"
Set-Content -LiteralPath "$layout/jjyb-portable/.keep" -Value 'Portable application data lives here.' -Encoding utf8
$bin = 'msix-layout/JJYB-YouTube-dl/bin'
foreach ($tool in @('yt-dlp.exe', 'ffmpeg.exe', 'ffprobe.exe', 'deno.exe', 'AtomicParsley.exe', 'metadata.json')) {
  if (-not (Test-Path -LiteralPath "$bin/$tool" -PathType Leaf)) { throw "Missing required tool: $tool" }
}
# Tool files come only from the fresh, hash-checked official-source staging directory.
Copy-Item -Path "$bin/*" -Destination "$layout/jjyb-portable/bin" -Recurse
foreach ($item in @('LICENSE', 'licenses', 'README.md', 'CONTRIBUTING.md', 'docs')) {
  Copy-Item -LiteralPath $item -Destination $layout -Recurse
}
foreach ($item in @('src-tauri/LICENSE-OpenCC-Apache-2.0', 'src-tauri/NOTICE-OpenCC')) {
  Copy-Item -LiteralPath $item -Destination $layout
}
Copy-Item -LiteralPath 'docs/manifest/manifest.json', 'docs/manifest/manifest.sig' -Destination "$layout/jjyb-portable"
$forbidden = Get-ChildItem -LiteralPath $layout -Recurse -Force | Where-Object {
  $_.Name -match '(?i)(cookie|\.pkcs8|\.pfx$|\.pem$|\.log$|\.download$|^\.env|^profile$|^cache$|^\.tmp-)'
}
if ($forbidden) { throw 'Portable staging contains forbidden runtime or secret artifacts' }
& "$layout/jjyb-portable/bin/yt-dlp.exe" --version
if ($LASTEXITCODE -ne 0) { throw 'yt-dlp smoke test failed' }
& "$layout/jjyb-portable/bin/ffmpeg.exe" -version
if ($LASTEXITCODE -ne 0) { throw 'ffmpeg smoke test failed' }
Compress-Archive -Path "$layout/*" -DestinationPath "$OutputDirectory/JJYB-YouTube-dl_${version}_x64-portable.zip"
# Archive tracked sources from the exact build commit, including vendored source and licenses.
git archive --format=zip --prefix="JJYB-YouTube-dl_${version}-source/" --output="$OutputDirectory/JJYB-YouTube-dl_${version}-source.zip" HEAD
if ($LASTEXITCODE -ne 0) { throw 'Complete source archive failed' }
