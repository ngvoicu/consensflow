#!/bin/sh
# ConsensFlow's installer for macOS:
#
#     curl -fsSL https://consensflow.ngvoicu.dev/install.sh | sh
#     curl -fsSL https://consensflow.ngvoicu.dev/install.sh | sh -s -- --alpha
#
# It reads the release from the update feed the app reads (feed-stable, or
# feed-alpha with --alpha), or takes the one --version names, and installs
# nothing until every check below has passed:
#
# - the release's SHA256SUMS carries a minisign signature by ConsensFlow's
#   update key (the key below, the one the app's updater holds), whose trusted
#   comment names that release: ConsensFlow_<version>_SHA256SUMS;
# - the DMG it downloads has the SHA-256 that SHA256SUMS lists for it;
# - the app in the DMG is the version asked for, its code signature holds
#   (codesign --verify --deep --strict) and is ConsensFlow's own (identifier
#   dev.ngvoicu.consensflow, Developer ID team 3K53U2H749), and Gatekeeper
#   accepts it as notarized (spctl --assess); and all of that again of the copy
#   that goes in, made beside the installed app before anything is moved.
#
# A download through curl skips Gatekeeper's own look, hence these. No tool
# is installed for them: the signature is checked by macOS's own JavaScript
# (osascript -l JavaScript). Releases before 3.0.7-alpha.1 have no signed
# SHA256SUMS and are refused; their pages on GitHub have their downloads.
#
# The app goes to /Applications, or to ~/Applications where /Applications is
# not yours to write to (never through sudo), and an installed ConsensFlow is
# upgraded where it is. An installed ConsensFlow newer than the release is
# never replaced by it, unless --version names that release. An open
# ConsensFlow is asked to quit, as Quit in its menu would, and the install waits
# for it; one that stays open is never stopped, and nothing is installed. The
# installed app is deleted only once the new one is in place and says its
# version. `cf` comes inside the app: nothing else is set up. ConsensFlow is
# made for Apple silicon Macs and Windows only.
#
# A run that is killed (SIGKILL, a power cut: no trap runs) leaves in the folder
# the copy it was making, `.ConsensFlow.app.installing.<pid>`, or the app it had
# moved aside, `.ConsensFlow.app.previous.<pid>`; and in TMPDIR its own folder,
# `consensflow-install.<pid>.<six letters and digits>`, with the DMG still
# attached. The next run, before anything else, removes what is unmistakably an
# earlier run's: by exactly such a name, of a pid that no install is running as
# (a process of that pid is an install only if it is a shell, or its command line
# names install.sh), never a link and never what is on another volume. Where
# nothing is installed any more, the app moved aside is put back if it is a whole,
# checked ConsensFlow, and left, with the reason, if it is not; where the installed
# app is not whole and checked, the one moved aside is left too. A DMG of a dead
# run is detached, never forced. Every one it removes, puts back or leaves is said.
#
# Everything is inside main, called on the last line, so a download cut short
# runs nothing. It writes only in a folder of its own under TMPDIR, removed at
# the end, and in the folder the app goes to.

usage() {
    cat <<'HELP'
Install ConsensFlow on this Mac.

    curl -fsSL https://consensflow.ngvoicu.dev/install.sh | sh
    curl -fsSL https://consensflow.ngvoicu.dev/install.sh | sh -s -- [options]

Options:
  --alpha            the latest alpha (the alpha update channel)
  --stable           the latest release (the default)
  --version VERSION  that release, for example 3.0.7 (v3.0.7 works too)
  -h, --help         this help

Environment (each the same as its option, where it has one):
  CONSENSFLOW_CHANNEL=stable|alpha
  CONSENSFLOW_VERSION=VERSION
  CONSENSFLOW_INSTALL_DIR=DIR    the folder ConsensFlow.app goes into, instead
                                 of /Applications or ~/Applications
  CONSENSFLOW_RELEASES_URL=URL   where the releases are, by default
                                 https://github.com/ngvoicu/consensflow/releases

Uninstall: quit ConsensFlow and move ConsensFlow.app from /Applications (or
~/Applications) to the Trash. Your projects, boards and settings stay in
~/.consensflow; delete that folder too to remove everything.
HELP
}

say() {
    printf 'consensflow: %s\n' "$*"
}

refuse() {
    printf 'consensflow: %s\n' "$*" >&2
    printf 'consensflow: nothing was installed.\n' >&2
    exit 1
}

# Downloads $1 into $2.
fetch() {
    if [ "$releases" = "$official" ]; then
        curl --proto '=https' --proto-redir '=https' --tlsv1.2 --retry 3 -fsL "$1" -o "$2"
    else
        curl --retry 3 -fsL "$1" -o "$2"
    fi
}

# The processes running from the app at $1, one pid a line.
processes_of() {
    ps -axo pid=,comm= | awk -v inside="$1/Contents/" 'index($0, inside) { print $1 }'
}

# version order: begin
# Whether the version $1 comes after $2, by SemVer's precedence: the numbers
# first, then a release after any of its pre-releases, then the pre-release's
# fields one by one (numbers by value, below words; words by their bytes), and
# a build's +metadata counts for nothing.
newer_than() {
    LC_ALL=C awk -v a="$1" -v b="$2" '
        function numeric(x) { return x ~ /^[0-9]+$/ }
        function field(x, y) {
            if (numeric(x) && numeric(y)) return (x + 0 > y + 0) - (x + 0 < y + 0)
            if (numeric(x)) return -1
            if (numeric(y)) return 1
            return (x > y) - (x < y)
        }
        function order(x, y,    xm, ym, xp, yp, i, n, c, xs, ys) {
            sub(/\+.*/, "", x); sub(/\+.*/, "", y)
            xp = index(x, "-") ? substr(x, index(x, "-") + 1) : ""
            yp = index(y, "-") ? substr(y, index(y, "-") + 1) : ""
            if (index(x, "-")) x = substr(x, 1, index(x, "-") - 1)
            if (index(y, "-")) y = substr(y, 1, index(y, "-") - 1)
            split(x, xm, "."); split(y, ym, ".")
            for (i = 1; i <= 3; i++) { c = field(xm[i] + 0, ym[i] + 0); if (c) return c }
            if (xp == "" && yp == "") return 0
            if (xp == "") return 1
            if (yp == "") return -1
            xs = split(xp, xm, "."); ys = split(yp, ym, ".")
            n = xs < ys ? xs : ys
            for (i = 1; i <= n; i++) { c = field(xm[i], ym[i]); if (c) return c }
            return (xs > ys) - (xs < ys)
        }
        BEGIN { exit !(order(a, b) > 0) }'
}
# version order: end

