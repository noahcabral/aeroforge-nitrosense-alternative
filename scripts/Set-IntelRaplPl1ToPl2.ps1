param(
    [switch]$Apply,
    [switch]$RestoreLatest,
    [string]$RestoreFile,
    [string]$DevicePath = "\\.\WinRing0_1_2_0",
    [int]$TelemetryVerifySeconds = 8
)

$ErrorActionPreference = "Stop"

$nativeSource = @"
using System;
using System.Runtime.InteropServices;

public static class WinRingMsrNative
{
    [StructLayout(LayoutKind.Sequential)]
    public struct MsrWriteInput
    {
        public uint Register;
        public uint Eax;
        public uint Edx;
    }

    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    public static extern IntPtr CreateFileW(
        string lpFileName,
        uint dwDesiredAccess,
        uint dwShareMode,
        IntPtr lpSecurityAttributes,
        uint dwCreationDisposition,
        uint dwFlagsAndAttributes,
        IntPtr hTemplateFile);

    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern bool CloseHandle(IntPtr hObject);

    [DllImport("kernel32.dll", EntryPoint = "DeviceIoControl", SetLastError = true)]
    public static extern bool DeviceIoControlRead(
        IntPtr hDevice,
        uint dwIoControlCode,
        ref uint lpInBuffer,
        uint nInBufferSize,
        out ulong lpOutBuffer,
        uint nOutBufferSize,
        out uint lpBytesReturned,
        IntPtr lpOverlapped);

    [DllImport("kernel32.dll", EntryPoint = "DeviceIoControl", SetLastError = true)]
    public static extern bool DeviceIoControlWrite(
        IntPtr hDevice,
        uint dwIoControlCode,
        ref MsrWriteInput lpInBuffer,
        uint nInBufferSize,
        IntPtr lpOutBuffer,
        uint nOutBufferSize,
        out uint lpBytesReturned,
        IntPtr lpOverlapped);
}
"@

Add-Type -TypeDefinition $nativeSource

$GenericRead = [Convert]::ToUInt32("80000000", 16)
$GenericWrite = [Convert]::ToUInt32("40000000", 16)
$OpenExisting = [uint32]3
$FileAttributeNormal = [uint32]0x80
$InvalidHandleValue = [IntPtr]::new(-1)
$MsrRaplPowerUnit = [uint32]0x606
$MsrPkgPowerLimit = [uint32]0x610
$Pl1Mask = [uint64]0x7fff
$LowDwordMask = [Convert]::ToUInt64("FFFFFFFF", 16)
$NonPl1Mask = [Convert]::ToUInt64("FFFFFFFFFFFF8000", 16)

function New-CtlCode {
    param(
        [uint32]$DeviceType,
        [uint32]$Function,
        [uint32]$Method,
        [uint32]$Access
    )

    [uint32](($DeviceType -shl 16) -bor ($Access -shl 14) -bor ($Function -shl 2) -bor $Method)
}

$IoctlReadMsr = New-CtlCode -DeviceType 40000 -Function 0x821 -Method 0 -Access 0
$IoctlWriteMsr = New-CtlCode -DeviceType 40000 -Function 0x822 -Method 0 -Access 0

function Format-Hex64 {
    param([uint64]$Value)
    "0x{0:X16}" -f $Value
}

function ConvertFrom-Hex64 {
    param([string]$Value)
    $trimmed = $Value.Trim()
    if ($trimmed.StartsWith("0x", [StringComparison]::OrdinalIgnoreCase)) {
        $trimmed = $trimmed.Substring(2)
    }
    [Convert]::ToUInt64($trimmed, 16)
}

function Open-WinRingDevice {
    $handle = [WinRingMsrNative]::CreateFileW(
        $DevicePath,
        ($GenericRead -bor $GenericWrite),
        0,
        [IntPtr]::Zero,
        $OpenExisting,
        $FileAttributeNormal,
        [IntPtr]::Zero)

    if ($handle -eq $InvalidHandleValue) {
        $errorCode = [Runtime.InteropServices.Marshal]::GetLastWin32Error()
        throw "Failed to open $DevicePath. Win32=$errorCode. Run elevated and confirm AeroForgeService/WinRing0 is installed."
    }

    $handle
}

function Read-Msr {
    param(
        [IntPtr]$Handle,
        [uint32]$Register
    )

    $output = [uint64]0
    $bytesReturned = [uint32]0
    $inputRegister = [uint32]$Register
    $ok = [WinRingMsrNative]::DeviceIoControlRead(
        $Handle,
        $IoctlReadMsr,
        [ref]$inputRegister,
        4,
        [ref]$output,
        8,
        [ref]$bytesReturned,
        [IntPtr]::Zero)

    if (-not $ok) {
        $errorCode = [Runtime.InteropServices.Marshal]::GetLastWin32Error()
        throw "Read MSR $(Format-Hex64 $Register) failed. Win32=$errorCode."
    }

    $output
}

