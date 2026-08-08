$ErrorActionPreference = 'Stop'

$workspace = Split-Path -Parent $PSScriptRoot
$fixtureDirectory = Join-Path $workspace 'target\manual-stage1'
$fixturePath = Join-Path $fixtureDirectory 'stage1-utf8.txt'

New-Item -ItemType Directory -Path $fixtureDirectory -Force | Out-Null
$fixtureBytes = [System.Convert]::FromBase64String(
  'UmVhZGxvb20g6Zi25q61IDENCuWIneWni+S4reaWh+WGheWuuQ0K'
)
[System.IO.File]::WriteAllBytes($fixturePath, $fixtureBytes)

$fixturePath
