# ConsensFlow's installer for Windows:
#
#     irm https://consensflow.ngvoicu.dev/install.ps1 | iex
#
# It reads the release from the update feed the app reads (feed-stable, or
# feed-alpha with $env:CONSENSFLOW_CHANNEL = 'alpha'), or takes the one
# $env:CONSENSFLOW_VERSION names, and installs nothing until every check below
# has passed:
#
# - the release's SHA256SUMS carries a minisign signature by ConsensFlow's
#   update key (the key below, the one the app's updater holds), whose trusted
#   comment names that release: ConsensFlow_<version>_SHA256SUMS;
# - the installer it downloads has the SHA-256 that SHA256SUMS lists for it.
#
# A download through irm skips SmartScreen's own look, and the Windows builds
# carry no Authenticode signature: the signed SHA256SUMS is the proof. No tool
# is installed for it: the signature is checked by the PowerShell code below.
# Releases before 3.0.7-alpha.1 have no signed SHA256SUMS and are refused;
# their pages on GitHub have their downloads.
#
# The per-user installer runs quietly (no administrator rights), and an
# installed ConsensFlow is upgraded where it is. An installed ConsensFlow newer
# than the release is never replaced by it, unless --version names that
# release, and one installed for all users of the PC is never joined by a
# second, per-user one. The installer is held open from before its hash is
# taken until it has run, so nothing can write or delete it in between. An open
# ConsensFlow is asked to close its window, as closing it by hand would, and
# the install waits for it; one that stays open is never stopped, and nothing
# is installed. Right before the installer starts, nothing may run from the
# app's folder; Tauri's quiet installer itself closes a ConsensFlow that is
# opened in that last moment. `cf` comes inside the app: nothing else is set
# up. ConsensFlow is made for x64 Windows and Apple silicon Macs only.
#
# Options, as variables set before the command (irm | iex takes no arguments):
#
#     $env:CONSENSFLOW_CHANNEL = 'alpha'      the latest alpha ('stable' is the default)
#     $env:CONSENSFLOW_VERSION = '3.0.7'      that release ('v3.0.7' works too)
#     $env:CONSENSFLOW_RELEASES_URL = '...'   where the releases are, by default
#                                             https://github.com/ngvoicu/consensflow/releases
#
# or as arguments to the script itself:
#
#     & ([scriptblock]::Create((irm https://consensflow.ngvoicu.dev/install.ps1))) --alpha
#     ... --version 3.0.7      ... --help
#
# Uninstall: Settings > Apps > Installed apps > ConsensFlow > Uninstall, or run
# "$env:LOCALAPPDATA\ConsensFlow\uninstall.exe". Your projects, boards and
# settings stay in "$env:USERPROFILE\.consensflow"; delete that folder too to
# remove everything.
#
# Errors are thrown, never `exit`: under `irm | iex` an exit would close the
# PowerShell window it runs in. Everything is inside the script block, run on
# the last line, so a download cut short runs nothing. It writes only in a
# folder of its own under TEMP, consensflow-install.<pid>.<12 hex>, removed at
# the end, and where the installer puts the app. A run that is killed leaves that
# folder: the next run, before anything else, removes the folder of a run that is
# gone (never one that may be an install still going on, never a link, never what
# is not named exactly so), and says what it removed or left.

