# Reuse the released 7.9.3 DLLs and reconstruct MSVC import libraries from their exports.
$ErrorActionPreference = "Stop"
$root = Join-Path $env:RUNNER_TEMP "qymcad-occt"
New-Item -ItemType Directory -Force $root | Out-Null
$archive = Join-Path $root "upstream.zip"
Invoke-WebRequest "https://github.com/QymIs-Tech/QymCAD/releases/download/v0.1.1-dev.20261026/qymcad-0.1.1-dev.20261026-win64.zip" -OutFile $archive
Expand-Archive $archive (Join-Path $root "portable") -Force
Invoke-WebRequest "https://github.com/Open-Cascade-SAS/OCCT/archive/refs/tags/V7_9_3.tar.gz" -OutFile (Join-Path $root "occt.tar.gz")
tar xf (Join-Path $root "occt.tar.gz") -C $root
if ($LASTEXITCODE -ne 0) { throw "OCCT extraction failed" }
$include = Join-Path $root "include"
$lib = Join-Path $root "lib"
New-Item -ItemType Directory -Force $include,$lib | Out-Null
Get-ChildItem (Join-Path $root "OCCT-7_9_3/src") -File -Recurse | Where-Object { $_.Extension -in ".hxx", ".lxx", ".h", ".gxx", ".pxx" } | Copy-Item -Destination $include
$dlls = Get-ChildItem (Join-Path $root "portable") -Recurse -Filter "TK*.dll"
if ($dlls.Count -lt 20) { throw "OCCT DLLs are missing" }
foreach ($dll in $dlls) {
    $lines = & dumpbin /nologo /exports $dll.FullName
    if ($LASTEXITCODE -ne 0) { throw "Cannot read exports: $dll" }
    $names = @($lines | ForEach-Object { if ($_ -match '^\s*\d+\s+[0-9A-Fa-f]+\s+[0-9A-Fa-f]+\s+(\S+)') { $Matches[1] } })
    if ($names.Count -eq 0) { throw "No exports: $dll" }
    $def = Join-Path $lib ($dll.BaseName + ".def")
    @("LIBRARY $($dll.Name)","EXPORTS") + $names | Set-Content -Encoding ascii $def
    & lib.exe /nologo /machine:x64 "/def:$def" "/out:$(Join-Path $lib ($dll.BaseName + '.lib'))"
    if ($LASTEXITCODE -ne 0) { throw "Import library failed: $dll" }
}
"OCCT_INCLUDE_DIR=$include" | Out-File -Append -Encoding utf8 $env:GITHUB_ENV
"OCCT_LIB_DIR=$lib" | Out-File -Append -Encoding utf8 $env:GITHUB_ENV
"AI_PORTABLE_DIR=$($dlls[0].DirectoryName)" | Out-File -Append -Encoding utf8 $env:GITHUB_ENV
$dlls[0].DirectoryName | Out-File -Append -Encoding utf8 $env:GITHUB_PATH
