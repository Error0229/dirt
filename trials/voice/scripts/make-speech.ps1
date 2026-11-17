# Speaks the synthetic half of the test set with Windows' built-in
# synthesiser (System.Speech), writing 16 kHz mono WAV files and, for each,
# the moment every word starts, as the synthesiser reports it.
#
#   powershell -NoProfile -File scripts/make-speech.ps1 -Out data/set
#
# Windows PowerShell 5 is needed: System.Speech is not part of PowerShell 7.
# Nothing leaves the computer, and no microphone is used.
param([Parameter(Mandatory = $true)][string]$Out)

Add-Type -AssemblyName System.Speech
$null = New-Item -ItemType Directory -Force -Path $Out
$Out = (Resolve-Path $Out).Path

# Short thoughts of the kind Dirt is for. Written as they would be typed;
# scripts/analyse.py spells out the numbers before it counts mistakes.
$thoughts = @(
    "Ask Jordan about the Q3 offsite.",
    "Buy oat milk and stamps.",
    "Remember to renew the car insurance before the end of the month.",
    "Idea for the blog: write about why small tools beat big platforms.",
    "Call the dentist tomorrow morning and move the appointment to Friday at 3 pm.",
    "The login page should remember the last email address someone typed.",
    "Book the train to Kyoto for the second week of April, window seat if possible.",
    "Send Priya the photos from the hike, and ask her for the recipe she mentioned.",
    "Maybe the sync indicator should stay quiet unless something actually went wrong.",
    "Pick up the parcel from the post office, it closes at half past five.",
    "Look into why the garage door opens by itself when it rains.",
    "Gift idea for Dad: a decent pair of noise cancelling headphones, under 200 dollars.",
    # The second measurement added these, for a set large enough to rank models.
    "Tell Maya the demo moved to Thursday at half past two.",
    "Password reset emails are landing in spam, check the DKIM record.",
    "Try the ramen place on Fifth Street before it closes for renovation.",
    "Note to self: the meeting notes belong in the shared folder, not in my inbox.",
    "Refund for the headphones should arrive within 5 to 7 business days.",
    "Could the archive view show a count of how many notes are hidden?",
    "Water the plants on Tuesday and Friday while Sam is away.",
    "Read chapter 4 of the database book before the study group on the 12th.",
    "Ask the landlord whether the heating can be fixed before December.",
    "The quarterly report needs two more charts and a shorter summary.",
    "Bring a charger, the passport, and the printed tickets to the airport.",
    "What if search matched tags even when the hash sign is left out?",
    "Dinner with Aunt Rosa on Saturday, bring the lemon cake.",
    "Cancel the gym membership before the 1st, it renews automatically.",
    "The bike needs new brake pads and the rear tire is losing air.",
    "Remind Chen that the invoice for October is still unpaid.",
    "Back up the laptop tonight, the disk has been making a clicking noise.",
    "Podcast idea: interview people who quit their phones for a month.",
    "Move the standup to 9:30 so the team in Lisbon can join.",
    "Buy batteries, dish soap, coffee filters, and a birthday card for Leo.",
    "The error only happens when two devices sync at the same moment.",
    "Schedule the flu shot for next week, mornings are better.",
    "Thank Nadia for covering my shift on Sunday.",
    "It would be nice if the widget remembered whether I was typing or talking."
)
$voices = @("Microsoft David Desktop", "Microsoft Zira Desktop")

$format = New-Object System.Speech.AudioFormat.SpeechAudioFormatInfo(
    16000,
    [System.Speech.AudioFormat.AudioBitsPerSample]::Sixteen,
    [System.Speech.AudioFormat.AudioChannel]::Mono)

$manifest = @()
for ($i = 0; $i -lt $thoughts.Count; $i++) {
    $voice = $voices[$i % $voices.Count]
    $id = "synth-{0:d2}" -f ($i + 1)
    $synth = New-Object System.Speech.Synthesis.SpeechSynthesizer
    $synth.SelectVoice($voice)
    $synth.SetOutputToWaveFile((Join-Path $Out "$id.wav"), $format)
    $words = New-Object System.Collections.ArrayList
    $handler = Register-ObjectEvent -InputObject $synth -EventName SpeakProgress -Action {
        $null = $Event.MessageData.Add([pscustomobject]@{
            word  = $EventArgs.Text
            start = [math]::Round($EventArgs.AudioPosition.TotalSeconds, 3)
        })
    } -MessageData $words
    $synth.Speak($thoughts[$i])
    Start-Sleep -Milliseconds 300
    Unregister-Event -SourceIdentifier $handler.Name
    $synth.Dispose()
    [pscustomobject]@{ id = $id; voice = $voice; text = $thoughts[$i]; words = $words } |
        ConvertTo-Json -Depth 4 | Set-Content -Encoding UTF8 (Join-Path $Out "$id.synth.json")
    Write-Output "$id  $voice  $($words.Count) words"
}
