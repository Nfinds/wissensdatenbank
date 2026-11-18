# Rebuilds every image in docs/img from source: demo vault, `wb ui` views, agent output, hero, mascot.
# Windows, needs Rust, Python 3 with Pillow and Google Chrome (used only as a headless renderer).
#   powershell -ExecutionPolicy Bypass -File docs\screenshots.ps1
$ErrorActionPreference = "Stop"
$root = Split-Path $PSScriptRoot -Parent
$docs = $PSScriptRoot
$img = Join-Path $docs "img"
$work = Join-Path ([IO.Path]::GetTempPath()) "wb-screenshots"
$chrome = "C:\Program Files\Google\Chrome\Application\chrome.exe"
if (Test-Path $work) { Remove-Item $work -Recurse -Force }
New-Item -ItemType Directory -Force $work, $img | Out-Null

function Render-Page([string]$src, [string]$out, [int]$w, [int]$h, [string]$bg = "0B0B0CFF") {
    if (Test-Path $out) { Remove-Item $out }
    $uri = ([Uri](Resolve-Path $src).Path).AbsoluteUri
    $a = @("--headless=new", "--disable-gpu", "--hide-scrollbars", "--force-device-scale-factor=1",
        "--user-data-dir=`"$work\chrome`"", "--default-background-color=$bg", "--window-size=$w,$h",
        "--screenshot=`"$out`"", $uri)
    Start-Process -FilePath $chrome -ArgumentList $a -Wait -WindowStyle Hidden
    if (-not (Test-Path $out)) { throw "no image: $out" }
}

# 1. demo vault (invented projects; file times follow checked_on)
$vault = Join-Path $docs "demo-vault"
if (Test-Path $vault) { Remove-Item $vault -Recurse -Force }
python -I (Join-Path $docs "make_demo_vault.py") $vault
$index = Join-Path $work "index"

# 2. binaries
Push-Location $root
cargo build -q --release -p wb-cli
cargo build -q --release -p wb-tui --features core --example readme_shots
$target = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $root "target" }
$wb = Join-Path $target "release\wb.exe"
& (Join-Path $target "release\examples\readme_shots.exe") $vault $index $work | Out-Null
Pop-Location

# 3. framed terminal shots
$py = Join-Path $docs "screenshots.py"
foreach ($n in "ui-overview", "ui-project", "ui-search", "ui-reader") {
    python -I $py frame "$work\$n.html" "$work\f-$n.html" "wb ui"
    Render-Page "$work\f-$n.html" "$img\$n.png" 1300 720
    python -I $py crop "$img\$n.png"
}
foreach ($n in "cli-context", "cli-search") {
    python -I $py cli "$work\$n.html" $wb $vault $index (Join-Path $docs "shots\$n.txt")
    python -I $py frame "$work\$n.html" "$work\f-$n.html" "agent shell"
    Render-Page "$work\f-$n.html" "$img\$n.png" 1300 760
    python -I $py crop "$img\$n.png"
}

# 4. hero and mascot
Render-Page (Join-Path $docs "hero\hero.html") "$img\hero.png" 1600 760
Render-Page (Join-Path $docs "brand\elephant.svg") "$img\elephant-512.png" 512 512 "00000000"
Get-ChildItem $img | ForEach-Object { "{0,-22} {1,6} KB" -f $_.Name, [int]($_.Length / 1KB) }
