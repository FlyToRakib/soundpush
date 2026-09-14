<#
.SYNOPSIS
    Compiles, links and packages the driver without the "Windows Driver Kit" Visual Studio
    component (no administrator rights needed).

.DESCRIPTION
    The MSBuild project needs the WDK's Visual Studio component for its kernel-mode platform
    toolset. On a computer without it (and without rights to add it), this script builds the
    same sources with cl.exe and link.exe using the WDK's kernel-mode settings, then runs the
    packaging checks:

      stampinf -> infverif /w (Windows driver rules) -> inf2cat -> ApiValidator

    Requirements: Visual Studio 2022 (or Build Tools) with the x64 C++ tools, and the NuGet
    packages restored into .\packages (nuget restore packages.config -PackagesDirectory packages).
    x64 only: ARM64 needs the MSVC ARM64 tools; the CI workflow builds both through MSBuild.

    It does not sign or install anything.

.EXAMPLE
    .\scripts\Test-DriverBuild.ps1
#>
[CmdletBinding()]
param(
    [ValidateSet('Release', 'Debug')] [string] $Configuration = 'Release',
    # Also run the compiler's static analysis (/analyze) and fail on its warnings.
    [switch] $Analyze
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$root = Split-Path $PSScriptRoot -Parent
$kitPackageVersion = '10.0.28000.2526'  # keep in sync with packages.config
$kit = '10.0.28000.0'
$wdk = Join-Path $root "packages\Microsoft.Windows.WDK.x64.$kitPackageVersion\c"
$sdk = Join-Path $root "packages\Microsoft.Windows.SDK.CPP.$kitPackageVersion\c"
if (-not (Test-Path $wdk)) { throw "WDK package not found at $wdk. Run: nuget restore packages.config -PackagesDirectory packages" }

$out = Join-Path $root "build\direct\x64\$Configuration"
$obj = Join-Path $out 'obj'
$package = Join-Path $out 'package'
New-Item -ItemType Directory -Force $obj, $package | Out-Null

# MSVC x64 environment (PATH for cl/link). Include and library paths are given explicitly below.
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
$vs = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if (-not $vs) { throw 'Visual Studio C++ x64 tools not found.' }
$env:PATH = "$(Split-Path $vswhere);$env:PATH"  # vcvars looks for vswhere on PATH
cmd /c "`"$vs\VC\Auxiliary\Build\vcvars64.bat`" >nul && set" | ForEach-Object {
    if ($_ -match '^([^=]+)=(.*)$') { Set-Item -Path "env:$($Matches[1])" -Value $Matches[2] }
}

function Invoke-Tool([string] $Name, [string] $Exe, [string[]] $Arguments) {
    Write-Host "== $Name"
    & $Exe @Arguments
    if ($LASTEXITCODE -ne 0) { throw "$Name failed with exit code $LASTEXITCODE" }
}

# ---------------------------------------------------------------- compile
# Mirrors WindowsDriver.Common/KernelMode/x64 props: /kernel, no exceptions or RTTI,
# /GS with the kernel fast-fail cookie library, wchar_t as a typedef, warnings as errors.
$optimize = if ($Configuration -eq 'Release') { @('/O2', '/Oi') } else { @('/Od', '/D', 'DBG=1') }
# Kit headers are "external": their warnings (e.g. C28301 in wdm.h) are not ours to fix.
if ($Analyze) { $optimize += @('/analyze', '/analyze:stacksize1024', '/analyze:external-') }
$cl = @(
    '/nologo', '/c', '/kernel', '/std:c++17', '/W4', '/WX', '/sdl', '/Zi', '/FS',
    '/GS', '/GF', '/Gy', '/GR-', '/EHs-c-', '/Zc:wchar_t-', '/cbstring', '/d2epilogunwind',
    '/D', '_WIN64', '/D', '_AMD64_', '/D', 'AMD64', '/D', 'DEPRECATE_DDK_FUNCTIONS=1', '/D', 'MSC_NOOPT',
    '/D', '_WIN32_WINNT=0x0A00', '/D', 'WINVER=0x0A00', '/D', 'WINNT=1', '/D', 'NTDDI_VERSION=0x0A000008',
    '/X', '/external:W0',
    '/external:I', "$wdk\Include\$kit\km\crt", '/external:I', "$wdk\Include\$kit\km",
    '/external:I', "$wdk\Include\$kit\shared", '/external:I', "$sdk\Include\$kit\shared",
    "/Fo$obj\", "/Fd$obj\SoundPushVirtualAudio.pdb"
) + $optimize + (Get-ChildItem (Join-Path $root 'src\*.cpp') | ForEach-Object FullName)
Invoke-Tool 'cl (x64 kernel mode)' 'cl.exe' $cl

# ---------------------------------------------------------------- link
$lib = "$wdk\Lib\$kit\km\x64"
$sys = Join-Path $package 'SoundPushVirtualAudio.sys'
$link = @(
    '/nologo', '/WX', '/DRIVER', '/KERNEL', '/SUBSYSTEM:NATIVE,10.00', '/ENTRY:GsDriverEntry',
    '/NODEFAULTLIB', '/OPT:REF', '/OPT:ICF', '/MERGE:_TEXT=.text', '/MERGE:_PAGE=PAGE',
    '/RELEASE', '/DEBUG', '/NXCOMPAT', '/DYNAMICBASE', '/osversion:10.0', '/debugtype:pdata',
    "/OUT:$sys", "/PDB:$out\SoundPushVirtualAudio.pdb",
    "$lib\BufferOverflowFastFailK.lib", "$lib\ntoskrnl.lib", "$lib\hal.lib", "$lib\wmilib.lib", "$lib\portcls.lib"
) + (Get-ChildItem "$obj\*.obj" | ForEach-Object FullName)
Invoke-Tool 'link' 'link.exe' $link

# ---------------------------------------------------------------- package checks
$inf = Join-Path $package 'SoundPushVirtualAudio.inf'
Copy-Item (Join-Path $root 'SoundPushVirtualAudio.inf') $inf -Force
Invoke-Tool 'stampinf' "$wdk\bin\$kit\x64\stampinf.exe" @('-f', $inf, '-a', 'amd64', '-d', '*', '-v', '1.0.0.0')
Invoke-Tool 'infverif /w' "$wdk\tools\$kit\x64\infverif.exe" @('/w', '/v', $inf)
Invoke-Tool 'inf2cat' "$wdk\bin\$kit\x86\Inf2Cat.exe" @("/driver:$package", '/os:10_VB_X64', '/uselocaltime')
$ddis = "$wdk\build\$kit\universalDDIs\x64"
Invoke-Tool 'ApiValidator' "$wdk\bin\$kit\x64\apivalidator.exe" @(
    "-DriverPackagePath:$package",
    "-SupportedApiXmlFiles:$ddis\UniversalDDIs.xml",
    "-ModuleWhiteListXmlFiles:$ddis\ModuleWhitelist.xml"
)

Write-Host "Built and checked: $package"
Get-ChildItem $package | Format-Table Name, Length
