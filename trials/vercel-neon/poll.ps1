# Runs the two-hour polling test without letting Windows go to sleep.
#
#   pwsh -File poll.ps1 [-Minutes 116]
#
# Reads TRIAL_URL and TRIAL_TOKEN from .env.trials.local at the repo root.
param([int]$Minutes = 116)

# ES_CONTINUOUS | ES_SYSTEM_REQUIRED: asks Windows not to sleep while this
# process runs. It was not enough on the trial machine, which slept after 76
# minutes; check the power settings before a long run.
Add-Type -Namespace Win32 -Name Power -MemberDefinition `
    '[DllImport("kernel32.dll")] public static extern uint SetThreadExecutionState(uint esFlags);'
[Win32.Power]::SetThreadExecutionState([uint32]"0x80000001") | Out-Null

Get-Content (Join-Path $PSScriptRoot '..\..\.env.trials.local') | ForEach-Object {
    if ($_ -match '^([A-Z_]+)=(.*)$') { Set-Item "env:$($Matches[1])" $Matches[2] }
}

Set-Location $PSScriptRoot
& .\target\release\examples\load.exe poll --minutes $Minutes --interval-secs 30 `
    --out results\poll.csv *> results\poll.txt