# The pids of this shell and the processes it runs under.
ancestry() {
    pid=$$
    while [ -n "$pid" ] && [ "$pid" -gt 1 ]; do
        echo "$pid"
        pid=$(ps -o ppid= -p "$pid" | tr -d ' ')
    done
}

# Asks the ConsensFlow that runs from $1 to quit, the way Quit in its menu
# does, and waits for every process of it to end. It is never stopped: one that
# stays open ends the install.
quit_open_app() {
    running=$(processes_of "$1")
    [ -n "$running" ] || return 0
    for pid in $(ancestry); do
        if printf '%s\n' "$running" | grep -qx "$pid"; then
            refuse "this shell runs inside the ConsensFlow at $1, and quitting it to upgrade would end the install too. Run this from Terminal or another terminal app."
        fi
    done
    say "ConsensFlow is open: asking it to quit, as Quit in its menu would..."
    for pid in $running; do
        osascript -l JavaScript -e "ObjC.import('AppKit'); const app = \$.NSRunningApplication.runningApplicationWithProcessIdentifier($pid); if (!app.isNil()) app.terminate;" >/dev/null 2>&1 || true
    done
    waited=0
    while [ -n "$(processes_of "$1")" ]; do
        if [ "$waited" -ge "$quit_wait" ]; then
            refuse "ConsensFlow at $1 is still open after ${quit_wait}s. Quit it (ConsensFlow > Quit ConsensFlow) and run this again."
        fi
        sleep 1
        waited=$((waited + 1))
    done
}

# Whether $1 is a pid as a process here has one: one to five digits, no leading
# zero (a Mac's pids stay below 100000), read byte by byte as the C locale reads
# them, whatever the user's: a digit of another script is none.
is_pid() {
    (
        LC_ALL=C
        case "$1" in
            "" | 0* | *[!0-9]*) exit 1 ;;
        esac
        [ "${#1}" -le 5 ]
    )
}

# Whether $1 is the six letters and digits `mktemp` ends a name with.
is_suffix() {
    (
        LC_ALL=C
        case "$1" in
            "" | *[!A-Za-z0-9]*) exit 1 ;;
        esac
        [ "${#1}" -eq 6 ]
    )
}

# Whether the process $1 may be an install that goes on: a shell running a file
# named install.sh, a program whose command line names one, or a shell that was
# given the script on its input (`curl | sh` runs it as `sh -s`, which names
# nothing), so any shell, a Terminal tab's among them, whose pid a dead run had.
# Never this shell: it has staged nothing yet, so what has its pid is an earlier
# run's, whose pid this one was given again.
install_goes_on() {
    [ "$1" != "$$" ] || return 1
    program=$(ps -o comm= -p "$1" 2>/dev/null || true)
    [ -n "$program" ] || return 1
    commandline=$(ps -o command= -p "$1" 2>/dev/null || true)
    case "$commandline" in
        *install.sh*) return 0 ;;
    esac
    program=${program##*/}
    case "${program#-}" in
        sh | bash | dash | zsh | ksh | ash | csh | tcsh | fish) return 0 ;;
    esac
    return 1
}

# Whether $1 is a folder (never a link) on the volume of the folder $2 it is in:
# a volume mounted at it makes it another one's.
plain_folder() {
    [ ! -L "$1" ] && [ -d "$1" ] && [ "$(stat -f %d "$1")" = "$(stat -f %d "$2")" ]
}

# Why the app at $1 ($2 in the words) is not a whole, checked ConsensFlow of the
# version it says itself: what check_app refuses with, or nothing where it holds.
unsound() {
    own=
    # PlistBuddy answers "File Doesn't Exist, Will Create" for a file that is not there.
    if [ -f "$1/Contents/Info.plist" ]; then
        own=$("$plistbuddy" -c 'Print :CFBundleShortVersionString' "$1/Contents/Info.plist" 2>/dev/null || true)
    fi
    case "$own" in
        [0-9]*) (check_app "$1" "$2" "$own") 2>&1 | sed -n '1s/^consensflow: //p' ;;
        *) echo "it says no version" ;;
    esac
}

