# Signs calmdesk.exe and the installer with a code-signing certificate from the user's
# certificate store. Run after `cargo build --release -p calmdesk` and before building the
# installer (it embeds calmdesk.exe), then again for the installer:
#
#   $env:CALMDESK_CERT = "<certificate thumbprint>"
#   tools\sign.ps1 target\release\calmdesk.exe
#   cargo build --release -p calmdesk-setup
#   tools\sign.ps1 target\release\calmdesk-setup.exe
#
# Without CALMDESK_CERT it does nothing, so unsigned dev builds keep working.
param([Parameter(Mandatory)][string[]]$files)

if (-not $env:CALMDESK_CERT) { "CALMDESK_CERT not set: not signing."; exit 0 }
$signtool = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\*\x64\signtool.exe" -ErrorAction SilentlyContinue |
  Sort-Object FullName -Descending | Select-Object -First 1
if (-not $signtool) { throw "signtool.exe not found: install the Windows SDK." }
& $signtool.FullName sign /sha1 $env:CALMDESK_CERT /fd SHA256 /tr http://timestamp.digicert.com /td SHA256 /d Calmdesk @files
exit $LASTEXITCODE