function Write-Msr {
    param(
        [IntPtr]$Handle,
        [uint32]$Register,
        [uint64]$Value
    )

    $input = [WinRingMsrNative+MsrWriteInput]::new()
    $input.Register = $Register
    $input.Eax = [uint32]($Value -band $LowDwordMask)
    $input.Edx = [uint32](($Value -shr 32) -band $LowDwordMask)
    $bytesReturned = [uint32]0
    $ok = [WinRingMsrNative]::DeviceIoControlWrite(
        $Handle,
        $IoctlWriteMsr,
        [ref]$input,
        [Runtime.InteropServices.Marshal]::SizeOf([type][WinRingMsrNative+MsrWriteInput]),
        [IntPtr]::Zero,
        0,
        [ref]$bytesReturned,
        [IntPtr]::Zero)

    if (-not $ok) {
        $errorCode = [Runtime.InteropServices.Marshal]::GetLastWin32Error()
        throw "Write MSR $(Format-Hex64 $Register) failed. Win32=$errorCode."
    }
}

function Get-RaplPowerUnitW {
    param([uint64]$UnitRaw)
    $powerExponent = [int]($UnitRaw -band [uint64]0x0f)
    1.0 / [Math]::Pow(2.0, $powerExponent)
}

function Get-PackageLimitState {
    param(
        [uint64]$Raw,
        [double]$PowerUnitW
    )

    $pl1Raw = [uint64]($Raw -band $Pl1Mask)
    $pl2Raw = [uint64](($Raw -shr 32) -band $Pl1Mask)
    [pscustomobject]@{
        RawHex = Format-Hex64 $Raw
        Pl1Raw = $pl1Raw
        Pl1W = [math]::Round($pl1Raw * $PowerUnitW, 3)
        Pl1Enabled = ((($Raw -shr 15) -band [uint64]1) -eq 1)
        Pl2Raw = $pl2Raw
        Pl2W = [math]::Round($pl2Raw * $PowerUnitW, 3)
        Pl2Enabled = ((($Raw -shr 47) -band [uint64]1) -eq 1)
        Locked = ((($Raw -shr 63) -band [uint64]1) -eq 1)
    }
}

function Get-LatestBackup {
    param([string]$BackupDir)
    Get-ChildItem -LiteralPath $BackupDir -Filter 'rapl-pkg-power-limit-*.json' -ErrorAction SilentlyContinue |
        Sort-Object LastWriteTime -Descending |
        Select-Object -First 1
}

function Read-ServiceTelemetry {
    $path = "C:\ProgramData\AeroForge\Service\state\lowlevel.json"
    if (-not (Test-Path -LiteralPath $path)) {
        return $null
    }

    try {
        Get-Content -Raw -LiteralPath $path | ConvertFrom-Json
    } catch {
        $null
    }
}

$backupDir = "C:\ProgramData\AeroForge\Service\state\rapl-backups"
$handle = [IntPtr]::Zero

