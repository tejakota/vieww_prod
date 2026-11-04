# The Windows half of the accessibility suite - what UI Automation can be
# shown to see of a vieww application (FW-10c).
#
#   ci/check/a11y-windows.ps1 -Check      # what this machine can run
#   ci/check/a11y-windows.ps1 -Dump       # the live UIA tree, as JSON, on stdout
#   ci/check/a11y-windows.ps1 -Assert     # the contract, over the live tree
#   ci/check/a11y-windows.ps1 -Golden     # dump vs the committed golden, structure only
#   ci/check/a11y-windows.ps1 -Record     # write the golden from this run (first run)
#
# # What this settles, and on which machine
#
# Narrator (and every Windows screen reader) reads the UI Automation tree.
# This script walks it with .NET's own UIA client (UIAutomationClient, ships
# with Windows PowerShell), dumps it as JSON, and asserts the vieww contract:
# the button exists and is named, the field carries its value, every
# interactive element is named, the tree is not the placeholder.
#
# The **golden** comparison is the second half: the tree recorded on the
# self-hosted leg, compared structurally (roles, names, shape - not bounds,
# which move with the machine's fonts and DPI). No golden is committed yet,
# because no Windows machine has ever run this; `-Record` on the first
# self-hosted run writes it, and every run after that compares against it.
# Until then `-Golden` says exactly that and fails - a golden comparison with
# no golden is not a pass, it is an admission.
#
# Windows PowerShell 5.1 by choice, not pwsh 7: the UIA client assemblies are
# .NET Framework ones, and on pwsh they do not load. The GitHub windows runner
# has 5.1; `shell: powershell` is how the workflow step asks for it.
#
# # What no half of this settles
#
# Ears. Nothing here proves Narrator *spoke*, the same standing the Android
# and iOS legs record for TalkBack and VoiceOver: a green tree is "the reader
# can find and name every control", and a manual session beside it is the
# other half of the contract PENDING 3.6 writes down.

param(
    [switch]$Check,
    [switch]$Dump,
    [switch]$Assert,
    [switch]$Golden,
    [switch]$Record,
    [string]$ProcessName = "desktop"
)

$ErrorActionPreference = "Stop"

# The UIA client, from the .NET Framework - see the header for why 5.1.
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes

$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$golden = Join-Path $here "testdata\a11y-uia-tree.json"

function Report([string]$message) { Write-Host "a11y-windows: $message" }
function Fail([string]$message) {
    Write-Error "a11y-windows: FAIL - $message"
    exit 1
}

# The demo's top-level window, by process name. The demo is
# `cargo run -p vieww-platform-winit --example desktop`, so the process is
# "desktop"; -ProcessName is how a differently-named build is pointed at.
function Find-Root {
    $process = Get-Process -Name $ProcessName -ErrorAction SilentlyContinue | Select-Object -First 1
    if (-not $process) {
        return $null
    }
    # The UIA root, scoped to the process - every element found below it is
    # the application's own, not the desktop's.
    $root = [System.Windows.Automation.AutomationElement]::RootElement
    $condition = New-Object System.Windows.Automation.PropertyCondition(
        [System.Windows.Automation.AutomationElement]::ProcessIdProperty, $process.Id)
    return $root.FindFirst([System.Windows.Automation.TreeScope]::Children, $condition)
}

# One UIA element as the JSON this script's own assertions (and the golden)
# speak in. Every property read is defensive: an element that answers a query
# with an exception must cost that field, not the whole walk.
function Convert-Element($element, [int]$index, [string]$path) {
    $node = [ordered]@{
        id    = "$path/$index"
        role  = ""
        label = $null
        value = $null
    }
    try {
        $node.role = $element.Current.ControlType.ProgrammaticName -replace "^ControlType\.", ""
    } catch { $node.role = "Unknown" }
    try {
        $name = $element.Current.Name
        if ($name) { $node.label = $name }
    } catch { }
    try {
        # The value pattern is what a text field's content arrives through;
        # most elements do not implement it, which is not an error.
        $pattern = $null
        if ($element.TryGetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern, [ref]$pattern)) {
            $value = $pattern.Current.Value
            if ($value) { $node.value = $value }
        }
    } catch { }
    try {
        $node.focusable = [bool]($element.Current.IsKeyboardFocusable)
    } catch { }
    try {
        $node.enabled = [bool]($element.Current.IsEnabled)
    } catch { }
    try {
        $node.bounds = @(
            [math]::Round($element.Current.BoundingRectangle.X, 1),
            [math]::Round($element.Current.BoundingRectangle.Y, 1),
            [math]::Round($element.Current.BoundingRectangle.X + $element.Current.BoundingRectangle.Width, 1),
            [math]::Round($element.Current.BoundingRectangle.Y + $element.Current.BoundingRectangle.Height, 1)
        )
    } catch { }

    $node.children = @()
    return $node
}

# The whole tree under an element, depth-first, as a flat list of nodes whose
# children are named by their ids - the same shape a11y_atspi.py reads, so
# the two harnesses speak one tree language.
function Walk-Tree($element, [string]$path) {
    $nodes = @()
    $index = 0
    foreach ($child in $element.FindAll([System.Windows.Automation.TreeScope]::Children, [System.Windows.Automation.Condition]::TrueCondition)) {
        $childPath = "$path/$index"
        $node = Convert-Element $child $index $path
        $node.id = $childPath
        $nodes += $node
        $nodes += (Walk-Tree $child $childPath)
        $index++
    }
    return $nodes
}

