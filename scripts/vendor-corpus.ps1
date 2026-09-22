# Explicit source acquisition, not a runtime resolver or a normal CI step.
# Import creates a NEW corpus only; Verify re-downloads sources without edits.
param([ValidateSet('Import','Verify')][string]$Mode='Verify')
$ErrorActionPreference='Stop'
Add-Type -AssemblyName System.Net.Http
Add-Type -AssemblyName System.IO.Compression
$repoRoot=Split-Path -Parent $PSScriptRoot
$corpusRoot=Join-Path $repoRoot 'crates/glaux-standards/corpus'
$manifestPath=Join-Path $corpusRoot 'manifest.json'
if($Mode -eq 'Import' -and ((Test-Path $manifestPath) -or (Test-Path (Join-Path $corpusRoot 'originals')))){
    throw 'Import refuses to replace an existing corpus; use Verify.'
}
$client=[Net.Http.HttpClient]::new()
$client.Timeout=[TimeSpan]::FromSeconds(90)
$client.DefaultRequestHeaders.UserAgent.ParseAdd('Glaux-source-packaging/1.0')
$entries=[Collections.Generic.List[object]]::new()
$payloads=@{}
function Digest([byte[]]$Bytes){
    $hasher=[Security.Cryptography.SHA256]::Create()
    try { return [BitConverter]::ToString($hasher.ComputeHash($Bytes)).Replace('-','').ToLowerInvariant() }
    finally {$hasher.Dispose()}
}
function Fetch([string]$Url){
    # All callers supply fixed reviewed origins/pins below, never fixture refs.
    $response=$client.GetAsync($Url).GetAwaiter().GetResult()
    try {
        $null=$response.EnsureSuccessStatusCode()
        $bytes=$response.Content.ReadAsByteArrayAsync().GetAwaiter().GetResult()
        if($bytes.Length -gt 100MB){throw 'Source exceeds acquisition bound'}
        return @{bytes=$bytes;url=$Url;effective_url=$response.RequestMessage.RequestUri.AbsoluteUri;retrieved_at=[DateTime]::UtcNow.ToString('o')}
    } finally {$response.Dispose()}
}
function Add-Artifact($Path,$Kind,$Uri,$Source,$Revision,$Licence,$Bytes,$Aliases=@(),$Mirrors=@()){
    if($payloads.ContainsKey($Path)){throw "Duplicate artifact $Path"}
    $payloads[$Path]=$Bytes
    $entries.Add([ordered]@{
        path=$Path;kind=$Kind;uri=$Uri;aliases=@($Aliases);source_url=$Source.url;
        effective_source_url=$Source.effective_url;retrieved_at=$Source.retrieved_at;
        source_revision=$Revision;bytes=$Bytes.Length;sha256=(Digest $Bytes);
        licence=$Licence;verified_mirrors=@($Mirrors)
    })
}
function Mirror($Url,$Bytes){
    $remote=Fetch $Url
    if((Digest $remote.bytes) -cne (Digest $Bytes)){throw "Source variant changed: $Url; do not substitute it"}
    return [ordered]@{url=$Url;retrieved_at=$remote.retrieved_at;bytes=$remote.bytes.Length;sha256=(Digest $remote.bytes)}
}
try {
    $csapiPin='8e03b236a049849f2ccc24b4fd9fdce5ff69bed2'
    $csapiBase="https://raw.githubusercontent.com/opengeospatial/ogcapi-connected-systems/$csapiPin/"
    $archive=Fetch "https://codeload.github.com/opengeospatial/ogcapi-connected-systems/zip/$csapiPin"
    $memory=[IO.MemoryStream]::new($archive.bytes)
    $zip=[IO.Compression.ZipArchive]::new($memory)
    $prefix="ogcapi-connected-systems-$csapiPin/"
    $schemaCount=0
    try {
        foreach($entry in $zip.Entries){
            if(-not $entry.FullName.StartsWith($prefix,[StringComparison]::Ordinal)){throw 'Unexpected archive prefix'}
            $relative=$entry.FullName.Substring($prefix.Length)
            $schema=$relative -cmatch '^(api/part[12]/openapi/schemas/.+|sensorml/schemas/json/[^/]+|swecommon/schemas/json/[^/]+)\.json$' -or $relative -cmatch '^common/(link|timeInstant|timeInstantOrNow|timePeriod)\.json$'
            $header=$relative -cin @('api/part1/standard/23-001r0.adoc','api/part2/standard/23-002r0.adoc','sensorml/standard/23-000.adoc','swecommon/standard/24-014.adoc')
            if(-not($schema -or $header -or $relative -ceq 'LICENSE')){continue}
            if($entry.Length -gt 5MB){throw 'Unexpected large selected artifact'}
            $reader=$entry.Open();$buffer=[IO.MemoryStream]::new()
            try {$reader.CopyTo($buffer);$bytes=$buffer.ToArray()} finally {$reader.Dispose();$buffer.Dispose()}
            $uri=$csapiBase+$relative
            $source=@{url=$uri;effective_url=$uri;retrieved_at=$archive.retrieved_at}
            $aliases=@();$mirrors=@()
            if($schema){$schemaCount++}
            if($relative.StartsWith('swecommon/schemas/json/')){
                $canonical='https://schemas.opengis.net/sweCommon/3.0/json/'+($relative.Split('/')[-1])
                $mirrors=@(Mirror $canonical $bytes);$aliases=@($canonical)
            }
            $kind=if($schema){'schema'}elseif($header){'source-header'}else{'licence'}
            Add-Artifact ("originals/csapi/"+$relative) $kind $uri $source $csapiPin 'originals/csapi/LICENSE' $bytes $aliases $mirrors
        }
    } finally {$zip.Dispose();$memory.Dispose()}
    if($schemaCount -ne 116){throw "Reviewed CSAPI closure changed: $schemaCount schemas"}
    $geoPin='f2bc6e8f1e8e1ba376d901914ecf8d0fed947d6e'
    $geoSource='660d67d1d44d168aa2bba3931fb23618b058d14b'
    foreach($name in @('Feature','FeatureCollection','Geometry','Point')){
        $remote=Fetch "https://raw.githubusercontent.com/geojson/schema/$geoPin/$name.json"
        $canonical="https://geojson.org/schema/$name.json"
        $mirror=Mirror $canonical $remote.bytes
        Add-Artifact "originals/geojson/$name.json" 'schema' $canonical $remote $geoPin 'originals/licences/geojson-MIT.md' $remote.bytes @($remote.url) @($mirror)
    }
    $remote=Fetch "https://raw.githubusercontent.com/geojson/schema/$geoSource/license.md"
    Add-Artifact 'originals/licences/geojson-MIT.md' 'licence' $remote.url $remote $geoSource 'originals/licences/geojson-MIT.md' $remote.bytes
    $json2020='add836e705c9a07434c467b6b90946ba45258a73'
    foreach($name in @('schema','meta/core','meta/applicator','meta/unevaluated','meta/validation','meta/meta-data','meta/format-annotation','meta/content')){
        $canonical="https://json-schema.org/draft/2020-12/$name"
        $remote=Fetch $canonical
        # Hosted meta resources are digest-pinned snapshots. Some differ from
        # the historical tag; do not assign that tag's identity to these bytes.
        Add-Artifact "originals/json-schema/2020-12/$name.json" 'schema' $canonical $remote $null 'originals/licences/json-schema-LICENSE' $remote.bytes
    }
    $remote=Fetch 'https://json-schema.org/draft-07/schema'
    Add-Artifact 'originals/json-schema/draft-07/schema.json' 'schema' 'http://json-schema.org/draft-07/schema' $remote $null 'originals/licences/json-schema-LICENSE' $remote.bytes @($remote.url)
    foreach($notice in @(
        @('add836e705c9a07434c467b6b90946ba45258a73','README.md','json-schema-2020-12-README.md'),
        @('1afc34b65ead445ff363cfc870a28de0ca56e20f','README.md','json-schema-draft-07-README.md'),
        @('4f56a9900674b27804f0ec32e3b7fdfa4efad695','LICENSE','json-schema-LICENSE')
    )){
        $remote=Fetch "https://raw.githubusercontent.com/json-schema-org/json-schema-spec/$($notice[0])/$($notice[1])"
        Add-Artifact ("originals/licences/"+$notice[2]) 'licence' $remote.url $remote $notice[0] 'originals/licences/json-schema-LICENSE' $remote.bytes
    }
    $ordered=@($entries|Sort-Object {$_.path})
    if(@($ordered|Where-Object {$_.kind -eq 'source-header'}).Count -ne 4){throw 'Missing version/source header'}
    $manifest=[ordered]@{
        format_version=1;
        purpose='Original initial schema corpus; packaging is not validation or conformance.';
        csapi_archive=[ordered]@{url=$archive.url;retrieved_at=$archive.retrieved_at;sha256=(Digest $archive.bytes)};
        artifacts=$ordered
    }
    if($Mode -eq 'Verify'){
        $saved=Get-Content -Raw -LiteralPath $manifestPath|ConvertFrom-Json
        if(($saved.artifacts.path -join "`n") -cne ($ordered.path -join "`n")){throw 'Manifest selection differs from reviewed source selection'}
        foreach($artifact in $ordered){
            $existing=$saved.artifacts|Where-Object {$_.path -ceq $artifact.path}
            $path=Join-Path $corpusRoot $artifact.path
            if(-not(Test-Path -LiteralPath $path)){throw "Missing packaged source: $($artifact.path)"}
            if((Digest ([IO.File]::ReadAllBytes($path))) -cne $artifact.sha256 -or $existing.sha256 -cne $artifact.sha256 -or $existing.bytes -ne $artifact.bytes){throw "Source correspondence failed: $($artifact.path)"}
            if($existing.uri -cne $artifact.uri -or $existing.source_revision -cne $artifact.source_revision -or ($existing.aliases -join "`n") -cne ($artifact.aliases -join "`n")){throw 'Source mapping changed'}
        }
        Write-Output "Fresh upstream verification: $($ordered.Count) original artifacts match; no local file changed."
    } else {
        # Mechanical byte-preserving vendoring: never regenerate upstream JSON.
        foreach($artifact in $ordered){
            $path=[IO.Path]::GetFullPath((Join-Path $corpusRoot $artifact.path))
            if(-not $path.StartsWith([IO.Path]::GetFullPath($corpusRoot)+[IO.Path]::DirectorySeparatorChar,[StringComparison]::OrdinalIgnoreCase)){throw 'Artifact escapes corpus'}
            $null=New-Item -ItemType Directory -Force -Path (Split-Path -Parent $path)
            [IO.File]::WriteAllBytes($path,$payloads[$artifact.path])
        }
        [IO.File]::WriteAllText($manifestPath,($manifest|ConvertTo-Json -Depth 20)+"`n",[Text.UTF8Encoding]::new($false))
        Write-Output "Imported $($ordered.Count) original artifacts; 116 pinned CSAPI schemas; 23 SWE registry comparisons."
    }
} finally {$client.Dispose()}