# Takes away what an earlier run left in $dir if it was stopped where no trap
# runs, and says each thing it did. A leftover is only a folder (never a link, never
# a volume mounted at it) named exactly .ConsensFlow.app.installing.<pid> or
# .ConsensFlow.app.previous.<pid>, in $dir itself, whose pid no install runs as:
# its path is made here from $dir and the name, and nothing else is ever removed,
# nothing on another volume with it (rm -x). The app moved aside goes first.
remove_leftovers() {
    for kind in previous installing; do
        for found in "$dir"/.ConsensFlow.app."$kind".*; do
            [ -e "$found" ] || [ -L "$found" ] || continue
            name=${found##*/}
            owner=${name#.ConsensFlow.app."$kind".}
            is_pid "$owner" || continue
            leftover="$dir/.ConsensFlow.app.$kind.$owner"
            if ! plain_folder "$leftover" "$dir"; then
                say "left $leftover alone: an install makes a folder there, on the volume of $dir, and this is not one."
                continue
            fi
            if install_goes_on "$owner"; then
                say "left $leftover alone: process $owner may be an install that is still going on."
                continue
            fi
            if [ "$kind" = installing ]; then
                if rm -rf -x "$leftover"; then
                    say "removed $leftover, the copy of the app an earlier install was making (no install runs as process $owner)."
                else
                    say "could not remove $leftover, the copy of the app an earlier install was making."
                fi
            elif [ ! -e "$dest" ] && [ ! -L "$dest" ]; then
                problem=$(unsound "$leftover" "the app an earlier install moved aside")
                if [ -n "$problem" ]; then
                    say "left $leftover alone: it is not a whole, checked ConsensFlow ($problem), so it is not put back as $dest."
                elif mv "$leftover" "$dest"; then
                    say "put $leftover back as $dest: an earlier install was stopped after it moved that app aside, and nothing was in its place."
                else
                    say "could not put $leftover back as $dest."
                fi
            else
                have=$("$plistbuddy" -c 'Print :CFBundleShortVersionString' "$dest/Contents/Info.plist" 2>/dev/null || true)
                problem=$(unsound "$dest" "the ConsensFlow in $dir")
                if [ -n "$problem" ]; then
                    say "left $leftover alone: the ConsensFlow in $dest is not a whole, checked app ($problem), and this may be the only whole one."
                elif rm -rf -x "$leftover"; then
                    say "removed $leftover, the app an earlier install moved aside: $dest is whole, and checked (version $have)."
                else
                    say "could not remove $leftover, the app an earlier install moved aside."
                fi
            fi
        done
    done
}

# The same for what a killed run left in the user's own temporary folder: its
# folder consensflow-install.<pid>.<six letters and digits>, which holds the DMG it
# attached at volume/. The folder of a dead run, made by this user, is taken as the
# leftovers are, once its DMG is detached (never forced: one that will not detach
# keeps the folder, and is said). This run's own folder is never one.
remove_stale_runs() {
    base=${TMPDIR:-/tmp}
    base=${base%/}
    physical=$(cd -P "$base" 2>/dev/null && pwd -P) || return 0
    for found in "$base"/consensflow-install.*; do
        [ -e "$found" ] || [ -L "$found" ] || continue
        name=${found##*/}
        [ "$name" != "${tmp##*/}" ] || continue
        rest=${name#consensflow-install.}
        owner=${rest%%.*}
        suffix=${rest#*.}
        is_pid "$owner" && is_suffix "$suffix" && [ "$rest" = "$owner.$suffix" ] || continue
        stale="$physical/$name"
        if ! plain_folder "$stale" "$physical"; then
            say "left $stale alone: an install makes a folder there, on the volume of $physical, and this is not one."
            continue
        fi
        if [ "$(stat -f %u "$stale")" != "$(id -u)" ]; then
            say "left $stale alone: it is not yours."
            continue
        fi
        if install_goes_on "$owner"; then
            say "left $stale alone: process $owner may be an install that is still going on."
            continue
        fi
        if mount | grep -qF " on $stale/volume ("; then
            if hdiutil detach -quiet "$stale/volume" >/dev/null 2>&1; then
                say "detached $stale/volume, the DMG an earlier install attached (no install runs as process $owner)."
            else
                say "could not detach $stale/volume, the DMG an earlier install attached: left $stale alone."
                continue
            fi
        fi
        if rm -rf -x "$stale"; then
            say "removed $stale, the folder an earlier install worked in (no install runs as process $owner)."
        else
            say "could not remove $stale, the folder an earlier install worked in."
        fi
    done
}

# Takes away what this run left half done: the mounted DMG, the copy that was
# going in, a new app put in place and not yet confirmed, and the app it was
# replacing, put back where nothing is; then the run's own folder. So a refusal
# at any step leaves the folder as it found it. Once the new app is confirmed,
# what is left of the old one (a Ctrl-C while it was deleted) is deleted, never
# moved: moved into the new app's place it would land inside its bundle.
cleanup() {
    if [ -n "${mounted:-}" ]; then
        hdiutil detach -quiet "$mounted" >/dev/null 2>&1 || hdiutil detach -quiet -force "$mounted" >/dev/null 2>&1 || true
    fi
    if [ -n "${stage:-}" ] && [ -e "$stage" ]; then
        rm -rf -x "$stage"
    fi
    if [ -n "${placed:-}" ] && [ -e "$dest" ]; then
        rm -rf -x "$dest"
    fi
    if [ -n "${previous:-}" ] && [ -e "$previous" ]; then
        if [ ! -e "$dest" ]; then
            mv "$previous" "$dest"
        elif [ -z "${placed:-}" ]; then
            rm -rf -x "$previous"
        fi
    fi
    if [ -n "${tmp:-}" ] && [ -d "$tmp" ]; then
        rm -rf -x "$tmp"
    fi
}

main() {
    set -eu

    # ConsensFlow's update key (minisign, as app/src-tauri/tauri.conf.json
    # holds it inside its own base64), and the Developer ID team it signs with.
    key=RWTirTrEjU6epgN2c8yV2aIfRHzcCHVBi9zddieRVM5SahwhDFJlFd7Q
    team=3K53U2H749
    requirement="anchor apple generic and identifier \"dev.ngvoicu.consensflow\" and certificate leaf[subject.OU] = \"$team\""
    spctl=/usr/sbin/spctl
    plistbuddy=/usr/libexec/PlistBuddy
    quit_wait=120

    official=https://github.com/ngvoicu/consensflow/releases
    releases="${CONSENSFLOW_RELEASES_URL:-$official}"
    releases="${releases%/}"
    channel="${CONSENSFLOW_CHANNEL:-stable}"
    version="${CONSENSFLOW_VERSION:-}"
    named="$version"
    while [ $# -gt 0 ]; do
        case "$1" in
            --alpha) channel=alpha ;;
            --stable) channel=stable ;;
            --version)
                [ $# -ge 2 ] || { echo "consensflow: --version needs a release, for example --version 3.0.7" >&2; exit 2; }
                version="$2"
                named="$2"
                shift
                ;;
            --version=*) version="${1#--version=}"; named="$version" ;;
            -h | --help) usage; exit 0 ;;
            *) echo "consensflow: unknown option '$1' (see --help)" >&2; exit 2 ;;
        esac
        shift
    done
    case "$channel" in
        stable | alpha) ;;
        *) echo "consensflow: the channel is stable or alpha, not '$channel'" >&2; exit 2 ;;
    esac
    if [ -n "$version" ]; then
        version="${version#v}"
        case "$version" in
            [0-9]*) ;;
            *) echo "consensflow: a version is a release number such as 3.0.7, not '$version'" >&2; exit 2 ;;
        esac
        case "$version" in
            *[!0-9A-Za-z.+-]*) echo "consensflow: a version is a release number such as 3.0.7, not '$version'" >&2; exit 2 ;;
        esac
    fi

    system=$(uname -s)
    case "$system" in
        Darwin) ;;
        Linux) refuse "ConsensFlow is made for macOS and Windows only; there is no Linux build." ;;
        *) refuse "ConsensFlow is made for macOS and Windows only, not $system. On Windows, run in PowerShell: irm https://consensflow.ngvoicu.dev/install.ps1 | iex" ;;
    esac
    if [ "$(sysctl -n hw.optional.arm64 2>/dev/null || echo 0)" != 1 ]; then
        refuse "ConsensFlow for Mac is made for Apple silicon (M1 and later); this Mac has an Intel processor."
    fi
    for tool in curl shasum od osascript hdiutil codesign ditto awk stat mount; do
        command -v "$tool" >/dev/null 2>&1 || refuse "this Mac has no $tool, which the install needs."
    done

    tmp=$(mktemp -d "${TMPDIR:-/tmp}/consensflow-install.$$.XXXXXX")
    mounted=""
    stage=""
    placed=""
    previous=""
    dest=""
    trap cleanup EXIT
    trap 'exit 130' INT TERM
    write_verifier "$tmp/verify.js"

    if [ -n "${CONSENSFLOW_INSTALL_DIR:-}" ]; then
        dir="${CONSENSFLOW_INSTALL_DIR%/}"
    elif [ -d /Applications/ConsensFlow.app ]; then
        dir=/Applications
    elif [ -d "$HOME/Applications/ConsensFlow.app" ]; then
        dir="$HOME/Applications"
    elif [ -w /Applications ]; then
        dir=/Applications
    else
        dir="$HOME/Applications"
    fi
    mkdir -p "$dir" 2>/dev/null || refuse "could not make the folder $dir."
    [ -w "$dir" ] || refuse "$dir is not yours to write to. Run this as a user who can, or with CONSENSFLOW_INSTALL_DIR=$HOME/Applications; it never uses sudo itself."
    dest="$dir/ConsensFlow.app"
    remove_leftovers
    remove_stale_runs

    if [ -z "$version" ]; then
        feed="$releases/download/feed-$channel/latest.json"
        fetch "$feed" "$tmp/latest.json" || refuse "could not read the $channel update feed at $feed."
        version=$(sed -n 's/.*"version"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$tmp/latest.json" | head -n 1)
        case "$version" in
            [0-9]*) ;;
            *) refuse "the $channel update feed at $feed names no version." ;;
        esac
        case "$version" in
            *[!0-9A-Za-z.+-]*) refuse "the $channel update feed at $feed names no version." ;;
        esac
    fi
    say "ConsensFlow $version"
    if [ -d "$dest" ] && [ -z "$named" ]; then
        current=$("$plistbuddy" -c 'Print :CFBundleShortVersionString' "$dest/Contents/Info.plist" 2>/dev/null || true)
        if [ -n "$current" ] && newer_than "$current" "$version"; then
            refuse "ConsensFlow $current is installed in $dir, newer than $version, which the $channel channel names. To go back to $version, run this with --version $version."
        fi
    fi
    release="$releases/download/v$version"
    signed="ConsensFlow_${version}_SHA256SUMS"
    dmg="ConsensFlow_${version}_aarch64.dmg"

    fetch "$release/SHA256SUMS" "$tmp/SHA256SUMS" || refuse "could not download $release/SHA256SUMS: is $version a release of ConsensFlow?"
    fetch "$release/SHA256SUMS.sig" "$tmp/SHA256SUMS.sig" ||
        refuse "ConsensFlow $version has no signed SHA256SUMS, so its files cannot be checked here: releases before 3.0.7-alpha.1 have none. Download it from $releases/tag/v$version; releases from 3.0.7-alpha.1 on install from here."
    # The check reads SHA256SUMS once, and answers the hash it lists for the DMG
    # from the very bytes whose signature it checked; it must end well and say so.
    if checked=$(osascript -l JavaScript "$tmp/verify.js" "$key" "$(cat "$tmp/SHA256SUMS.sig")" \
        "$(od -An -tx1 -v "$tmp/SHA256SUMS" | tr -d ' \n')" "$signed" "$dmg" 2>&1 </dev/null); then
        status=0
    else
        status=$?
    fi
    case "$status:$checked" in
        "0:ok "*) expected=${checked#ok } ;;
        "0:refused: lists no"*) refuse "$version's SHA256SUMS lists no $dmg." ;;
        0:*) refuse "the signature of $version's SHA256SUMS does not hold: ${checked#refused: }" ;;
        *) refuse "the check of $version's SHA256SUMS ended with status $status: $checked" ;;
    esac
    case "$expected" in
        "" | *[!0-9a-f]*) refuse "$version's SHA256SUMS lists no $dmg." ;;
    esac
    [ "${#expected}" -eq 64 ] || refuse "$version's SHA256SUMS lists no $dmg."
    say "SHA256SUMS is signed by ConsensFlow's update key, for $version"
    say "downloading $dmg"
    fetch "$release/$dmg" "$tmp/$dmg" || refuse "could not download $release/$dmg."
    got=$(shasum -a 256 "$tmp/$dmg" | awk '{ print $1 }')
    [ "$got" = "$expected" ] || refuse "$dmg was changed: its SHA-256 is $got, and the signed SHA256SUMS says $expected."
    say "$dmg has the SHA-256 the signed SHA256SUMS lists"

    # The DMG is mounted by its path in this run's own folder once its hash held: a
    # writer running as this user could swap it in between. The checks of the app
    # in it, and of the copy that goes in, bound what could get in that way to
    # another notarized ConsensFlow of this version under ConsensFlow's team;
    # checking again narrows such a race, it does not exclude it.
    mkdir "$tmp/volume"
    hdiutil attach -nobrowse -readonly -noautoopen -quiet -mountpoint "$tmp/volume" "$tmp/$dmg" </dev/null >/dev/null ||
        refuse "could not open $dmg."
    mounted="$tmp/volume"
    [ -d "$mounted/ConsensFlow.app" ] || refuse "$dmg holds no ConsensFlow.app."
    check_app "$mounted/ConsensFlow.app" "the app in $dmg"

    # The copy that goes in is made beside the installed app, and checked again
    # there: what is moved into place is what was checked, whatever happened to
    # the DMG once it was hashed.
    stage="$dir/.ConsensFlow.app.installing.$$"
    ditto "$mounted/ConsensFlow.app" "$stage" || refuse "could not copy the app into $dir."
    check_app "$stage" "the copy of the app made in $dir"
    say "the app is ConsensFlow's, signed (team $team) and notarized"

    quit_open_app "$dest"
    if [ -n "$(processes_of "$dest")" ]; then
        refuse "ConsensFlow at $dest was opened again while the install waited. Quit it and run this again."
    fi
    # The copy is checked whole again now, after the wait for the open app, as near
    # its move as a shell gets; and the app put in place is checked whole once
    # more while the one it replaces can still be put back. Each narrows what a
    # writer in this folder could change in between; none excludes it.
    check_app "$stage" "the copy of the app made in $dir"
    if [ -e "$dest" ]; then
        previous="$dir/.ConsensFlow.app.previous.$$"
        mv "$dest" "$previous" || { previous=""; refuse "could not move the installed ConsensFlow aside in $dir."; }
    fi
    placed=1
    mv "$stage" "$dest" || refuse "could not put the new ConsensFlow in $dir."
    stage=""
    check_app "$dest" "the app put in $dir"
    placed=""
    if [ -n "$previous" ]; then
        rm -rf -x "$previous"
        previous=""
    fi
    say "ConsensFlow $version is installed in $dest. Open it from there or with: open \"$dest\""
}

