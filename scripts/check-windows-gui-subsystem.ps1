param(
    [string]$Executable = "target\release\readloom-slint.exe"
)

$ErrorActionPreference = 'Stop'
$resolved = (Resolve-Path -LiteralPath $Executable).Path
$bytes = [System.IO.File]::ReadAllBytes($resolved)
if ($bytes.Length -lt 256 -or $bytes[0] -ne 0x4d -or $bytes[1] -ne 0x5a) {
    throw "Not a valid PE executable: $resolved"
}
$peOffset = [BitConverter]::ToInt32($bytes, 0x3c)
if ($peOffset -lt 0 -or $peOffset + 96 -ge $bytes.Length) {
    throw "Invalid PE header offset: $resolved"
}
if ([BitConverter]::ToUInt32($bytes, $peOffset) -ne 0x00004550) {
    throw "Missing PE signature: $resolved"
}
$optionalHeader = $peOffset + 24
$subsystem = [BitConverter]::ToUInt16($bytes, $optionalHeader + 68)
if ($subsystem -ne 2) {
    throw "Expected Windows GUI subsystem (2), found $subsystem. Double-clicking may create a console window."
}

[pscustomobject]@{
    executable = $resolved
    subsystem = $subsystem
    subsystemName = 'Windows GUI'
} | ConvertTo-Json
