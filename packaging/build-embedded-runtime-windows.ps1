$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true

$root = (Resolve-Path (Join-Path $PSScriptRoot ".."))
$lock = Join-Path $root "packaging/embedded-runtime-windows-x86_64.lock"
$output = if ($args.Count -gt 0) { $args[0] } else { Join-Path $root "dist/embed-bundle-windows-x86_64" }
$output = [IO.Path]::GetFullPath($output)
$download = Join-Path $output ".wheels"
$pythonRoot = Join-Path $output "lib/python3.12"
$packageVersion = [regex]::Match((Get-Content (Join-Path $root "Cargo.toml") -Raw), '(?m)^version = "([^"]+)"').Groups[1].Value
$sourceRevision = (& git -C $root rev-parse HEAD 2>$null)
if (-not $sourceRevision) { $sourceRevision = "unknown" }
$lockSha256 = (Get-FileHash $lock -Algorithm SHA256).Hash.ToLowerInvariant()

Remove-Item $output -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path (Join-Path $output "bin"), (Join-Path $output "licenses"), $download, $pythonRoot | Out-Null

$lockValues = @{}
foreach ($line in Get-Content $lock) {
    if ($line -match '^(CPYTHON_URL|CPYTHON_SHA256)=(.+)$') {
        $lockValues[$Matches[1]] = $Matches[2]
    }
}

$pythonArchive = Join-Path $download "cpython.tar.gz"
Invoke-WebRequest -Uri $lockValues.CPYTHON_URL -OutFile $pythonArchive
if ((Get-FileHash $pythonArchive -Algorithm SHA256).Hash.ToLowerInvariant() -ne $lockValues.CPYTHON_SHA256) {
    throw "CPython archive checksum mismatch"
}

$pythonExtract = Join-Path $download "cpython"
New-Item -ItemType Directory -Force -Path $pythonExtract | Out-Null
tar.exe -xzf $pythonArchive --strip-components=1 -C $pythonExtract
Copy-Item (Join-Path $pythonExtract "*") $pythonRoot -Recurse -Force

$python = Join-Path $pythonRoot "python.exe"
$pyo3Config = Join-Path $output "pyo3-config"
@"
implementation=CPython
version=3.12
shared=true
abi3=false
lib_name=python312
lib_dir=$pythonRoot\libs
executable=$python
pointer_width=64
"@ | Set-Content -Path $pyo3Config -Encoding utf8NoBOM

foreach ($line in Get-Content $lock) {
    if ($line -notmatch '^WHEEL\s+(\S+)\s+(\S+)\s+(\S+)$') { continue }
    $package = $Matches[1]
    $filename = $Matches[2]
    $digest = $Matches[3]
    $version = [regex]::Match($filename, '^[^-]+-(.+?)-').Groups[1].Value
    & $python -m pip download --only-binary=:all: --no-deps --platform win_amd64 --python-version 3.12 --implementation cp --abi cp312 --dest $download "$package==$version"
    $wheel = Join-Path $download $filename
    if ((Get-FileHash $wheel -Algorithm SHA256).Hash.ToLowerInvariant() -ne $digest) {
        throw "Wheel checksum mismatch: $filename"
    }
    & $python -m pip install --no-index --no-deps --disable-pip-version-check $wheel
}

$env:RUSTFLAGS = "$($env:RUSTFLAGS) -C link-arg=/LIBPATH:$($pythonRoot.Replace('\', '/'))/libs"
$env:PYO3_CONFIG_FILE = $pyo3Config
$env:PYO3_PYTHON = $python
cargo build --release --locked --target x86_64-pc-windows-msvc -p clab-cli
cargo build --release --locked --target x86_64-pc-windows-msvc -p clab-converter --example embed-smoke

$bin = Join-Path $output "bin"
Copy-Item (Join-Path $root "target/x86_64-pc-windows-msvc/release/clab.exe") (Join-Path $bin "clab.exe")
Copy-Item (Join-Path $root "target/x86_64-pc-windows-msvc/release/examples/embed-smoke.exe") (Join-Path $bin "embed-smoke.exe")
foreach ($dll in @("python3.dll", "python312.dll", "vcruntime140.dll", "vcruntime140_1.dll")) {
    $source = Join-Path $pythonRoot $dll
    if (Test-Path $source) { Copy-Item $source $bin -Force }
}
Remove-Item $pyo3Config -Force

Get-ChildItem $output -Recurse -Directory -Filter __pycache__ | Remove-Item -Recurse -Force
Get-ChildItem $output -Recurse -File -Filter direct_url.json | Remove-Item -Force
Get-ChildItem $output -Recurse -File -Filter RECORD | Remove-Item -Force
foreach ($name in @("f2py.exe", "fonttools.exe", "numpy-config.exe", "pyftmerge.exe", "pyftsubset.exe", "ttx.exe")) {
    Remove-Item (Join-Path $pythonRoot $name) -Force -ErrorAction SilentlyContinue
}

foreach ($distInfo in Get-ChildItem $pythonRoot -Recurse -Directory -Filter *.dist-info) {
    foreach ($license in Get-ChildItem $distInfo -File -Filter "LICENSE*") {
        Copy-Item $license (Join-Path $output "licenses/$($distInfo.Name)-$($license.Name)")
    }
    $licenseDirectory = Join-Path $distInfo.FullName "licenses"
    if (Test-Path $licenseDirectory) {
        foreach ($license in Get-ChildItem $licenseDirectory -File) {
            Copy-Item $license (Join-Path $output "licenses/$($distInfo.Name)-$($license.Name)")
        }
    }
}
Remove-Item $download -Recurse -Force

$manifest = [ordered]@{
    schema_version = 1
    product = "clab"
    product_version = $packageVersion
    target = "x86_64-pc-windows-msvc"
    platform = "windows-x86_64"
    build_profile = "release"
    source_revision = $sourceRevision
    runtime = [ordered]@{
        implementation = "CPython"
        version = "3.12"
        lock_file = Split-Path $lock -Leaf
        lock_sha256 = $lockSha256
    }
    layout = [ordered]@{
        executable = "bin/clab.exe"
        embedded_python = "lib/python3.12/python.exe"
        licenses = "licenses/"
        checksums = "checksums.sha256"
    }
}
$manifest | ConvertTo-Json -Depth 5 | Set-Content (Join-Path $output "manifest.json") -Encoding utf8NoBOM

$checksumPath = Join-Path $output "checksums.sha256"
Get-ChildItem $output -Recurse -File | Where-Object Name -ne "checksums.sha256" | ForEach-Object {
    $relative = $_.FullName.Substring($output.Length + 1).Replace('\', '/')
    "{0}  {1}" -f (Get-FileHash $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant(), $relative
} | Sort-Object | Set-Content $checksumPath -Encoding ascii

$archive = "$output.zip"
if (Test-Path $archive) { Remove-Item $archive -Force }
$parent = Split-Path $output -Parent
$name = Split-Path $output -Leaf
Compress-Archive -Path (Join-Path $parent $name) -DestinationPath $archive
(Get-FileHash $archive -Algorithm SHA256).Hash.ToLowerInvariant() + "  " + (Split-Path $archive -Leaf) | Set-Content "$archive.sha256" -Encoding ascii
