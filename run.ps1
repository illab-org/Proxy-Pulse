# ─────────────────────────────────────────────
#  Proxy Pulse — Run / Stop Script (PowerShell)
#  Auto-downloads the latest release binary
#  Self-updates when a newer version is available
# ─────────────────────────────────────────────
$ErrorActionPreference = "Stop"

$REPO = "OpenInfra-Labs/Proxy-Pulse"
$SCRIPT_DIR = Split-Path -Parent $MyInvocation.MyCommand.Path
$SCRIPT_PATH = $MyInvocation.MyCommand.Path
$BINARY = Join-Path $SCRIPT_DIR "proxy-pulse.exe"
$PID_FILE = Join-Path $SCRIPT_DIR ".proxy-pulse.pid"
$CONFIG_FILE = Join-Path $SCRIPT_DIR "config.yaml"
$CONFIG_EXAMPLE = Join-Path $SCRIPT_DIR "config.example.yaml"
$VERSION_FILE = Join-Path $SCRIPT_DIR ".proxy-pulse.version"
$LOG_FILE = Join-Path $SCRIPT_DIR "proxy-pulse.log"

# ─── Detect architecture ───
function Get-ArtifactName {
    $arch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture
    switch ($arch) {
        "X64"   { return "proxy-pulse-windows-amd64" }
        "Arm64" { return "proxy-pulse-windows-arm64" }
        default { Write-Error "Unsupported architecture: $arch"; exit 1 }
    }
}

# ─── Self-update ───
function Self-Update {
    Write-Host "Checking for run script updates..."
    $remoteUrl = "https://raw.githubusercontent.com/$REPO/main/run.ps1"
    $tmpFile = [System.IO.Path]::GetTempFileName()

    try {
        Invoke-WebRequest -Uri $remoteUrl -OutFile $tmpFile -UseBasicParsing
        $localHash = (Get-FileHash $SCRIPT_PATH -Algorithm SHA256).Hash
        $remoteHash = (Get-FileHash $tmpFile -Algorithm SHA256).Hash

        if ($localHash -ne $remoteHash) {
            Write-Host "Run script has been updated. Applying update..."
            Copy-Item $tmpFile $SCRIPT_PATH -Force
            Remove-Item $tmpFile -Force
            Write-Host "Restarting with updated script..."
            & powershell -ExecutionPolicy Bypass -File $SCRIPT_PATH @args
            exit 0
        }
        Write-Host "Run script is up to date."
    } catch {
        Write-Warning "Could not check for script updates."
    } finally {
        Remove-Item $tmpFile -Force -ErrorAction SilentlyContinue
    }
}

# ─── Get latest version ───
function Get-LatestVersion {
    $response = Invoke-RestMethod -Uri "https://api.github.com/repos/$REPO/releases/latest" -UseBasicParsing
    return $response.tag_name
}

# ─── Download binary ───
function Download-Binary {
    param([string]$Version)
    $artifact = Get-ArtifactName
    $zipFile = "$artifact.zip"
    $downloadUrl = "https://github.com/$REPO/releases/download/$Version/$zipFile"
    $zipPath = Join-Path $SCRIPT_DIR $zipFile

    Write-Host "Downloading $zipFile ($Version)..."
    Invoke-WebRequest -Uri $downloadUrl -OutFile $zipPath -UseBasicParsing

    Write-Host "Extracting..."
    Expand-Archive -Path $zipPath -DestinationPath $SCRIPT_DIR -Force
    Remove-Item $zipPath -Force

    $Version | Out-File -FilePath $VERSION_FILE -Encoding ascii -NoNewline
    Write-Host "Binary installed: $BINARY ($Version)"
}

# ─── Download config.example.yaml ───
function Download-ConfigExample {
    Write-Host "Downloading config.example.yaml..."
    Invoke-WebRequest -Uri "https://raw.githubusercontent.com/$REPO/main/config.example.yaml" -OutFile $CONFIG_EXAMPLE -UseBasicParsing
    Write-Host "Config template downloaded: $CONFIG_EXAMPLE"
}

