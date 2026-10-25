<#
.SYNOPSIS
  Mirrors the design docs into a checkout of the GitHub wiki.

.DESCRIPTION
  docs/ stays the source of truth (CLAUDE.md); the wiki is a read-only mirror of it.
  This copies docs/PLAN.md, docs/STATUS.md, docs/recon/*.md and docs/milestones/*.md into the
  wiki checkout, one page each, turns the docs' references to each other into wiki links, and
  writes the Home, _Sidebar and _Footer pages. Pages it no longer generates are deleted.

  It never commits or pushes. Review the checkout's diff, then commit and push it once the owner
  says so (CLAUDE.md: ask before pushing anything).

.PARAMETER WikiDir
  The wiki checkout. Defaults to run/wiki, which is gitignored. Cloned from GitHub if missing.

.EXAMPLE
  pwsh scripts/sync-wiki.ps1
#>
[CmdletBinding()]
param([string]$WikiDir)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$repo = Split-Path -Parent $PSScriptRoot
$site = 'https://github.com/Doodle-Studios/limonite-client'
$wikiUrl = "$site.wiki.git"
# Kept ASCII-only so Windows PowerShell 5.1 reads this file the same as PowerShell 7.
$dot = [char]0x00B7
$utf8 = [System.Text.UTF8Encoding]::new($false)

# Resolve the folder now, against where we were started: .NET's idea of the current directory
# doesn't follow Set-Location.
if (-not $WikiDir) { $WikiDir = Join-Path $repo 'run/wiki' }
elseif (-not [System.IO.Path]::IsPathRooted($WikiDir)) { $WikiDir = Join-Path (Get-Location).Path $WikiDir }

# Source file, relative to the repo -> wiki page name. Recon notes and milestone plans are
# found by name, so a new milestone's plan gets its page without editing this script.
$pages = [ordered]@{
    'docs/PLAN.md'   = 'Design-Plan'
    'docs/STATUS.md' = 'Status'
}
foreach ($file in Get-ChildItem (Join-Path $repo 'docs/recon') -Filter '*.md' | Sort-Object Name) {
    if ($file.Name -notmatch '^m(\w+?)-') { throw "docs/recon/$($file.Name): expected a name like m<N>-<topic>.md" }
    $pages["docs/recon/$($file.Name)"] = "Milestone-$($Matches[1])-Recon"
}
$plans = Get-ChildItem (Join-Path $repo 'docs/milestones') -Filter 'm*-plan.md' |
    Sort-Object { [int]($_.Name -replace '^m(\d+).*$', '$1') }, Name
foreach ($file in $plans) {
    if ($file.Name -notmatch '^m(\w+)-plan\.md$') { throw "docs/milestones/$($file.Name): expected a name like m<N>-plan.md" }
    $pages["docs/milestones/$($file.Name)"] = "Milestone-$($Matches[1])-Plan"
}
$taken = @{}
foreach ($source in $pages.Keys) {
    $page = $pages[$source]
    if ($taken.ContainsKey($page)) { throw "$source and $($taken[$page]) would both be the wiki page $page" }
    $taken[$page] = $source
}

# The wiki should match a commit, so the docs it mirrors must have no uncommitted changes.
$dirty = git -C $repo status --porcelain -- @($pages.Keys)
if ($LASTEXITCODE) { throw 'git status failed' }
if ($dirty) { throw "Commit these first, so the wiki matches a commit:`n$($dirty -join "`n")" }
$commit = (git -C $repo rev-parse --short HEAD).Trim()

if (-not (Test-Path $WikiDir)) {
    Write-Host "Cloning the wiki into $WikiDir"
    git clone $wikiUrl $WikiDir
    if ($LASTEXITCODE) {
        throw "Couldn't clone $wikiUrl. If the wiki has no pages yet, create its first page on GitHub (the Wiki tab, then 'Create the first page'), then run this again."
    }
}

function Read-Lines([string]$path) {
    return [System.IO.File]::ReadAllText($path, $utf8).Replace("`r`n", "`n").TrimEnd("`n").Split("`n")
}

function Write-Page([string]$name, [string[]]$lines) {
    $text = ($lines -join "`n").TrimEnd("`n") + "`n"
    [System.IO.File]::WriteAllText((Join-Path $WikiDir "$name.md"), $text, $utf8)
}

# A doc's H1, and the short form the sidebar uses: "Milestone 4 plan: First pixels" -> "M4 . First pixels".
function Get-Title([string]$source) {
    return ((Read-Lines (Join-Path $repo $source))[0] -replace '^#\s+', '').Trim()
}
function Get-ShortTitle([string]$title) {
    if ($title -match '^Milestone (\S+) plan: (.+)$') { return "M$($Matches[1]) $dot $($Matches[2])" }
    if ($title -match '^Milestone (\S+) recon: (.+)$') { return "M$($Matches[1]) $dot Recon" }
    return $title
}

# Every way the docs name each other in backticks: the repo path, the path under docs/, and the
# bare file name when only one doc has it.
$aliases = @{}
$names = @{}
foreach ($source in $pages.Keys) {
    $aliases[$source] = $pages[$source]
    $aliases[$source.Substring('docs/'.Length)] = $pages[$source]
    $leaf = Split-Path $source -Leaf
    if ($names.ContainsKey($leaf)) { $names[$leaf] = $null } else { $names[$leaf] = $pages[$source] }
}
foreach ($leaf in $names.Keys) { if ($names[$leaf]) { $aliases[$leaf] = $names[$leaf] } }

# `docs/STATUS.md` becomes [`docs/STATUS.md`](Status), outside fenced code blocks only.
$linkify = {
    param($match)
    $path = $match.Groups[1].Value
    if ($aliases.ContainsKey($path)) { return "[``$path``]($($aliases[$path]))" }
    return $match.Value
}
function Convert-Links([string[]]$lines) {
    $inFence = $false
    foreach ($line in $lines) {
        if ($line -match '^\s*(```|~~~)') { $inFence = -not $inFence; $line; continue }
        if ($inFence) { $line; continue }
        [regex]::Replace($line, '(?<!\[)`([^`\s]+\.md)`', $linkify)
    }
}

$written = @{}
foreach ($source in $pages.Keys) {
    $page = $pages[$source]
    $lines = Read-Lines (Join-Path $repo $source)
    # The wiki shows the page name as a title, so an H1 that only repeats it ("Status") goes.
    if ($lines.Count -gt 1 -and ($lines[0] -replace '^#\s+', '').Trim() -eq ($page -replace '-', ' ')) {
        $lines = $lines[1..($lines.Count - 1)]
        if ($lines.Count -gt 1 -and $lines[0] -eq '') { $lines = $lines[1..($lines.Count - 1)] }
    }
    $header = "<sub>Mirrored from [``$source``]($site/blob/main/$source) at commit ``$commit``.</sub>"
    Write-Page $page (@($header, '') + @(Convert-Links $lines))
    $written[$page] = $true
}

$milestoneRows = foreach ($source in $pages.Keys) {
    if ($source -notmatch '^docs/(recon|milestones)/') { continue }
    $page = $pages[$source]
    "| $(Get-ShortTitle (Get-Title $source)) | [$($page -replace '-', ' ')]($page) |"
}

Write-Page 'Home' @(
    "<p align=`"center`"><img src=`"https://raw.githubusercontent.com/Doodle-Studios/limonite-client/main/docs/readme/title.png`" alt=`"Limonite Client`" width=`"640`"></p>"
    ''
    "<p align=`"center`"><b>The design documents of <a href=`"$site`">Limonite Client</a>, a Minecraft: Bedrock Edition client written in Rust.</b></p>"
    ''
    "This wiki mirrors the project's [``docs/``]($site/tree/main/docs) folder: the design plan, the status page, the recon note and every milestone's plan, with its results and deviations. The repository is the source of truth, and these pages are regenerated from it."
    ''
    "**Developed AI-first.** These documents are written by Claude Opus 5.5, and every milestone's plan is approved by [@LaDoodleInTheHat](https://github.com/LaDoodleInTheHat) before any code is written."
    ''
    '## Start here'
    ''
    '| Page | What''s in it |'
    '| --- | --- |'
    '| [Design Plan](Design-Plan) | The design, and the source of truth for decisions: architecture, the fork''s seams, the renderer, packs, UI, milestones and risks |'
    '| [Status](Status) | Where the project stands, the dev setup, build times and gotchas |'
    ''
    '## Milestones'
    ''
    '| Milestone | Page |'
    '| --- | --- |'
    $milestoneRows
)
$written['Home'] = $true

$sidebarMilestones = foreach ($source in $pages.Keys) {
    if ($source -notmatch '^docs/(recon|milestones)/') { continue }
    "- [$(Get-ShortTitle (Get-Title $source))]($($pages[$source]))"
}
Write-Page '_Sidebar' (@(
    '**[Home](Home)**'
    ''
    '**Design**'
    ''
    '- [Design Plan](Design-Plan)'
    '- [Status](Status)'
    ''
    '**Milestones**'
    ''
) + @($sidebarMilestones) + @(
    ''
    '**Repository**'
    ''
    "- [README]($site#readme)"
    "- [CLAUDE.md]($site/blob/main/CLAUDE.md)"
))
$written['_Sidebar'] = $true

Write-Page '_Footer' @(
    "<sub>This wiki mirrors [``docs/``]($site/tree/main/docs) in [limonite-client]($site), synced at commit ``$commit`` by ``scripts/sync-wiki.ps1``. Edit the docs there: changes made here are overwritten by the next sync.</sub>"
)
$written['_Footer'] = $true

foreach ($file in Get-ChildItem $WikiDir -Filter '*.md' -File) {
    $name = [System.IO.Path]::GetFileNameWithoutExtension($file.Name)
    if (-not $written.ContainsKey($name)) {
        Remove-Item -LiteralPath $file.FullName
        Write-Host "Removed $($file.Name): no doc generates it any more"
    }
}

Write-Host "Wrote $($written.Count) pages to $WikiDir from commit $commit."
if (Test-Path (Join-Path $WikiDir '.git')) {
    git -C $WikiDir status --short
    Write-Host ''
    Write-Host 'Review the diff, then commit and push once the owner says so:'
    Write-Host "  git -C `"$WikiDir`" add -A"
    Write-Host "  git -C `"$WikiDir`" commit -m `"Sync with limonite-client $commit`""
    Write-Host "  git -C `"$WikiDir`" push"
}
