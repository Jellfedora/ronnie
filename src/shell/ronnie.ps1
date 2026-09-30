# Ronnie shell integration for PowerShell (generated, overwritten at each start). Run after the user's
# profile: tells Ronnie the current folder (OSC 7) and marks commands (OSC 133), and gives this terminal
# its own history.

# This terminal's own history, kept across restarts. A new terminal starts from the usual history.
if ($env:RONNIE_HISTFILE -and (Get-Module PSReadLine)) {
    $global:RonnieGlobalHistory = (Get-PSReadLineOption).HistorySavePath
    if (-not (Test-Path -LiteralPath $env:RONNIE_HISTFILE)) {
        New-Item -ItemType Directory -Force -Path (Split-Path -Parent $env:RONNIE_HISTFILE) | Out-Null
        if ($global:RonnieGlobalHistory -and (Test-Path -LiteralPath $global:RonnieGlobalHistory)) {
            Copy-Item -LiteralPath $global:RonnieGlobalHistory -Destination $env:RONNIE_HISTFILE
        } else {
            New-Item -ItemType File -Path $env:RONNIE_HISTFILE | Out-Null
        }
    }
    Set-PSReadLineOption -HistorySavePath $env:RONNIE_HISTFILE
    # Commands typed here also go to the usual history, for other terminals. What the user's handler
    # (or PSReadLine's own, which leaves out commands holding secrets) keeps in memory only stays out.
    $global:RonnieHistoryHandler = (Get-PSReadLineOption).AddToHistoryHandler
    Set-PSReadLineOption -AddToHistoryHandler {
        param([string]$line)
        $keep = if ($global:RonnieHistoryHandler) { & $global:RonnieHistoryHandler $line } else { $true }
        if (($keep -eq $true -or "$keep" -eq 'MemoryAndFile') -and $line.Trim() -and $global:RonnieGlobalHistory) {
            try {
                $entry = ($line -split "`r?`n") -join "``$([Environment]::NewLine)"
                [IO.File]::AppendAllText($global:RonnieGlobalHistory, $entry + [Environment]::NewLine)
            } catch {}
        }
        $keep
    }
}

# Text for OSC sequences: no ";" nor control characters.
function global:RonnieUrl([string]$text) {
    ($text.Substring(0, [Math]::Min($text.Length, 200)) -split '/' | ForEach-Object { [Uri]::EscapeDataString($_) }) -join '/'
}

# Command marks: the command line once Enter is pressed...
$global:RonnieRunning = $false
if (Test-Path Function:\PSConsoleHostReadLine) {
    $global:RonnieReadLine = $function:PSConsoleHostReadLine
    function global:PSConsoleHostReadLine {
        $line = & $global:RonnieReadLine
        if ($line -and $line.Trim()) {
            $global:RonnieRunning = $true
            [Console]::Write("$([char]27)]133;C;cmdline_url=$(RonnieUrl $line)$([char]7)")
        }
        $line
    }
}

# ...its end with its status, the folder, then a new prompt (the user's own, drawn as before).
$global:RonniePrompt = $function:prompt
function global:prompt {
    $ok = $global:?
    $code = if ($ok) { 0 } elseif ($global:LASTEXITCODE) { $global:LASTEXITCODE } else { 1 }
    $e = [char]27
    $bel = [char]7
    $marks = ''
    if ($global:RonnieRunning) {
        $marks += "$e]133;D;$code$bel"
        $global:RonnieRunning = $false
    }
    $here = $executionContext.SessionState.Path.CurrentLocation
    if ($here.Provider.Name -eq 'FileSystem') {
        $marks += "$e]7;file://$env:COMPUTERNAME/$(RonnieUrl ($here.ProviderPath -replace '\\', '/'))$bel"
    }
    $marks += "$e]133;A$bel"
    $marks + (& $global:RonniePrompt)
}