# ─── Check for binary update ───
function Check-BinaryUpdate {
    $latestVersion = Get-LatestVersion
    if (-not $latestVersion) {
        Write-Warning "Could not fetch latest version info."
        return
    }

    if (-not (Test-Path $BINARY) -or -not (Test-Path $VERSION_FILE)) {
        Download-Binary -Version $latestVersion
        return
    }

    $currentVersion = Get-Content $VERSION_FILE -Raw
    $currentVersion = $currentVersion.Trim()
    if ($currentVersion -ne $latestVersion) {
        Write-Host "New version available: $currentVersion -> $latestVersion"
        Download-Binary -Version $latestVersion
    } else {
        Write-Host "Binary is up to date ($currentVersion)."
    }
}

# ─── Ensure config ───
function Ensure-Config {
    if (-not (Test-Path $CONFIG_EXAMPLE)) {
        Download-ConfigExample
    }
    if (-not (Test-Path $CONFIG_FILE)) {
        Copy-Item $CONFIG_EXAMPLE $CONFIG_FILE
        Write-Host "Config: created config.yaml from config.example.yaml (default settings)."
    }
}

# ─── Read server port from config ───
function Get-ServerPort {
    if (Test-Path $CONFIG_FILE) {
        $match = Select-String -Path $CONFIG_FILE -Pattern '^\s+port:\s*(\d+)' | Select-Object -First 1
        if ($match) {
            return $match.Matches.Groups[1].Value
        }
    }
    return "8080"
}

# ─── Open browser ───
function Open-Browser {
    param([string]$Url)
    try {
        Start-Process $Url
        Write-Host "Browser opened: $Url"
    } catch {
        # Headless — skip
    }
}

# ─── Stop ───
function Stop-Service_ {
    if (-not (Test-Path $PID_FILE)) {
        Write-Host "Proxy Pulse is not running (no PID file found)."
        return
    }

    $pid = [int](Get-Content $PID_FILE -Raw).Trim()
    $proc = Get-Process -Id $pid -ErrorAction SilentlyContinue

    if ($proc) {
        Write-Host "Stopping Proxy Pulse (PID: $pid)..."
        Stop-Process -Id $pid -Force
        Remove-Item $PID_FILE -Force
        Write-Host "Proxy Pulse stopped."
    } else {
        Write-Host "Proxy Pulse is not running (stale PID: $pid)."
        Remove-Item $PID_FILE -Force
    }
}

# ─── Start ───
function Start-Service_ {
    if (Test-Path $PID_FILE) {
        $pid = [int](Get-Content $PID_FILE -Raw).Trim()
        $proc = Get-Process -Id $pid -ErrorAction SilentlyContinue
        if ($proc) {
            Write-Host "Proxy Pulse is already running (PID: $pid)."
            exit 1
        }
        Remove-Item $PID_FILE -Force
    }

    Self-Update
    Check-BinaryUpdate
    Ensure-Config

    Write-Host "Starting Proxy Pulse..."
    $proc = Start-Process -FilePath $BINARY -WorkingDirectory $SCRIPT_DIR `
        -RedirectStandardOutput $LOG_FILE -RedirectStandardError $LOG_FILE `
        -WindowStyle Hidden -PassThru
    $proc.Id | Out-File -FilePath $PID_FILE -Encoding ascii -NoNewline
    Write-Host "Proxy Pulse started in background (PID: $($proc.Id))."
    Write-Host "Log: $LOG_FILE"

    # Auto-open browser
    $port = Get-ServerPort
    Start-Sleep -Seconds 1
    Open-Browser "http://localhost:$port"
}

# ─── Main ───
$command = if ($args.Count -gt 0) { $args[0] } else { "" }

switch ($command) {
    "stop" {
        Stop-Service_
    }
    "status" {
        if ((Test-Path $PID_FILE) -and (Get-Process -Id ([int](Get-Content $PID_FILE -Raw).Trim()) -ErrorAction SilentlyContinue)) {
            Write-Host "Proxy Pulse is running (PID: $((Get-Content $PID_FILE -Raw).Trim()))."
        } else {
            Write-Host "Proxy Pulse is not running."
        }
    }
    "update" {
        Self-Update
        Check-BinaryUpdate
        Write-Host "Update complete."
    }
    default {
        Start-Service_
    }
}