try {
    $handle = Open-WinRingDevice
    $unitRaw = Read-Msr -Handle $handle -Register $MsrRaplPowerUnit
    $powerUnitW = Get-RaplPowerUnitW -UnitRaw $unitRaw
    $currentRaw = Read-Msr -Handle $handle -Register $MsrPkgPowerLimit
    $current = Get-PackageLimitState -Raw $currentRaw -PowerUnitW $powerUnitW

    if ($RestoreLatest -or $RestoreFile) {
        if ($RestoreLatest) {
            $latest = Get-LatestBackup -BackupDir $backupDir
            if ($null -eq $latest) {
                throw "No RAPL backup file found under $backupDir."
            }
            $RestoreFile = $latest.FullName
        }

        $backup = Get-Content -Raw -LiteralPath $RestoreFile | ConvertFrom-Json
        $targetRaw = ConvertFrom-Hex64 $backup.OriginalRawHex
        $target = Get-PackageLimitState -Raw $targetRaw -PowerUnitW $powerUnitW

        [pscustomobject]@{
            Mode = if ($Apply) { "Restore apply" } else { "Restore dry-run" }
            RestoreFile = $RestoreFile
            CurrentRaw = $current.RawHex
            TargetRaw = $target.RawHex
            CurrentPl1W = $current.Pl1W
            TargetPl1W = $target.Pl1W
            CurrentPl2W = $current.Pl2W
            TargetPl2W = $target.Pl2W
            Locked = $current.Locked
        } | Format-List

        if ($Apply) {
            if ($current.Locked) {
                throw "MSR 0x610 is locked; refusing restore write."
            }
            Write-Msr -Handle $handle -Register $MsrPkgPowerLimit -Value $targetRaw
            Start-Sleep -Milliseconds 250
            $afterRaw = Read-Msr -Handle $handle -Register $MsrPkgPowerLimit
            if ($afterRaw -ne $targetRaw) {
                throw "Restore write did not stick. Expected $(Format-Hex64 $targetRaw), read back $(Format-Hex64 $afterRaw)."
            }
            Write-Host "Restored package power-limit MSR to $($target.RawHex)."
        }

        return
    }

    if ($current.Locked) {
        [pscustomobject]@{
            Mode = "Dry-run"
            CurrentRaw = $current.RawHex
            PowerUnitW = $powerUnitW
            CurrentPl1W = $current.Pl1W
            CurrentPl2W = $current.Pl2W
            Locked = $current.Locked
            Result = "Locked, no write possible"
        } | Format-List
        throw "MSR 0x610 is locked; refusing to write."
    }

    if ($current.Pl2Raw -eq 0) {
        throw "PL2 raw limit is 0; refusing to copy it to PL1."
    }

    $targetRaw = [uint64](($currentRaw -band $NonPl1Mask) -bor $current.Pl2Raw)
    $target = Get-PackageLimitState -Raw $targetRaw -PowerUnitW $powerUnitW

    [pscustomObject]@{
        Mode = if ($Apply) { "Apply PL1=PL2" } else { "Dry-run PL1=PL2" }
        CurrentRaw = $current.RawHex
        TargetRaw = $target.RawHex
        PowerUnitW = $powerUnitW
        CurrentPl1W = $current.Pl1W
        TargetPl1W = $target.Pl1W
        Pl2W = $current.Pl2W
        Pl1Enabled = $current.Pl1Enabled
        Pl2Enabled = $current.Pl2Enabled
        Locked = $current.Locked
    } | Format-List

    if (-not $Apply) {
        Write-Host "Dry-run only. Re-run with -Apply to write PL1 to the current PL2 value."
        return
    }

    New-Item -ItemType Directory -Force -Path $backupDir | Out-Null
    $backupPath = Join-Path $backupDir ("rapl-pkg-power-limit-{0}.json" -f (Get-Date -Format "yyyyMMdd-HHmmss"))
    [ordered]@{
        Timestamp = (Get-Date).ToString("o")
        OriginalRawHex = $current.RawHex
        TargetRawHex = $target.RawHex
        PowerUnitW = $powerUnitW
        OriginalPl1W = $current.Pl1W
        TargetPl1W = $target.Pl1W
        Pl2W = $current.Pl2W
        DevicePath = $DevicePath
    } | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath $backupPath -Encoding UTF8

    Write-Msr -Handle $handle -Register $MsrPkgPowerLimit -Value $targetRaw
    Start-Sleep -Milliseconds 250
    $afterRaw = Read-Msr -Handle $handle -Register $MsrPkgPowerLimit
    $after = Get-PackageLimitState -Raw $afterRaw -PowerUnitW $powerUnitW

    if (($afterRaw -band $Pl1Mask) -ne $current.Pl2Raw) {
        Write-Msr -Handle $handle -Register $MsrPkgPowerLimit -Value $currentRaw
        throw "PL1 write verification failed. Restored original MSR. Expected PL1 raw $($current.Pl2Raw), read back $($after.Pl1Raw)."
    }

    if (($afterRaw -band $NonPl1Mask) -ne ($targetRaw -band $NonPl1Mask)) {
        Write-Warning "PL1 updated, but firmware changed non-PL1 bits during readback. Current raw is $($after.RawHex). Backup is $backupPath."
    }

    [pscustomobject]@{
        Result = "Verified"
        BackupPath = $backupPath
        AfterRaw = $after.RawHex
        AfterPl1W = $after.Pl1W
        AfterPl2W = $after.Pl2W
    } | Format-List

    $lastTelemetry = $null
    for ($i = 0; $i -lt $TelemetryVerifySeconds; $i++) {
        Start-Sleep -Seconds 1
        $lastTelemetry = Read-ServiceTelemetry
        if ($null -ne $lastTelemetry -and $lastTelemetry.packagePl1W -ge ($target.Pl1W - 0.5)) {
            break
        }
    }

    if ($null -ne $lastTelemetry) {
        [pscustomobject]@{
            ServiceTelemetryPl1W = $lastTelemetry.packagePl1W
            ServiceTelemetryPl2W = $lastTelemetry.packagePl2W
            ServiceTelemetryLocked = $lastTelemetry.packagePowerLimitLocked
            ServiceTelemetryUpdatedAt = $lastTelemetry.updatedAt
        } | Format-List
    } else {
        Write-Warning "No AeroForge lowlevel telemetry file was available for service-side verification."
    }
} finally {
    if ($handle -ne [IntPtr]::Zero -and $handle -ne $InvalidHandleValue) {
        [void][WinRingMsrNative]::CloseHandle($handle)
    }
}