function Dump-Json($root) {
    # The window itself first - Walk-Tree only descends.
    $top = Convert-Element $root 0 ""
    $top.id = ""
    $nodes = @($top) + @(Walk-Tree $root "")
    # Children of every node are the ids immediately under its own path.
    foreach ($node in $nodes) {
        $prefix = "$($node.id)/"
        $node.children = @($nodes | Where-Object { $_.id.StartsWith($prefix) -and ($_.id.Substring($prefix.Length) -notmatch "/") } | ForEach-Object { $_.id })
    }
    $document = [ordered]@{
        source = "recorded from the live UI Automation tree (Windows PowerShell UIAutomationClient)"
        root   = ""
        nodes  = $nodes
    }
    return ($document | ConvertTo-Json -Depth 6)
}

# The vieww contract, over the dumped nodes - the same assertions
# a11y_atspi.py makes, in this harness's own spelling.
function Assert-Contract($nodes) {
    $failures = @()

    if ($nodes.Count -lt 10) {
        $failures += "the tree has $($nodes.Count) node(s) - that is the placeholder shape, not an application tree"
    }
    if ($nodes[0].role -ne "Window") {
        $failures += "the root is a $($nodes[0].role), not a Window"
    }
    foreach ($node in $nodes) {
        foreach ($child in $node.children) {
            if (-not ($nodes | Where-Object { $_.id -eq $child })) {
                $failures += "node $($node.id) lists child $child that is not in the tree"
            }
        }
    }
    $tap = $nodes | Where-Object { $_.label -eq "Tap me" } | Select-Object -First 1
    if (-not $tap) {
        $failures += "no element named 'Tap me' - the button never reached the UIA tree"
    } else {
        if ($tap.role -ne "Button") {
            $failures += "'Tap me' is a $($tap.role), not a Button"
        }
        if (-not $tap.focusable) {
            $failures += "'Tap me' is not keyboard-focusable - a reader cannot stop on it"
        }
    }
    $field = $nodes | Where-Object { $_.value -eq "tap to type" } | Select-Object -First 1
    if (-not $field) {
        $failures += "no element holding 'tap to type' - the field never reached the UIA tree"
    }
    # Every interactive element is named - UIA's ControlTypes that act.
    $interactive = @("Button", "CheckBox", "RadioButton", "MenuItem", "ComboBox", "List", "TabItem", "Hyperlink", "Slider", "Custom")
    foreach ($node in $nodes) {
        if ($interactive -contains $node.role -and -not $node.label -and -not $node.value) {
            $failures += "node $($node.id) is a $($node.role) with no name"
        }
    }
    return $failures
}

if ($Check) {
    Report "checking what this machine can run"
    $session = [Environment]::UserInteractive
    Report ("an interactive session: " + $(if ($session) { "yes - the live legs can run" } else { "no - the tree cannot be walked here" }))
    if (Test-Path $golden) {
        Report "a committed golden exists at ci/check/testdata/a11y-uia-tree.json"
    } else {
        Report "no golden yet - the first self-hosted -Record writes it; -Golden fails until then, honestly"
    }
    Report "the Rust bridge still type-checks anywhere: cargo check --target x86_64-pc-windows-msvc -p vieww-platform-winit"
    exit 0
}

$root = Find-Root
if (-not $root) {
    Fail "no '$ProcessName' process with a top-level window - run the demo first (cargo run -p vieww-platform-winit --example desktop)"
}

if ($Dump) {
    Dump-Json $root
    exit 0
}

if ($Record) {
    $json = Dump-Json $root
    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $golden) | Out-Null
    [IO.File]::WriteAllText($golden, $json)
    Report "recorded the live UIA tree to $golden - commit it; -Golden compares every later run against it"
    exit 0
}

$nodes = (Dump-Json $root | ConvertFrom-Json).nodes
$failures = Assert-Contract $nodes
foreach ($failure in $failures) { Report "FAIL - $failure" }
Report ("the live UIA tree: " + $nodes.Count + " node(s), " + $(if ($failures.Count) { "$($failures.Count) failure(s)" } else { "PASS" }))

if ($Golden) {
    if (-not (Test-Path $golden)) {
        Fail "no golden to compare against - record one first with -Record on the self-hosted leg"
    }
    # Structure only: bounds move with the machine's fonts and DPI; the
    # accessibility contract is roles, names, and shape.
    $strip = {
        param($doc)
        foreach ($node in $doc.nodes) { $node.PSObject.Properties.Remove("bounds") }
        $doc.nodes = $doc.nodes | Sort-Object id
        $doc
    }
    $live = & $strip (Dump-Json $root | ConvertFrom-Json)
    $committed = & $strip (Get-Content $golden -Raw | ConvertFrom-Json)
    $liveJson = $live | ConvertTo-Json -Depth 6
    $committedJson = $committed | ConvertTo-Json -Depth 6
    if ($liveJson -ne $committedJson) {
        Fail "the UIA tree drifted from the committed golden - review, then -Record to re-baseline"
    }
    Report "the live tree matches the committed golden structurally"
}

exit $(if ($failures.Count) { 1 } else { 0 })
