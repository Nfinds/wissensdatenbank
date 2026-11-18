# Legt eine Verknuepfung an, die `wb ui` in einem eigenen Fenster von Windows Terminal startet
# (ohne Windows Terminal: in der klassischen Konsole). Aendert nichts am Vault und an keiner Regel.
#
# Aufruf:  powershell -ExecutionPolicy Bypass -File scripts\desktop-verknuepfung.ps1
# Mit eigenen Orten, z. B.
#          ... -Wb <Pfad zu wb.exe> -Vault <Vault>
# Eine vorhandene Verknuepfung gleichen Namens wird ersetzt.
param(
  [string]$Wb     = (Join-Path $PSScriptRoot '..\target\release\wb.exe'),
  [string]$Vault  = (Join-Path $PSScriptRoot '..\vault'),
  [string]$Name   = 'Wissensbank',
  [string]$Ordner = [Environment]::GetFolderPath('Desktop'),
  [string]$Groesse = '160,45',
  [string]$Symbol = "$env:SystemRoot\System32\imageres.dll,7"
)
$ErrorActionPreference = 'Stop'
$Wb = (Resolve-Path -LiteralPath $Wb).ProviderPath
$Vault = (Resolve-Path -LiteralPath $Vault).ProviderPath.TrimEnd('\')
if (-not (Test-Path -LiteralPath $Ordner -PathType Container)) { throw "Zielordner fehlt: $Ordner" }
if ($Name -notmatch '^[\w .-]+$') { throw "Name nur aus Buchstaben, Ziffern, Leerzeichen, Punkt und Strich: $Name" }

$lnk = Join-Path $Ordner "$Name.lnk"
$gab = Test-Path -LiteralPath $lnk
$wt = Join-Path $env:LOCALAPPDATA 'Microsoft\WindowsApps\wt.exe'
$s = (New-Object -ComObject WScript.Shell).CreateShortcut($lnk)
if (Test-Path -LiteralPath $wt) {
  # -w new: immer ein eigenes Fenster, nie ein Tab in einem vorhandenen Terminal.
  $s.TargetPath = $wt
  $s.Arguments = "-w new --size $Groesse --title `"$Name`" --suppressApplicationTitle `"$Wb`" --vault `"$Vault`" ui"
} else {
  $s.TargetPath = $Wb
  $s.Arguments = "--vault `"$Vault`" ui"
}
$s.WorkingDirectory = Split-Path -Parent $Vault
$s.Description = "Wissensbank: Ansichten (wb ui) fuer $Vault"
$s.IconLocation = $Symbol
$s.Save()
if ($gab) { "ersetzt: $lnk" } else { "angelegt: $lnk" }
"  startet: $($s.TargetPath) $($s.Arguments)"
