# Writes the machine's load every ten seconds while a measurement runs:
# the share of processor time in use, and how fast the processor is running
# against its nominal speed (under 100 means it is throttled).
#   powershell -NoProfile -File scripts/sample-load.ps1 -Out data/logs/load.csv
param([Parameter(Mandatory = $true)][string]$Out)
"time,busy_percent,speed_percent" | Set-Content $Out
while ($true) {
    $s = (Get-Counter '\Processor(_Total)\% Processor Time', '\Processor Information(_Total)\% Processor Performance' -SampleInterval 10).CounterSamples
    "{0},{1:n0},{2:n0}" -f (Get-Date -Format s), $s[0].CookedValue, $s[1].CookedValue | Add-Content $Out
}