& {
    $ErrorActionPreference = 'Stop'
    $ProgressPreference = 'SilentlyContinue'

    # ConsensFlow's update key (minisign, as app/src-tauri/tauri.conf.json
    # holds it inside its own base64).
    $key = 'RWTirTrEjU6epgN2c8yV2aIfRHzcCHVBi9zddieRVM5SahwhDFJlFd7Q'
    $quitWait = 120

    function Stop-CfInstall([string]$why) {
        throw "consensflow: $why`nconsensflow: nothing was installed."
    }

    $usage = @'
Install ConsensFlow on this PC.

    irm https://consensflow.ngvoicu.dev/install.ps1 | iex
    & ([scriptblock]::Create((irm https://consensflow.ngvoicu.dev/install.ps1))) [options]

Options:
  --alpha            the latest alpha (the alpha update channel)
  --stable           the latest release (the default)
  --version VERSION  that release, for example 3.0.7 (v3.0.7 works too)
  --help             this help

Environment (each the same as its option):
  $env:CONSENSFLOW_CHANNEL = 'stable' or 'alpha'
  $env:CONSENSFLOW_VERSION = 'VERSION'
  $env:CONSENSFLOW_RELEASES_URL = 'URL'   where the releases are, by default
                                          https://github.com/ngvoicu/consensflow/releases

Uninstall: Settings > Apps > Installed apps > ConsensFlow > Uninstall, or run
"$env:LOCALAPPDATA\ConsensFlow\uninstall.exe". Your projects, boards and
settings stay in "$env:USERPROFILE\.consensflow"; delete that folder too to
remove everything.
'@

    $official = 'https://github.com/ngvoicu/consensflow/releases'
    $releases = if ($env:CONSENSFLOW_RELEASES_URL) { $env:CONSENSFLOW_RELEASES_URL.TrimEnd('/') } else { $official }
    $channel = if ($env:CONSENSFLOW_CHANNEL) { $env:CONSENSFLOW_CHANNEL } else { 'stable' }
    $version = if ($env:CONSENSFLOW_VERSION) { $env:CONSENSFLOW_VERSION } else { '' }
    $asked = $version
    $words = @($args)
    for ($i = 0; $i -lt $words.Count; $i++) {
        $word = [string]$words[$i]
        if ($word -eq '--alpha' -or $word -eq '-Alpha') { $channel = 'alpha' }
        elseif ($word -eq '--stable' -or $word -eq '-Stable') { $channel = 'stable' }
        elseif ($word -eq '--version' -or $word -eq '-Version') {
            if ($i + 1 -ge $words.Count) { throw 'consensflow: --version needs a release, for example --version 3.0.7' }
            $i++
            $version = [string]$words[$i]
            $asked = $version
        }
        elseif ($word.StartsWith('--version=')) { $version = $word.Substring('--version='.Length); $asked = $version }
        elseif ($word -eq '--help' -or $word -eq '-h' -or $word -eq '-Help') { return $usage }
        else { throw "consensflow: unknown option '$word' (see --help)" }
    }
    if ($channel -cne 'stable' -and $channel -cne 'alpha') { throw "consensflow: the channel is stable or alpha, not '$channel'" }
    if ($version) {
        $version = $version -replace '^v', ''
        if ($version -cnotmatch '\A[0-9][0-9A-Za-z.+-]*\z') { throw "consensflow: a version is a release number such as 3.0.7, not '$version'" }
    }

    # PowerShell 7 runs on Linux and macOS too: $IsWindows is there, and false.
    if ($PSVersionTable.PSEdition -eq 'Core' -and -not $IsWindows) {
        if ($IsLinux) { Stop-CfInstall 'ConsensFlow is made for macOS and Windows only; there is no Linux build.' }
        Stop-CfInstall 'this is the installer for Windows. On a Mac, run in Terminal: curl -fsSL https://consensflow.ngvoicu.dev/install.sh | sh'
    }
    $arch = if ($env:PROCESSOR_ARCHITEW6432) { $env:PROCESSOR_ARCHITEW6432 } else { $env:PROCESSOR_ARCHITECTURE }
    if ($arch -ne 'AMD64') { Stop-CfInstall "ConsensFlow for Windows is made for x64 (64-bit Intel and AMD) processors; this PC's is $arch." }

    # verifier: begin
    # The check of a release's signed SHA256SUMS: minisign's "ED" signature,
    # Ed25519 (RFC 8032) over the BLAKE2b-512 (RFC 7693) of the file, and its
    # global signature over the signature and the trusted comment. Written
    # out here because neither Windows PowerShell's .NET nor PowerShell 7's
    # checks Ed25519; the numbers are .NET's BigInteger and its SHA-512.
    # Add-Type -AssemblyName loads .NET's own System.Numerics assembly; it
    # compiles nothing.
    Add-Type -AssemblyName System.Numerics
    $BI = [System.Numerics.BigInteger]
    $CfP = $BI::Subtract($BI::Pow([System.Numerics.BigInteger]2, 255), [System.Numerics.BigInteger]19)
    $CfL = $BI::Add($BI::Pow([System.Numerics.BigInteger]2, 252), $BI::Parse('27742317777372353535851937790883648493'))
    $CfTwo = [System.Numerics.BigInteger]2

    function CfMod($a) {
        $r = $BI::Remainder($a, $CfP)
        if ($r.Sign -lt 0) { $r = $BI::Add($r, $CfP) }
        $r
    }

    function CfInverse($a) { $BI::ModPow((CfMod $a), $BI::Subtract($CfP, $CfTwo), $CfP) }

    $CfD = CfMod ($BI::Multiply([System.Numerics.BigInteger](-121665), (CfInverse ([System.Numerics.BigInteger]121666))))
    $CfD2 = CfMod ($BI::Multiply($CfTwo, $CfD))
    $CfSqrtM1 = $BI::ModPow($CfTwo, $BI::Divide($BI::Subtract($CfP, [System.Numerics.BigInteger]1), [System.Numerics.BigInteger]4), $CfP)

    function CfNumber([byte[]]$bytes) {
        $unsigned = New-Object byte[] ($bytes.Length + 1)
        [Array]::Copy($bytes, $unsigned, $bytes.Length)
        New-Object System.Numerics.BigInteger (, $unsigned)
    }

    function CfBytes($n, [int]$length) {
        $out = New-Object byte[] $length
        $le = $n.ToByteArray()
        [Array]::Copy($le, $out, [Math]::Min($le.Length, $length))
        , $out
    }

    # Points in extended coordinates (X, Y, Z, T): x = X/Z, y = Y/Z, x*y = T/Z.
    function CfAdd($p, $q) {
        $a = $BI::Remainder($BI::Multiply($BI::Subtract($p[1], $p[0]), $BI::Subtract($q[1], $q[0])), $CfP)
        $b = $BI::Remainder($BI::Multiply($BI::Add($p[1], $p[0]), $BI::Add($q[1], $q[0])), $CfP)
        $c = $BI::Remainder($BI::Multiply($BI::Multiply($CfD2, $p[3]), $q[3]), $CfP)
        $d = $BI::Remainder($BI::Multiply($BI::Multiply($CfTwo, $p[2]), $q[2]), $CfP)
        $e = $BI::Subtract($b, $a)
        $f = $BI::Subtract($d, $c)
        $g = $BI::Add($d, $c)
        $h = $BI::Add($b, $a)
        , @(
            $BI::Remainder($BI::Multiply($e, $f), $CfP),
            $BI::Remainder($BI::Multiply($g, $h), $CfP),
            $BI::Remainder($BI::Multiply($f, $g), $CfP),
            $BI::Remainder($BI::Multiply($e, $h), $CfP)
        )
    }

    function CfMultiply($scalar, $point) {
        $result = @($BI::Zero, $BI::One, $BI::One, $BI::Zero)
        $p = $point
        $s = $scalar
        while ($s.Sign -gt 0) {
            if (-not $s.IsEven) { $result = CfAdd $result $p }
            $p = CfAdd $p $p
            $s = $BI::Divide($s, $CfTwo)
        }
        , $result
    }

    function CfEncode($p) {
        $zi = CfInverse $p[2]
        $x = CfMod ($BI::Multiply($p[0], $zi))
        $y = CfMod ($BI::Multiply($p[1], $zi))
        if (-not $x.IsEven) { $y = $BI::Add($y, $BI::Pow($CfTwo, 255)) }
        CfBytes $y 32
    }

    # RFC 8032 5.1.3; a point not on the curve, or not in canonical form, is $null.
    function CfDecode([byte[]]$bytes) {
        $n = CfNumber $bytes
        $sign = ($bytes[31] -shr 7) -band 1
        $y = $BI::Remainder($n, $BI::Pow($CfTwo, 255))
        if ($y -ge $CfP) { return $null }
        $y2 = $BI::Multiply($y, $y)
        $u = CfMod ($BI::Subtract($y2, $BI::One))
        $v = CfMod ($BI::Add($BI::Multiply($CfD, $y2), $BI::One))
        $x2 = CfMod ($BI::Multiply($u, (CfInverse $v)))
        $x = $BI::ModPow($x2, $BI::Divide($BI::Add($CfP, [System.Numerics.BigInteger]3), [System.Numerics.BigInteger]8), $CfP)
        if (-not (CfMod ($BI::Subtract($BI::Multiply($x, $x), $x2))).IsZero) { $x = CfMod ($BI::Multiply($x, $CfSqrtM1)) }
        if (-not (CfMod ($BI::Subtract($BI::Multiply($x, $x), $x2))).IsZero) { return $null }
        if ($x.IsZero -and $sign -eq 1) { return $null }
        if (($x.IsEven -and $sign -eq 1) -or (-not $x.IsEven -and $sign -eq 0)) { $x = $BI::Subtract($CfP, $x) }
        , @($x, $y, $BI::One, (CfMod ($BI::Multiply($x, $y))))
    }

    $CfBase = CfDecode (CfBytes (CfMod ($BI::Multiply([System.Numerics.BigInteger]4, (CfInverse ([System.Numerics.BigInteger]5))))) 32)

    function CfJoin {
        $out = New-Object System.Collections.Generic.List[byte]
        foreach ($part in $args) { $out.AddRange([byte[]]$part) }
        , $out.ToArray()
    }

    function CfSame([byte[]]$a, [byte[]]$b) {
        if ($a.Length -ne $b.Length) { return $false }
        for ($i = 0; $i -lt $a.Length; $i++) { if ($a[$i] -ne $b[$i]) { return $false } }
        $true
    }

    # RFC 8032 5.1.7: the 64-byte signature of $message by the 32-byte $key,
    # with S below L and R and A canonical points.
    function Test-CfEd25519([byte[]]$key, [byte[]]$message, [byte[]]$signature) {
        if ($key.Length -ne 32 -or $signature.Length -ne 64) { return $false }
        $a = CfDecode $key
        $r = [byte[]]$signature[0..31]
        if ($null -eq $a -or $null -eq (CfDecode $r)) { return $false }
        $s = CfNumber ([byte[]]$signature[32..63])
        if ($s -ge $CfL) { return $false }
        $sha = [System.Security.Cryptography.SHA512]::Create()
        try { $digest = $sha.ComputeHash((CfJoin $r $key $message)) } finally { $sha.Dispose() }
        $k = $BI::Remainder((CfNumber $digest), $CfL)
        $ka = CfMultiply $k $a
        $minusKa = @((CfMod ($BI::Negate($ka[0]))), $ka[1], $ka[2], (CfMod ($BI::Negate($ka[3]))))
        CfSame (CfEncode (CfAdd (CfMultiply $s $CfBase) $minusKa)) $r
    }

    # BLAKE2b's 64-bit words as UInt64: PowerShell turns a UInt64 sum that
    # overflows into a Double, so sums are made of 32-bit halves.
    $CfLow32 = [uint64]4294967295
    function CfAdd64([uint64]$a, [uint64]$b) {
        $low = ($a -band $CfLow32) + ($b -band $CfLow32)
        $high = (($a -shr 32) + ($b -shr 32) + ($low -shr 32)) -band $CfLow32
        [uint64](($high -shl 32) -bor ($low -band $CfLow32))
    }
    function CfRotr64([uint64]$x, [int]$n) { [uint64](($x -shr $n) -bor ($x -shl (64 - $n))) }

    $CfBlakeIv = [uint64[]]@(
        '6a09e667f3bcc908', 'bb67ae8584caa73b', '3c6ef372fe94f82b', 'a54ff53a5f1d36f1',
        '510e527fade682d1', '9b05688c2b3e6c1f', '1f83d9abfb41bd6b', '5be0cd19137e2179'
    ).ForEach({ [Convert]::ToUInt64($_, 16) })
    $CfSigma = @(
        @(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15),
        @(14, 10, 4, 8, 9, 15, 13, 6, 1, 12, 0, 2, 11, 7, 5, 3),
        @(11, 8, 12, 0, 5, 2, 15, 13, 10, 14, 3, 6, 7, 1, 9, 4),
        @(7, 9, 3, 1, 13, 12, 11, 14, 2, 6, 5, 10, 4, 0, 15, 8),
        @(9, 0, 5, 7, 2, 4, 10, 15, 14, 1, 11, 12, 6, 8, 3, 13),
        @(2, 12, 6, 10, 0, 11, 8, 3, 4, 13, 7, 5, 15, 14, 1, 9),
        @(12, 5, 1, 15, 14, 13, 4, 10, 0, 7, 6, 3, 9, 2, 8, 11),
        @(13, 11, 7, 14, 12, 1, 3, 9, 5, 0, 15, 4, 8, 6, 2, 10),
        @(6, 15, 14, 9, 11, 3, 0, 8, 12, 2, 13, 7, 1, 4, 10, 5),
        @(10, 2, 8, 4, 7, 6, 1, 5, 15, 11, 9, 14, 3, 12, 13, 0)
    )

    function CfBlakeMix([uint64[]]$v, [int]$a, [int]$b, [int]$c, [int]$d, [uint64]$x, [uint64]$y) {
        $v[$a] = CfAdd64 (CfAdd64 $v[$a] $v[$b]) $x
        $v[$d] = CfRotr64 ($v[$d] -bxor $v[$a]) 32
        $v[$c] = CfAdd64 $v[$c] $v[$d]
        $v[$b] = CfRotr64 ($v[$b] -bxor $v[$c]) 24
        $v[$a] = CfAdd64 (CfAdd64 $v[$a] $v[$b]) $y
        $v[$d] = CfRotr64 ($v[$d] -bxor $v[$a]) 16
        $v[$c] = CfAdd64 $v[$c] $v[$d]
        $v[$b] = CfRotr64 ($v[$b] -bxor $v[$c]) 63
    }

    # BLAKE2b-512 with no key (RFC 7693).
    function Get-CfBlake2b512([byte[]]$bytes) {
        $h = [uint64[]]$CfBlakeIv.Clone()
        $h[0] = $h[0] -bxor [uint64]16842816
        $blocks = [Math]::Max(1, [Math]::Ceiling($bytes.Length / 128))
        for ($block = 0; $block -lt $blocks; $block++) {
            $last = $block -eq $blocks - 1
            $chunk = New-Object byte[] 128
            $start = $block * 128
            [Array]::Copy($bytes, $start, $chunk, 0, [Math]::Min(128, $bytes.Length - $start))
            $m = New-Object uint64[] 16
            for ($t = 0; $t -lt 16; $t++) { $m[$t] = [BitConverter]::ToUInt64($chunk, $t * 8) }
            $counter = if ($last) { [uint64]$bytes.Length } else { [uint64](($block + 1) * 128) }
            $v = New-Object uint64[] 16
            for ($i = 0; $i -lt 8; $i++) { $v[$i] = $h[$i]; $v[$i + 8] = $CfBlakeIv[$i] }
            $v[12] = $v[12] -bxor $counter
            if ($last) { $v[14] = $v[14] -bxor [uint64]::MaxValue }
            for ($round = 0; $round -lt 12; $round++) {
                $s = $CfSigma[$round % 10]
                CfBlakeMix $v 0 4 8 12 $m[$s[0]] $m[$s[1]]
                CfBlakeMix $v 1 5 9 13 $m[$s[2]] $m[$s[3]]
                CfBlakeMix $v 2 6 10 14 $m[$s[4]] $m[$s[5]]
                CfBlakeMix $v 3 7 11 15 $m[$s[6]] $m[$s[7]]
                CfBlakeMix $v 0 5 10 15 $m[$s[8]] $m[$s[9]]
                CfBlakeMix $v 1 6 11 12 $m[$s[10]] $m[$s[11]]
                CfBlakeMix $v 2 7 8 13 $m[$s[12]] $m[$s[13]]
                CfBlakeMix $v 3 4 9 14 $m[$s[14]] $m[$s[15]]
            }
            for ($i = 0; $i -lt 8; $i++) { $h[$i] = $h[$i] -bxor $v[$i] -bxor $v[$i + 8] }
        }
        $out = New-Object System.Collections.Generic.List[byte]
        foreach ($word in $h) { $out.AddRange([BitConverter]::GetBytes([uint64]$word)) }
        , $out.ToArray()
    }

    # Strict base64: padded, nothing else, and the encoding of what it decodes
    # to (no stray bits), or $null.
    function CfFromBase64([string]$text) {
        if ($text.Length % 4 -ne 0 -or $text -cnotmatch '\A[A-Za-z0-9+/]*={0,2}\z') { return $null }
        try { $bytes = [Convert]::FromBase64String($text) } catch { return $null }
        if ([Convert]::ToBase64String($bytes) -cne $text) { return $null }
        , $bytes
    }

    # minisign: the key line is "Ed", its 8-byte id and the 32-byte key; the
    # .sig file (base64 again, as Tauri's signer leaves it) is four lines: an
    # untrusted comment, "ED" with the key's id and the signature of
    # BLAKE2b-512(file), the trusted comment, and the signature of the first
    # signature and that comment. $null when it holds, else what is wrong.
    function Test-CfMinisign([string]$keyLine, [string]$sigFileText, [byte[]]$file, [string]$fileName) {
        $latin1 = [Text.Encoding]::GetEncoding(28591)
        $key = CfFromBase64 $keyLine
        if ($null -eq $key -or $key.Length -ne 42 -or $key[0] -ne 0x45 -or $key[1] -ne 0x64) {
            return 'the public key is not a minisign Ed25519 key'
        }
        $sigBytes = CfFromBase64 $sigFileText.Trim()
        $notMinisign = 'the signature file is not a minisign signature'
        if ($null -eq $sigBytes) { return $notMinisign }
        $lines = $latin1.GetString($sigBytes).Split([char]10)
        if ($lines.Length -ne 5 -or $lines[4].Length -ne 0) { return $notMinisign }
        if (-not $lines[0].StartsWith('untrusted comment: ', [StringComparison]::Ordinal) -or
            -not $lines[2].StartsWith('trusted comment: ', [StringComparison]::Ordinal)) { return $notMinisign }
        $signature = CfFromBase64 $lines[1]
        $global = CfFromBase64 $lines[3]
        if ($null -eq $signature -or $signature.Length -ne 74 -or $null -eq $global -or $global.Length -ne 64) { return $notMinisign }
        if ($signature[0] -ne 0x45 -or $signature[1] -ne 0x44) { return "the signature is not of minisign's prehashed kind (ED)" }
        if (-not (CfSame ([byte[]]$signature[2..9]) ([byte[]]$key[2..9]))) { return 'the signature was made by another key' }
        $publicKey = [byte[]]$key[10..41]
        $edSignature = [byte[]]$signature[10..73]
        if (-not (Test-CfEd25519 $publicKey (Get-CfBlake2b512 $file) $edSignature)) { return 'the signature does not match the file' }
        $comment = $lines[2].Substring('trusted comment: '.Length)
        if (-not (Test-CfEd25519 $publicKey (CfJoin $edSignature $latin1.GetBytes($comment)) ([byte[]]$global))) {
            return "the trusted comment's signature does not match"
        }
        $named = [regex]::Match($comment, '\Atimestamp:[0-9]+\tfile:(.*)\z')
        if (-not $named.Success) { return "the signature is for something else, not $fileName" }
        if (-not [string]::Equals($named.Groups[1].Value, $fileName, [StringComparison]::Ordinal)) {
            return "the signature is for $($named.Groups[1].Value), not $fileName"
        }
        $null
    }

    # The SHA-256 the signed list $file gives $name: a line of two fields, the
    # hash (64 lowercase hex) and the name, or the name after * (sha256sum's
    # binary mode). $null where no line, or more than one, names it.
    function Get-CfListedHash([byte[]]$file, [string]$name) {
        $found = @()
        foreach ($line in [Text.Encoding]::GetEncoding(28591).GetString($file).Split([char]10)) {
            $fields = @($line.Split([char[]]@(32, 9), [StringSplitOptions]::RemoveEmptyEntries))
            if ($fields.Count -eq 2 -and ([string]::Equals($fields[1], $name, [StringComparison]::Ordinal) -or
                    [string]::Equals($fields[1], "*$name", [StringComparison]::Ordinal))) { $found += $fields[0] }
        }
        if ($found.Count -eq 1 -and $found[0] -cmatch '\A[0-9a-f]{64}\z') { return $found[0] }
        $null
    }
    # verifier: end

    [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
    function Get-CfDownload([string]$url, [string]$path) {
        Invoke-WebRequest -UseBasicParsing -Uri $url -OutFile $path
    }

    # The processes running from the folder $dir.
    function Get-CfOpen([string]$dir) {
        $inside = $dir.TrimEnd('\') + '\'
        @(Get-Process -ErrorAction SilentlyContinue | Where-Object {
                $_.Path -and $_.Path.StartsWith($inside, [StringComparison]::OrdinalIgnoreCase)
            })
    }

    # Asks the ConsensFlow that runs from $dir to close, as closing its window
    # does, and waits for every process of it to end. It is never stopped: one
    # that stays open ends the install.
    function Close-CfOpen([string]$dir) {
        $open = Get-CfOpen $dir
        if ($open.Count -eq 0) { return }
        $parents = @{}
        foreach ($process in Get-CimInstance Win32_Process) { $parents[[int]$process.ProcessId] = [int]$process.ParentProcessId }
        $ids = @($open | ForEach-Object { $_.Id })
        $at = $PID
        $seen = @{}
        while ($at -and -not $seen.ContainsKey($at)) {
            if ($ids -contains $at) {
                Stop-CfInstall "this PowerShell runs inside the ConsensFlow in $dir, and closing it to upgrade would end the install too. Run this from Windows Terminal or another PowerShell window."
            }
            $seen[$at] = $true
            $at = $parents[$at]
        }
        Write-Host 'consensflow: ConsensFlow is open: asking it to close, as closing its window would...'
        foreach ($process in $open) {
            if ($process.MainWindowHandle -ne [IntPtr]::Zero) { [void]$process.CloseMainWindow() }
        }
        $deadline = (Get-Date).AddSeconds($quitWait)
        while ((Get-CfOpen $dir).Count -gt 0) {
            if ((Get-Date) -gt $deadline) {
                Stop-CfInstall "ConsensFlow in $dir is still open after $($quitWait)s. Close it and run this again."
            }
            Start-Sleep -Seconds 1
        }
    }

    # version order: begin
    # Whether the version $a comes after $b, by SemVer's precedence: the numbers
    # first, then a release after any of its pre-releases, then the
    # pre-release's fields one by one (numbers by value, below words; words by
    # their bytes), and a build's +metadata counts for nothing.
    function CfField([string]$x, [string]$y) {
        $xn = $x -cmatch '\A[0-9]+\z'
        $yn = $y -cmatch '\A[0-9]+\z'
        if ($xn -and $yn) {
            $x = $x.TrimStart([char]'0'); $y = $y.TrimStart([char]'0')
            if ($x.Length -ne $y.Length) { return [Math]::Sign($x.Length - $y.Length) }
            return [Math]::Sign([string]::CompareOrdinal($x, $y))
        }
        if ($xn) { return -1 }
        if ($yn) { return 1 }
        [Math]::Sign([string]::CompareOrdinal($x, $y))
    }
    function CfVersionOrder([string]$a, [string]$b) {
        $parts = foreach ($v in @($a, $b)) {
            $v = ($v -split '\+', 2)[0]
            $at = $v.IndexOf([char]'-')
            if ($at -ge 0) { , @($v.Substring(0, $at), $v.Substring($at + 1)) } else { , @($v, '') }
        }
        $am = $parts[0][0].Split([char]'.'); $bm = $parts[1][0].Split([char]'.')
        for ($k = 0; $k -lt 3; $k++) {
            $x = if ($k -lt $am.Length) { [regex]::Match($am[$k], '\A[0-9]*').Value } else { '' }
            $y = if ($k -lt $bm.Length) { [regex]::Match($bm[$k], '\A[0-9]*').Value } else { '' }
            $c = CfField $(if ($x) { $x } else { '0' }) $(if ($y) { $y } else { '0' })
            if ($c -ne 0) { return $c }
        }
        $ap = $parts[0][1]; $bp = $parts[1][1]
        if ($ap.Length -eq 0 -and $bp.Length -eq 0) { return 0 }
        if ($ap.Length -eq 0) { return 1 }
        if ($bp.Length -eq 0) { return -1 }
        $af = $ap.Split([char]'.'); $bf = $bp.Split([char]'.')
        for ($k = 0; $k -lt [Math]::Min($af.Length, $bf.Length); $k++) {
            $c = CfField $af[$k] $bf[$k]
            if ($c -ne 0) { return $c }
        }
        [Math]::Sign($af.Length - $bf.Length)
    }
    function Test-CfNewer([string]$a, [string]$b) { (CfVersionOrder $a $b) -gt 0 }
    # version order: end

    # Where the per-user installer put ConsensFlow, by its uninstall entry, and
    # a ConsensFlow installed for every user of the PC, by its own.
    $userEntry = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\ConsensFlow'
    $machineEntries = @('HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall\ConsensFlow', 'HKLM:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\ConsensFlow')
    function Get-CfInstalled {
        if (-not (Test-Path $userEntry)) { return $null }
        Get-ItemProperty $userEntry
    }
    function Get-CfMachineInstall {
        foreach ($entry in $machineEntries) {
            if (Test-Path $entry) { return Get-ItemProperty $entry }
        }
        $null
    }

    # Whether the process $process may be an install that goes on: any PowerShell (an
    # `irm | iex` runs inside one, and names nothing), or a program whose command line
    # names install.ps1.
    function Test-CfInstallGoesOn($process) {
        if (@('powershell', 'pwsh', 'powershell_ise') -contains $process.ProcessName.ToLowerInvariant()) { return $true }
        $line = Get-CimInstance Win32_Process -Filter "ProcessId = $($process.Id)" -ErrorAction SilentlyContinue
        [bool]($line -and $line.CommandLine -and $line.CommandLine -like '*install.ps1*')
    }

    # Deletes the folder $path and what is in it, a link inside it as a link: a junction or a
    # symbolic link is removed and its target is left, never entered (Directory.Delete with
    # recursion is not trusted with one, nor Remove-Item -Recurse in Windows PowerShell).
    function Remove-CfFolder([string]$path) {
        foreach ($entry in ([IO.DirectoryInfo]$path).GetFileSystemInfos()) {
            if ($entry.Attributes -band [IO.FileAttributes]::ReparsePoint) {
                if ($entry -is [IO.DirectoryInfo]) { [IO.Directory]::Delete($entry.FullName) } else { [IO.File]::Delete($entry.FullName) }
            } elseif ($entry -is [IO.DirectoryInfo]) {
                Remove-CfFolder $entry.FullName
            } else {
                $entry.Attributes = [IO.FileAttributes]::Normal
                [IO.File]::Delete($entry.FullName)
            }
        }
        [IO.Directory]::Delete($path)
    }

    # Takes away what a run that was killed (no `finally` runs) left in the user's own
    # temporary folder: its folder, consensflow-install.<pid>.<12 hex>, of a pid no install
    # runs as (a process that has it is an install only if Test-CfInstallGoesOn says it may
    # be). Only a real folder, never a link; the folder is deleted without following a link
    # inside it (Remove-CfFolder); this run's own folder ($own) is never one; every one is said in full.
    function Remove-CfStaleRuns([string]$own) {
        foreach ($item in Get-ChildItem -LiteralPath ([IO.Path]::GetTempPath()) -Force -ErrorAction SilentlyContinue) {
            if ($item.Name -cnotmatch '\Aconsensflow-install\.([1-9][0-9]{0,9})\.[0-9a-f]{12}\z') { continue }
            if ($item.FullName -eq $own) { continue }
            $pidOf = [long]($item.Name -split '\.')[1]
            $path = $item.FullName
            if (-not $item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
                Write-Host "consensflow: left $path alone: an install makes a folder there, and this is not one."
                continue
            }
            $process = if ($pidOf -ne $PID -and $pidOf -le [int]::MaxValue) { Get-Process -Id $pidOf -ErrorAction SilentlyContinue } else { $null }
            if ($process -and (Test-CfInstallGoesOn $process)) {
                Write-Host "consensflow: left $path alone: process $pidOf may be an install that is still going on."
                continue
            }
            try {
                Remove-CfFolder $path
                Write-Host "consensflow: removed $path, the folder an earlier install worked in (no install runs as process $pidOf)."
            } catch {
                Write-Host "consensflow: could not remove ${path}: $($_.Exception.Message)"
            }
        }
    }

    $tmp = Join-Path ([IO.Path]::GetTempPath()) ('consensflow-install.' + $PID + '.' + [guid]::NewGuid().ToString('N').Substring(0, 12))
    New-Item -ItemType Directory -Path $tmp | Out-Null
    try {
        Remove-CfStaleRuns $tmp
        $machine = Get-CfMachineInstall
        if ($machine) {
            $where = if ($machine.InstallLocation) { $machine.InstallLocation.Trim('"') } else { 'Program Files' }
            Stop-CfInstall "ConsensFlow $($machine.DisplayVersion) is installed for every user of this PC, in $where. This installer installs for your user only, and would put a second ConsensFlow beside it. Uninstall that one first (Settings > Apps > Installed apps > ConsensFlow), or keep updating it from the app."
        }
        if (-not $version) {
            $feed = "$releases/download/feed-$channel/latest.json"
            try { Get-CfDownload $feed (Join-Path $tmp 'latest.json') } catch { Stop-CfInstall "could not read the $channel update feed at ${feed}: $($_.Exception.Message)" }
            try { $version = [string](Get-Content -Raw (Join-Path $tmp 'latest.json') | ConvertFrom-Json).version } catch { $version = '' }
            if ($version -cnotmatch '\A[0-9][0-9A-Za-z.+-]*\z') { Stop-CfInstall "the $channel update feed at $feed names no version." }
        }
        Write-Host "consensflow: ConsensFlow $version"
        $before = Get-CfInstalled
        if ($before -and $before.DisplayVersion -and -not $asked -and (Test-CfNewer $before.DisplayVersion $version)) {
            Stop-CfInstall "ConsensFlow $($before.DisplayVersion) is installed, newer than $version, which the $channel channel names. To go back to $version, run this with --version $version (or `$env:CONSENSFLOW_VERSION = '$version')."
        }
        $release = "$releases/download/v$version"
        $signed = "ConsensFlow_${version}_SHA256SUMS"
        $installer = "ConsensFlow_${version}_x64-setup.exe"
        $sums = Join-Path $tmp 'SHA256SUMS'
        try { Get-CfDownload "$release/SHA256SUMS" $sums } catch { Stop-CfInstall "could not download $release/SHA256SUMS: is $version a release of ConsensFlow?" }
        try { Get-CfDownload "$release/SHA256SUMS.sig" "$sums.sig" } catch {
            Stop-CfInstall "ConsensFlow $version has no signed SHA256SUMS, so its files cannot be checked here: releases before 3.0.7-alpha.1 have none. Download it from $releases/tag/v$version; releases from 3.0.7-alpha.1 on install from here."
        }
        # SHA256SUMS is read once: the hash of the installer comes from the very
        # bytes whose signature was checked.
        $sumsBytes = [IO.File]::ReadAllBytes($sums)
        $problem = Test-CfMinisign $key (Get-Content -Raw "$sums.sig") $sumsBytes $signed
        if ($problem) { Stop-CfInstall "the signature of $version's SHA256SUMS does not hold: $problem" }
        Write-Host "consensflow: SHA256SUMS is signed by ConsensFlow's update key, for $version"
        $expected = Get-CfListedHash $sumsBytes $installer
        if (-not $expected) { Stop-CfInstall "$version's SHA256SUMS lists no $installer." }

        Write-Host "consensflow: downloading $installer"
        $setup = Join-Path $tmp $installer
        try { Get-CfDownload "$release/$installer" $setup } catch { Stop-CfInstall "could not download $release/${installer}: $($_.Exception.Message)" }
        # Held from before its hash is taken until it has run: others may read
        # it (Windows runs it), and nothing may write or delete it.
        $held = [IO.File]::Open($setup, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read)
        try {
            $hasher = [Security.Cryptography.SHA256]::Create()
            try { $got = -join ($hasher.ComputeHash($held) | ForEach-Object { $_.ToString('x2') }) } finally { $hasher.Dispose() }
            if (-not [string]::Equals($got, $expected, [StringComparison]::Ordinal)) {
                Stop-CfInstall "$installer was changed: its SHA-256 is $got, and the signed SHA256SUMS says $expected."
            }
            Write-Host "consensflow: $installer has the SHA-256 the signed SHA256SUMS lists"

            $dir = if ($before -and $before.InstallLocation) { $before.InstallLocation.Trim('"') } else { Join-Path $env:LOCALAPPDATA 'ConsensFlow' }
            Close-CfOpen $dir
            if ((Get-CfOpen $dir).Count -gt 0) {
                Stop-CfInstall "ConsensFlow in $dir was opened again while the install waited. Close it and run this again."
            }
            Write-Host "consensflow: installing ConsensFlow $version"
            $run = Start-Process -FilePath $setup -ArgumentList '/S' -Wait -PassThru
        } finally {
            $held.Dispose()
        }
        if ($run.ExitCode -ne 0) { Stop-CfInstall "the installer ended with code $($run.ExitCode)." }
        $after = Get-CfInstalled
        if (-not $after -or -not [string]::Equals([string]$after.DisplayVersion, $version, [StringComparison]::Ordinal)) {
            $said = if ($after) { $after.DisplayVersion } else { 'nothing' }
            throw "consensflow: the installer ran, and Windows lists ConsensFlow as version '$said', not $version."
        }
        $dir = $after.InstallLocation.Trim('"')
        if (-not (Test-Path -LiteralPath (Join-Path $dir 'ConsensFlow.exe'))) {
            throw "consensflow: the installer ran, and $dir has no ConsensFlow.exe."
        }
        Write-Host "consensflow: ConsensFlow $version is installed in $dir. Open it from the Start menu."
    } finally {
        Remove-Item -LiteralPath $tmp -Recurse -Force -ErrorAction SilentlyContinue
    }
} @args