# Refuses unless the app at $1 ($2 in the words) is the version asked for ($3
# where it is given: the one the app says, for an app that is only to be whole),
# its code signature holds, it is ConsensFlow's under its Developer ID, and
# Gatekeeper takes it as notarized.
check_app() {
    shown=$("$plistbuddy" -c 'Print :CFBundleShortVersionString' "$1/Contents/Info.plist" 2>/dev/null || true)
    want=${3-$version}
    [ "$shown" = "$want" ] || refuse "$2 is version '$shown', not $want."
    codesign --verify --deep --strict "$1" 2>"$tmp/codesign.txt" ||
        refuse "the code signature of $2 does not hold: $(cat "$tmp/codesign.txt")"
    codesign --verify -R="$requirement" "$1" 2>"$tmp/codesign.txt" ||
        refuse "$2 is not signed by ConsensFlow's Developer ID (team $team): $(cat "$tmp/codesign.txt")"
    assessed=$("$spctl" --assess --type execute -vv "$1" 2>&1) ||
        refuse "Gatekeeper refuses $2: $assessed"
    case "$assessed" in
        *"source=Notarized Developer ID"*) ;;
        *) refuse "Gatekeeper does not take $2 as notarized: $assessed" ;;
    esac
}

# The check of the signed SHA256SUMS, written into the run's folder by main.
write_verifier() {
    cat > "$1" <<'VERIFIER'
// The check of a release's signed SHA256SUMS, run by macOS's own JavaScript
// (`osascript -l JavaScript`): minisign's "ED" signature, Ed25519 (RFC 8032)
// over the BLAKE2b-512 (RFC 7693) of the file, and its global signature over
// the signature and the trusted comment. Written out here because no tool a
// Mac ships checks Ed25519. run(argv) takes the key's base64 line, the .sig
// file's text, the file's bytes in hex and the trusted comment's file name it
// must have, and answers "ok", or "refused: " and why.

const P = 2n ** 255n - 19n;
const L = 2n ** 252n + 27742317777372353535851937790883648493n;
const MASK64 = (1n << 64n) - 1n;

function mod(a, m) {
  const r = a % m;
  return r < 0n ? r + m : r;
}

function power(base, exponent, m) {
  let result = 1n;
  let b = mod(base, m);
  let e = exponent;
  while (e > 0n) {
    if (e & 1n) result = (result * b) % m;
    b = (b * b) % m;
    e >>= 1n;
  }
  return result;
}

function inverse(a) {
  return power(a, P - 2n, P);
}

const D = mod(-121665n * inverse(121666n), P);
const SQRT_M1 = power(2n, (P - 1n) / 4n, P);

function bytesToNumber(bytes) {
  let n = 0n;
  for (let i = bytes.length - 1; i >= 0; i--) n = (n << 8n) | BigInt(bytes[i]);
  return n;
}

function numberToBytes(n, length) {
  const out = new Uint8Array(length);
  let v = n;
  for (let i = 0; i < length; i++) {
    out[i] = Number(v & 255n);
    v >>= 8n;
  }
  return out;
}

// Points in extended coordinates (X, Y, Z, T), x = X/Z, y = Y/Z, x*y = T/Z.
function add(p, q) {
  const a = mod((p[1] - p[0]) * (q[1] - q[0]), P);
  const b = mod((p[1] + p[0]) * (q[1] + q[0]), P);
  const c = mod(2n * D * p[3] * q[3], P);
  const d = mod(2n * p[2] * q[2], P);
  const e = b - a;
  const f = d - c;
  const g = d + c;
  const h = b + a;
  return [mod(e * f, P), mod(g * h, P), mod(f * g, P), mod(e * h, P)];
}

function multiply(scalar, point) {
  let result = [0n, 1n, 1n, 0n];
  let p = point;
  let s = scalar;
  while (s > 0n) {
    if (s & 1n) result = add(result, p);
    p = add(p, p);
    s >>= 1n;
  }
  return result;
}

function negate(p) {
  return [mod(-p[0], P), p[1], p[2], mod(-p[3], P)];
}

function encode(p) {
  const zi = inverse(p[2]);
  const x = mod(p[0] * zi, P);
  const y = mod(p[1] * zi, P);
  return numberToBytes(y | ((x & 1n) << 255n), 32);
}

// RFC 8032 5.1.3; a point not on the curve, or not in canonical form, is null.
function decode(bytes) {
  const n = bytesToNumber(bytes);
  const sign = n >> 255n;
  const y = n & ((1n << 255n) - 1n);
  if (y >= P) return null;
  const u = mod(y * y - 1n, P);
  const v = mod(D * y * y + 1n, P);
  const x2 = mod(u * inverse(v), P);
  let x = power(x2, (P + 3n) / 8n, P);
  if (mod(x * x - x2, P) !== 0n) x = mod(x * SQRT_M1, P);
  if (mod(x * x - x2, P) !== 0n) return null;
  if (x === 0n && sign === 1n) return null;
  if ((x & 1n) !== sign) x = P - x;
  return [x, y, 1n, mod(x * y, P)];
}

const BASE = decode(
  numberToBytes(mod(4n * inverse(5n), P), 32),
);

const SHA512_K = [
  "428a2f98d728ae22", "7137449123ef65cd", "b5c0fbcfec4d3b2f", "e9b5dba58189dbbc",
  "3956c25bf348b538", "59f111f1b605d019", "923f82a4af194f9b", "ab1c5ed5da6d8118",
  "d807aa98a3030242", "12835b0145706fbe", "243185be4ee4b28c", "550c7dc3d5ffb4e2",
  "72be5d74f27b896f", "80deb1fe3b1696b1", "9bdc06a725c71235", "c19bf174cf692694",
  "e49b69c19ef14ad2", "efbe4786384f25e3", "0fc19dc68b8cd5b5", "240ca1cc77ac9c65",
  "2de92c6f592b0275", "4a7484aa6ea6e483", "5cb0a9dcbd41fbd4", "76f988da831153b5",
  "983e5152ee66dfab", "a831c66d2db43210", "b00327c898fb213f", "bf597fc7beef0ee4",
  "c6e00bf33da88fc2", "d5a79147930aa725", "06ca6351e003826f", "142929670a0e6e70",
  "27b70a8546d22ffc", "2e1b21385c26c926", "4d2c6dfc5ac42aed", "53380d139d95b3df",
  "650a73548baf63de", "766a0abb3c77b2a8", "81c2c92e47edaee6", "92722c851482353b",
  "a2bfe8a14cf10364", "a81a664bbc423001", "c24b8b70d0f89791", "c76c51a30654be30",
  "d192e819d6ef5218", "d69906245565a910", "f40e35855771202a", "106aa07032bbd1b8",
  "19a4c116b8d2d0c8", "1e376c085141ab53", "2748774cdf8eeb99", "34b0bcb5e19b48a8",
  "391c0cb3c5c95a63", "4ed8aa4ae3418acb", "5b9cca4f7763e373", "682e6ff3d6b2b8a3",
  "748f82ee5defb2fc", "78a5636f43172f60", "84c87814a1f0ab72", "8cc702081a6439ec",
  "90befffa23631e28", "a4506cebde82bde9", "bef9a3f7b2c67915", "c67178f2e372532b",
  "ca273eceea26619c", "d186b8c721c0c207", "eada7dd6cde0eb1e", "f57d4f7fee6ed178",
  "06f067aa72176fba", "0a637dc5a2c898a6", "113f9804bef90dae", "1b710b35131c471b",
  "28db77f523047d84", "32caab7b40c72493", "3c9ebe0a15c9bebc", "431d67c49c100d4c",
  "4cc5d4becb3e42b6", "597f299cfc657e2a", "5fcb6fab3ad6faec", "6c44198c4a475817",
].map((hex) => BigInt(`0x${hex}`));

function rotr(x, n) {
  return ((x >> n) | (x << (64n - n))) & MASK64;
}

function sha512(bytes) {
  const h = [
    0x6a09e667f3bcc908n, 0xbb67ae8584caa73bn, 0x3c6ef372fe94f82bn, 0xa54ff53a5f1d36f1n,
    0x510e527fade682d1n, 0x9b05688c2b3e6c1fn, 0x1f83d9abfb41bd6bn, 0x5be0cd19137e2179n,
  ];
  const padded = new Uint8Array(Math.ceil((bytes.length + 17) / 128) * 128);
  padded.set(bytes);
  padded[bytes.length] = 0x80;
  const bits = BigInt(bytes.length) * 8n;
  for (let i = 0; i < 16; i++) {
    padded[padded.length - 1 - i] = Number((bits >> BigInt(8 * i)) & 255n);
  }
  const w = new Array(80);
  for (let block = 0; block < padded.length; block += 128) {
    for (let t = 0; t < 16; t++) {
      let word = 0n;
      for (let i = 0; i < 8; i++) word = (word << 8n) | BigInt(padded[block + t * 8 + i]);
      w[t] = word;
    }
    for (let t = 16; t < 80; t++) {
      const s0 = rotr(w[t - 15], 1n) ^ rotr(w[t - 15], 8n) ^ (w[t - 15] >> 7n);
      const s1 = rotr(w[t - 2], 19n) ^ rotr(w[t - 2], 61n) ^ (w[t - 2] >> 6n);
      w[t] = (w[t - 16] + s0 + w[t - 7] + s1) & MASK64;
    }
    let [a, b, c, d, e, f, g, hh] = h;
    for (let t = 0; t < 80; t++) {
      const S1 = rotr(e, 14n) ^ rotr(e, 18n) ^ rotr(e, 41n);
      const ch = (e & f) ^ (~e & MASK64 & g);
      const t1 = (hh + S1 + ch + SHA512_K[t] + w[t]) & MASK64;
      const S0 = rotr(a, 28n) ^ rotr(a, 34n) ^ rotr(a, 39n);
      const maj = (a & b) ^ (a & c) ^ (b & c);
      const t2 = (S0 + maj) & MASK64;
      hh = g;
      g = f;
      f = e;
      e = (d + t1) & MASK64;
      d = c;
      c = b;
      b = a;
      a = (t1 + t2) & MASK64;
    }
    const next = [a, b, c, d, e, f, g, hh];
    for (let i = 0; i < 8; i++) h[i] = (h[i] + next[i]) & MASK64;
  }
  const out = new Uint8Array(64);
  for (let i = 0; i < 8; i++) {
    for (let j = 0; j < 8; j++) out[i * 8 + j] = Number((h[i] >> BigInt(56 - 8 * j)) & 255n);
  }
  return out;
}

const BLAKE2B_IV = [
  0x6a09e667f3bcc908n, 0xbb67ae8584caa73bn, 0x3c6ef372fe94f82bn, 0xa54ff53a5f1d36f1n,
  0x510e527fade682d1n, 0x9b05688c2b3e6c1fn, 0x1f83d9abfb41bd6bn, 0x5be0cd19137e2179n,
];

const BLAKE2B_SIGMA = [
  [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
  [14, 10, 4, 8, 9, 15, 13, 6, 1, 12, 0, 2, 11, 7, 5, 3],
  [11, 8, 12, 0, 5, 2, 15, 13, 10, 14, 3, 6, 7, 1, 9, 4],
  [7, 9, 3, 1, 13, 12, 11, 14, 2, 6, 5, 10, 4, 0, 15, 8],
  [9, 0, 5, 7, 2, 4, 10, 15, 14, 1, 11, 12, 6, 8, 3, 13],
  [2, 12, 6, 10, 0, 11, 8, 3, 4, 13, 7, 5, 15, 14, 1, 9],
  [12, 5, 1, 15, 14, 13, 4, 10, 0, 7, 6, 3, 9, 2, 8, 11],
  [13, 11, 7, 14, 12, 1, 3, 9, 5, 0, 15, 4, 8, 6, 2, 10],
  [6, 15, 14, 9, 11, 3, 0, 8, 12, 2, 13, 7, 1, 4, 10, 5],
  [10, 2, 8, 4, 7, 6, 1, 5, 15, 11, 9, 14, 3, 12, 13, 0],
];

// BLAKE2b-512 with no key (RFC 7693).
function blake2b512(bytes) {
  const h = BLAKE2B_IV.slice();
  h[0] ^= 0x01010040n;
  const blocks = Math.max(1, Math.ceil(bytes.length / 128));
  for (let block = 0; block < blocks; block++) {
    const last = block === blocks - 1;
    const m = new Array(16);
    for (let t = 0; t < 16; t++) {
      let word = 0n;
      for (let i = 7; i >= 0; i--) {
        const at = block * 128 + t * 8 + i;
        word = (word << 8n) | BigInt(at < bytes.length ? bytes[at] : 0);
      }
      m[t] = word;
    }
    const counter = BigInt(last ? bytes.length : (block + 1) * 128);
    const v = h.concat(BLAKE2B_IV);
    v[12] ^= counter & MASK64;
    v[13] ^= counter >> 64n;
    if (last) v[14] ^= MASK64;
    const g = (a, b, c, d, x, y) => {
      v[a] = (v[a] + v[b] + x) & MASK64;
      v[d] = rotr(v[d] ^ v[a], 32n);
      v[c] = (v[c] + v[d]) & MASK64;
      v[b] = rotr(v[b] ^ v[c], 24n);
      v[a] = (v[a] + v[b] + y) & MASK64;
      v[d] = rotr(v[d] ^ v[a], 16n);
      v[c] = (v[c] + v[d]) & MASK64;
      v[b] = rotr(v[b] ^ v[c], 63n);
    };
    for (let round = 0; round < 12; round++) {
      const s = BLAKE2B_SIGMA[round % 10];
      g(0, 4, 8, 12, m[s[0]], m[s[1]]);
      g(1, 5, 9, 13, m[s[2]], m[s[3]]);
      g(2, 6, 10, 14, m[s[4]], m[s[5]]);
      g(3, 7, 11, 15, m[s[6]], m[s[7]]);
      g(0, 5, 10, 15, m[s[8]], m[s[9]]);
      g(1, 6, 11, 12, m[s[10]], m[s[11]]);
      g(2, 7, 8, 13, m[s[12]], m[s[13]]);
      g(3, 4, 9, 14, m[s[14]], m[s[15]]);
    }
    for (let i = 0; i < 8; i++) h[i] ^= v[i] ^ v[i + 8];
  }
  const out = new Uint8Array(64);
  for (let i = 0; i < 8; i++) {
    for (let j = 0; j < 8; j++) out[i * 8 + j] = Number((h[i] >> BigInt(8 * j)) & 255n);
  }
  return out;
}

// RFC 8032 5.1.7: the signature `signature` (64 bytes) of `message` by the
// public key `key` (32 bytes), with S below L and R and A canonical points.
function ed25519Verify(key, message, signature) {
  if (key.length !== 32 || signature.length !== 64) return false;
  const a = decode(key);
  const r = signature.subarray(0, 32);
  if (a === null || decode(r) === null) return false;
  const s = bytesToNumber(signature.subarray(32));
  if (s >= L) return false;
  const k = mod(bytesToNumber(sha512(concat([r, key, message]))), L);
  const expected = encode(add(multiply(s, BASE), negate(multiply(k, a))));
  return equal(expected, r);
}

function concat(parts) {
  const out = new Uint8Array(parts.reduce((n, part) => n + part.length, 0));
  let at = 0;
  for (const part of parts) {
    out.set(part, at);
    at += part.length;
  }
  return out;
}

function equal(a, b) {
  if (a.length !== b.length) return false;
  for (let i = 0; i < a.length; i++) if (a[i] !== b[i]) return false;
  return true;
}

const BASE64 = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

// Strict base64: padded, no other character, and the encoding of what it
// decodes to (no stray bits), or null.
function fromBase64(text) {
  if (text.length % 4 !== 0 || !/^[A-Za-z0-9+/]*={0,2}$/.test(text)) return null;
  const pad = text.endsWith("==") ? 2 : text.endsWith("=") ? 1 : 0;
  const out = new Uint8Array((text.length / 4) * 3 - pad);
  let at = 0;
  for (let i = 0; i < text.length; i += 4) {
    const n = [0, 1, 2, 3].reduce(
      (acc, j) => acc * 64 + (text[i + j] === "=" ? 0 : BASE64.indexOf(text[i + j])),
      0,
    );
    for (const byte of [(n >> 16) & 255, (n >> 8) & 255, n & 255]) {
      if (at < out.length) out[at++] = byte;
    }
  }
  return toBase64(out) === text ? out : null;
}

function toBase64(bytes) {
  let text = "";
  for (let i = 0; i < bytes.length; i += 3) {
    const n = (bytes[i] << 16) | ((bytes[i + 1] ?? 0) << 8) | (bytes[i + 2] ?? 0);
    text += BASE64[(n >> 18) & 63] + BASE64[(n >> 12) & 63];
    text += i + 1 < bytes.length ? BASE64[(n >> 6) & 63] : "=";
    text += i + 2 < bytes.length ? BASE64[n & 63] : "=";
  }
  return text;
}

function fromHex(text) {
  if (text.length % 2 !== 0 || !/^[0-9a-f]*$/.test(text)) return null;
  const out = new Uint8Array(text.length / 2);
  for (let i = 0; i < out.length; i++) out[i] = Number.parseInt(text.slice(2 * i, 2 * i + 2), 16);
  return out;
}

function latin1(text) {
  const out = new Uint8Array(text.length);
  for (let i = 0; i < text.length; i++) {
    const code = text.charCodeAt(i);
    if (code > 255) return null;
    out[i] = code;
  }
  return out;
}

// minisign: the key line is "Ed", its 8-byte id and the 32-byte key; the .sig
// file (base64 again, as Tauri's signer leaves it) is four lines: an untrusted
// comment, "ED" with the key's id and the signature of BLAKE2b-512(file), the
// trusted comment, and the signature of the first signature and that comment.
function checkMinisign(keyLine, sigFileText, file, fileName) {
  const key = fromBase64(keyLine);
  if (key === null || key.length !== 42 || key[0] !== 0x45 || key[1] !== 0x64) {
    return "the public key is not a minisign Ed25519 key";
  }
  const sigText = fromBase64(sigFileText.trim());
  const lines = sigText === null ? null : String.fromCharCode(...sigText).split("\n");
  if (lines === null || lines.length !== 5 || lines[4] !== "") {
    return "the signature file is not a minisign signature";
  }
  if (!lines[0].startsWith("untrusted comment: ") || !lines[2].startsWith("trusted comment: ")) {
    return "the signature file is not a minisign signature";
  }
  const signature = fromBase64(lines[1]);
  const global = fromBase64(lines[3]);
  if (signature === null || signature.length !== 74 || global === null || global.length !== 64) {
    return "the signature file is not a minisign signature";
  }
  if (signature[0] !== 0x45 || signature[1] !== 0x44) {
    return "the signature is not of minisign's prehashed kind (ED)";
  }
  if (!equal(signature.subarray(2, 10), key.subarray(2, 10))) {
    return "the signature was made by another key";
  }
  const publicKey = key.subarray(10);
  if (!ed25519Verify(publicKey, blake2b512(file), signature.subarray(10))) {
    return "the signature does not match the file";
  }
  const comment = lines[2].slice("trusted comment: ".length);
  const commentBytes = latin1(comment);
  if (commentBytes === null || !ed25519Verify(publicKey, concat([signature.subarray(10), commentBytes]), global)) {
    return "the trusted comment's signature does not match";
  }
  const named = comment.match(/^timestamp:[0-9]+\tfile:(.*)$/);
  if (named === null || named[1] !== fileName) {
    return `the signature is for ${named === null ? "something else" : named[1]}, not ${fileName}`;
  }
  return null;
}

// The SHA-256 the signed list `file` gives `name`: a line of two fields, the
// hash (64 lowercase hex) and the name, or the name after `*` (sha256sum's
// binary mode). Null where no line, or more than one, names it.
function listedHash(file, name) {
  const found = [];
  for (const line of String.fromCharCode(...file).split("\n")) {
    const fields = line.split(/[ \t]+/).filter((field) => field !== "");
    if (fields.length === 2 && (fields[1] === name || fields[1] === `*${name}`)) found.push(fields[0]);
  }
  return found.length === 1 && /^[0-9a-f]{64}$/.test(found[0]) ? found[0] : null;
}

function run(argv) {
  if (argv.length !== 5) return "refused: the check takes a key, a signature, a file in hex, its name and the name of the file it lists";
  const file = fromHex(argv[2]);
  if (file === null) return "refused: the file is not hex";
  try {
    const problem = checkMinisign(argv[0], argv[1], file, argv[3]);
    if (problem !== null) return `refused: ${problem}`;
    const hash = listedHash(file, argv[4]);
    return hash === null ? `refused: lists no ${argv[4]}` : `ok ${hash}`;
  } catch (error) {
    return `refused: ${error}`;
  }
}
VERIFIER
}

main "$@"
