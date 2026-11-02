# Wright sandbox bootstrap (WR-14a): WSL 2 + Ubuntu + Bubblewrap.
#
# Generated from tools/src/setup.rs (setup::bootstrap_ps1()). Do not edit
# by hand -- regenerate the copy in packaging/windows/ from that source;
# a unit test pins the two together.
#
# What it does (in order): elevate, enable the WSL 2 features, install
# Ubuntu without launching it, apt-install the isolation utilities as
# root, then run the exact bwrap namespace probe Wright's sandbox uses.
# It does NOT install project toolchains inside WSL (Rust/Node/Python
# there are the user's choice) and never restarts the machine.

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

function Write-Step([string]$name) {
    Write-Host ("[wright bootstrap] " + $name)
}

# --- 1. elevation (the features need an administrator) -------------------
$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
$principal = New-Object Security.Principal.WindowsPrincipal($identity)
if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    Write-Step "re-launching elevated (answer the UAC prompt)"
    Start-Process -FilePath 'powershell.exe' `
        -ArgumentList @('-NoProfile','-ExecutionPolicy','Bypass','-File', $PSCommandPath) `
        -Verb RunAs -Wait
    exit 0
}
Write-Step "already administrator"

# --- 2. the WSL 2 features (idempotent; no restart forced) ----------------
Write-Step "enabling Microsoft-Windows-Subsystem-Linux"
dism.exe /online /enable-feature /featurename:Microsoft-Windows-Subsystem-Linux /all /norestart | Out-Null
Write-Step "enabling VirtualMachinePlatform"
dism.exe /online /enable-feature /featurename:VirtualMachinePlatform /all /norestart | Out-Null
Write-Step "default WSL version 2"
wsl.exe --set-default-version 2 2>$null | Out-Null

# --- 3. an Ubuntu distribution, unattended -------------------------------
$distro = $null
try {
    $distro = @(wsl.exe --list --quiet) | Where-Object { $_ -match '\S' } | Select-Object -First 1
} catch { $distro = $null }
if ($distro) {
    Write-Step ("a distribution is already installed: " + $distro)
} else {
    Write-Step "installing Ubuntu (no first-run launch)"
    wsl.exe --install --distribution Ubuntu --no-launch
    if ($LASTEXITCODE -ne 0) {
        # Older WSL versions lack --no-launch: install plainly and keep
        # going; the apt step below still runs as root.
        Write-Step "this WSL has no --no-launch; installing plainly"
        wsl.exe --install --distribution Ubuntu
    }
}

# --- 4. the isolation utilities, as root ---------------------------------
Write-Step "apt-get update + bubblewrap util-linux coreutils (as root, unattended)"
wsl.exe --distribution Ubuntu --user root --exec sh -c "apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y bubblewrap util-linux coreutils"
if ($LASTEXITCODE -ne 0) { throw "apt-get failed; run it by hand: wsl -d Ubuntu -u root -- apt-get install -y bubblewrap util-linux coreutils" }

# --- 5. verify with Wright's own probe -----------------------------------
Write-Step "verifying: the bwrap namespace probe Wright's sandbox runs"
wsl.exe --exec bwrap --die-with-parent --unshare-user --unshare-pid --unshare-ipc --unshare-uts --unshare-net --ro-bind / / --dev /dev --proc /proc -- /bin/true
if ($LASTEXITCODE -ne 0) { throw 'the bwrap probe failed; run wright doctor for the exact error' }

Write-Step "done: WSL 2 + Ubuntu + Bubblewrap ready"
Write-Host "WRIGHT-BOOTSTRAP-OK"
Write-Host "Next: finish Ubuntu's first-run user creation when you like, install the toolchains your projects need inside WSL, then run: wright doctor"
