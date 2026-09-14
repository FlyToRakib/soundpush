<#
.SYNOPSIS
    Installs or removes the test-signed SoundPush Virtual Audio driver on a TEST computer.

.DESCRIPTION
    For developers only. Never run this on a computer you rely on: the driver is test-signed,
    so Windows loads it only after `bcdedit /set testsigning on` (see README.md), which
    lowers the computer's protection against unsigned kernel code.

    Install:
      1. Adds the driver package to the driver store (pnputil /add-driver).
      2. Creates the "SoundPush Virtual Audio" software device (SWD\SoundPush\SoundPushVirtualAudio)
         with the SwDevice API. The device persists across restarts; Windows then matches it to
         the INF and loads the driver. No devcon is needed.

    Uninstall:
      1. Removes the software device.
      2. Deletes the driver package from the driver store.

    Run from an elevated PowerShell in the folder that holds SoundPushVirtualAudio.inf/.sys/.cat
    (the CI artifact), or pass -PackagePath.

.EXAMPLE
    .\Install-SoundPushDriver.ps1
.EXAMPLE
    .\Install-SoundPushDriver.ps1 -Uninstall
#>
[CmdletBinding()]
param(
    [switch] $Uninstall,
    [string] $PackagePath = $PSScriptRoot
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$InfName = 'SoundPushVirtualAudio.inf'
$Enumerator = 'SoundPush'
$InstanceId = 'SoundPushVirtualAudio'
$HardwareId = 'SWD\SoundPushVirtualAudio'
$DeviceDescription = 'SoundPush Virtual Audio'

$principal = [Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()
if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    throw 'Run this script from an elevated PowerShell (Run as administrator).'
}

# SwDeviceCreate is asynchronous and reports through a callback; this small wrapper waits for it.
# SWDeviceLifetimeParentPresent keeps the device after the handle is closed (and across restarts)
# until it is removed with -Uninstall.
Add-Type -Language CSharp -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Threading;

public static class SoundPushSwDevice
{
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    struct SW_DEVICE_CREATE_INFO
    {
        public uint cbSize;
        public string pszInstanceId;
        public IntPtr pszzHardwareIds;
        public IntPtr pszzCompatibleIds;
        public IntPtr pContainerId;
        public uint CapabilityFlags;
        public string pszDeviceDescription;
        public string pszDeviceLocation;
        public IntPtr pSecurityDescriptor;
    }

    [UnmanagedFunctionPointer(CallingConvention.StdCall, CharSet = CharSet.Unicode)]
    delegate void SwDeviceCreateCallback(IntPtr hSwDevice, int CreateResult, IntPtr pContext, string pszDeviceInstanceId);

    [DllImport("cfgmgr32.dll", CharSet = CharSet.Unicode)]
    static extern int SwDeviceCreate(string pszEnumeratorName, string pszParentDeviceInstance,
        ref SW_DEVICE_CREATE_INFO pCreateInfo, uint cPropertyCount, IntPtr pProperties,
        SwDeviceCreateCallback pCallback, IntPtr pContext, out IntPtr phSwDevice);

    [DllImport("cfgmgr32.dll")]
    static extern int SwDeviceSetLifetime(IntPtr hSwDevice, int Lifetime);

    [DllImport("cfgmgr32.dll")]
    static extern void SwDeviceClose(IntPtr hSwDevice);

    const uint SWDeviceCapabilitiesSilentInstall = 0x2;
    const uint SWDeviceCapabilitiesDriverRequired = 0x8;
    const int SWDeviceLifetimeHandle = 0;
    const int SWDeviceLifetimeParentPresent = 1;

    // Creates (or opens, if it already exists) the device and sets how long it lives.
    // Returns the device instance id.
    public static string Open(string enumerator, string instanceId, string hardwareId, string description, bool persist)
    {
        IntPtr hardwareIds = Marshal.StringToHGlobalUni(hardwareId + "\0\0");
        try
        {
            var info = new SW_DEVICE_CREATE_INFO();
            info.cbSize = (uint)Marshal.SizeOf(typeof(SW_DEVICE_CREATE_INFO));
            info.pszInstanceId = instanceId;
            info.pszzHardwareIds = hardwareIds;
            info.CapabilityFlags = SWDeviceCapabilitiesSilentInstall | SWDeviceCapabilitiesDriverRequired;
            info.pszDeviceDescription = description;

            int result = 0;
            string createdId = null;
            using (var done = new ManualResetEvent(false))
            {
                SwDeviceCreateCallback callback = (h, r, c, id) => { result = r; createdId = id; done.Set(); };
                IntPtr handle;
                int hr = SwDeviceCreate(enumerator, "HTREE\\ROOT\\0", ref info, 0, IntPtr.Zero, callback, IntPtr.Zero, out handle);
                if (hr < 0) Marshal.ThrowExceptionForHR(hr);
                try
                {
                    if (!done.WaitOne(TimeSpan.FromSeconds(30)))
                        throw new TimeoutException("Windows did not finish creating the device within 30 seconds.");
                    if (result < 0) Marshal.ThrowExceptionForHR(result);
                    hr = SwDeviceSetLifetime(handle, persist ? SWDeviceLifetimeParentPresent : SWDeviceLifetimeHandle);
                    if (hr < 0) Marshal.ThrowExceptionForHR(hr);
                }
                finally
                {
                    // With SWDeviceLifetimeHandle this removes the device.
                    SwDeviceClose(handle);
                }
                GC.KeepAlive(callback);
            }
            return createdId;
        }
        finally
        {
            Marshal.FreeHGlobal(hardwareIds);
        }
    }
}
'@

function Get-SoundPushDriverPackages {
    # Published names (oemNN.inf) of every SoundPush Virtual Audio package in the driver store.
    $output = & pnputil.exe /enum-drivers
    $packages = @()
    $published = $null
    foreach ($line in $output) {
        if ($line -match '^\s*Published Name\s*:\s*(\S+)') { $published = $Matches[1] }
        elseif ($line -match '^\s*Original Name\s*:\s*(\S+)' -and $Matches[1] -ieq $InfName -and $published) {
            $packages += $published
        }
    }
    return $packages
}

if ($Uninstall) {
    Write-Host 'Removing the SoundPush Virtual Audio device...'
    try {
        # Reopen the device with a handle lifetime; closing the handle removes it.
        [void][SoundPushSwDevice]::Open($Enumerator, $InstanceId, $HardwareId, $DeviceDescription, $false)
    } catch {
        Write-Warning "Could not remove the software device: $($_.Exception.Message)"
    }
    foreach ($package in Get-SoundPushDriverPackages) {
        Write-Host "Deleting driver package $package..."
        & pnputil.exe /delete-driver $package /uninstall /force
        if ($LASTEXITCODE -ne 0) { Write-Warning "pnputil /delete-driver $package exited with $LASTEXITCODE" }
    }
    Write-Host 'Done. Restart if Sound settings still list SoundPush endpoints.'
    return
}

$inf = Join-Path $PackagePath $InfName
foreach ($file in @($inf, (Join-Path $PackagePath 'SoundPushVirtualAudio.sys'), (Join-Path $PackagePath 'SoundPushVirtualAudio.cat'))) {
    if (-not (Test-Path $file)) { throw "Missing $file" }
}

$testSigning = (& bcdedit.exe /enum '{current}') -match 'testsigning\s+Yes'
if (-not $testSigning) {
    Write-Warning 'Test signing is off, so Windows will refuse to load this driver (Code 52). See README.md.'
}

Write-Host 'Adding the driver package to the driver store...'
& pnputil.exe /add-driver $inf /install
if ($LASTEXITCODE -ne 0 -and $LASTEXITCODE -ne 3010) {
    throw "pnputil /add-driver failed with exit code $LASTEXITCODE"
}

Write-Host 'Creating the SoundPush Virtual Audio device...'
$id = [SoundPushSwDevice]::Open($Enumerator, $InstanceId, $HardwareId, $DeviceDescription, $true)
Write-Host "Device: $id"
Write-Host 'Done. Sound settings should now list "SoundPush Microphone Feed" (output) and "SoundPush Microphone" (input).'
