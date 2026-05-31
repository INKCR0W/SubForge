param(
    [string]$ArtifactsRoot = "release-assets",
    [string]$OutputDirectory = "release-metadata",
    [string]$CommitSha = $env:GITHUB_SHA,
    [string]$WorkflowRunId = $env:GITHUB_RUN_ID,
    [string]$Repository = $env:GITHUB_REPOSITORY,
    [string]$WorkflowName = $env:GITHUB_WORKFLOW,
    [string]$MetadataArtifactName = "subforge-release-metadata",
    [switch]$StageUploadAssets,
    [switch]$Generate,
    [switch]$GenerateNodeSbom,
    [switch]$Verify,
    [switch]$DryRun
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$MetadataFileNames = @("SHA256SUMS", "release-manifest.json")

function Resolve-AbsolutePath {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Path
    )

    if ([System.IO.Path]::IsPathRooted($Path)) {
        return [System.IO.Path]::GetFullPath($Path)
    }

    return [System.IO.Path]::GetFullPath((Join-Path (Get-Location).Path $Path))
}

function Get-RelativePath {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Root,
        [Parameter(Mandatory = $true)]
        [string]$Path
    )

    $rootFull = (Resolve-AbsolutePath -Path $Root).TrimEnd(
        [System.IO.Path]::DirectorySeparatorChar,
        [System.IO.Path]::AltDirectorySeparatorChar
    )
    $pathFull = Resolve-AbsolutePath -Path $Path
    $rootPrefix = $rootFull + [System.IO.Path]::DirectorySeparatorChar
    if ($pathFull -ne $rootFull -and -not $pathFull.StartsWith($rootPrefix, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw "路径不在根目录内：$Path"
    }

    return $pathFull.Substring($rootFull.Length).TrimStart("\", "/") -replace "\\", "/"
}

function Test-IsGeneratedMetadataFile {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Name
    )

    if ($MetadataFileNames -contains $Name) {
        return $true
    }

    return $false
}

function Parse-ReleaseAssetName {
    param(
        [Parameter(Mandatory = $true)]
        [string]$AssetName
    )

    $nameWithoutExtension = [System.IO.Path]::GetFileNameWithoutExtension($AssetName)
    if ($AssetName.EndsWith(".cdx.json", [System.StringComparison]::OrdinalIgnoreCase)) {
        $nameWithoutExtension = $AssetName.Substring(0, $AssetName.Length - ".cdx.json".Length)
    }

    $metadata = [ordered]@{
        component = $null
        platform = $null
        target = $null
        package_kind = $null
    }

    if ($nameWithoutExtension -match "^subforge-(core|desktop)-(ubuntu-22\.04|windows-10|macos-13)-(.+?)(?:-(nsis|dmg|deb|appimage))?$") {
        $metadata.component = "subforge-$($Matches[1])"
        $metadata.platform = $Matches[2]
        $metadata.target = $Matches[3]
        if ($Matches.ContainsKey(4) -and -not [string]::IsNullOrEmpty($Matches[4])) {
            $metadata.package_kind = $Matches[4]
        }
    }
    elseif ($nameWithoutExtension -match "sbom") {
        $metadata.component = "subforge"
        $metadata.package_kind = "sbom"
    }
    elseif ($nameWithoutExtension -match "manifest|checksum|sha256") {
        $metadata.component = "release-metadata"
    }

    return $metadata
}

function Copy-ReleaseArtifactsForUpload {
    param(
        [Parameter(Mandatory = $true)]
        [string]$SourceRoot,
        [Parameter(Mandatory = $true)]
        [string]$DestinationRoot,
        [Parameter(Mandatory = $true)]
        [string]$MetadataArtifact
    )

    if (-not (Test-Path -LiteralPath $SourceRoot -PathType Container)) {
        throw "产物目录不存在：$SourceRoot"
    }

    if (Test-Path -LiteralPath $DestinationRoot) {
        Remove-Item -LiteralPath $DestinationRoot -Recurse -Force
    }
    New-Item -ItemType Directory -Path $DestinationRoot -Force | Out-Null

    $copied = New-Object System.Collections.Generic.List[object]
    $files = Get-ChildItem -LiteralPath $SourceRoot -File -Recurse | Sort-Object FullName
    foreach ($file in $files) {
        $relativePath = Get-RelativePath -Root $SourceRoot -Path $file.FullName
        $parts = $relativePath -split "/"
        if ($parts.Count -lt 2) {
            continue
        }

        $artifactName = $parts[0]
        if ($artifactName -eq $MetadataArtifact) {
            $targetName = $file.Name
        }
        else {
            $extension = [System.IO.Path]::GetExtension($file.Name)
            $targetName = "$artifactName$extension"
            $targetPathProbe = Join-Path $DestinationRoot $targetName
            if (Test-Path -LiteralPath $targetPathProbe) {
                $targetName = "$artifactName--$($file.Name)"
            }
        }

        $targetPath = Join-Path $DestinationRoot $targetName
        if (Test-Path -LiteralPath $targetPath) {
            throw "发布产物命名冲突：$targetName"
        }

        Copy-Item -LiteralPath $file.FullName -Destination $targetPath -Force
        $copied.Add([ordered]@{
            source_artifact = $artifactName
            source_path = $relativePath
            upload_name = $targetName
        }) | Out-Null
    }

    if ($copied.Count -eq 0) {
        throw "未发现可上传的发布产物。"
    }

    Write-Host "已暂存 $($copied.Count) 个发布文件到：$DestinationRoot"
    return $copied
}

function Get-ManifestInputFiles {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Root
    )

    if (-not (Test-Path -LiteralPath $Root -PathType Container)) {
        throw "待生成 manifest 的目录不存在：$Root"
    }

    return @(Get-ChildItem -LiteralPath $Root -File -Recurse |
        Where-Object { -not (Test-IsGeneratedMetadataFile -Name $_.Name) } |
        Sort-Object FullName)
}

function New-ReleaseManifest {
    param(
        [Parameter(Mandatory = $true)]
        [string]$InputRoot,
        [Parameter(Mandatory = $true)]
        [string]$ManifestDirectory
    )

    if (-not (Test-Path -LiteralPath $ManifestDirectory)) {
        New-Item -ItemType Directory -Path $ManifestDirectory -Force | Out-Null
    }

    $files = Get-ManifestInputFiles -Root $InputRoot
    if ($files.Count -eq 0) {
        throw "没有可写入 SHA256SUMS 的发布产物。"
    }

    $records = New-Object System.Collections.Generic.List[object]
    foreach ($file in $files) {
        $relativePath = Get-RelativePath -Root $InputRoot -Path $file.FullName
        $hash = (Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
        $parsed = Parse-ReleaseAssetName -AssetName $file.Name
        $records.Add([ordered]@{
            asset_name = $file.Name
            relative_path = $relativePath
            component = $parsed.component
            platform = $parsed.platform
            target = $parsed.target
            package_kind = $parsed.package_kind
            size_bytes = $file.Length
            sha256 = $hash
        }) | Out-Null
    }

    $checksumLines = $records |
        Sort-Object { $_.relative_path } |
        ForEach-Object { "$($_.sha256)  $($_.relative_path)" }
    Set-Content -LiteralPath (Join-Path $ManifestDirectory "SHA256SUMS") -Value $checksumLines -Encoding ASCII

    $manifest = [ordered]@{
        schema_version = "subforge.release-manifest.v1"
        generated_at_utc = (Get-Date).ToUniversalTime().ToString("o")
        repository = $Repository
        commit_sha = $CommitSha
        workflow_name = $WorkflowName
        workflow_run_id = $WorkflowRunId
        checksum_file = "SHA256SUMS"
        provenance = [ordered]@{
            mechanism = "github-artifact-attestations"
            signer_workflow = ".github/workflows/release_ci.yml"
        }
        coverage = [ordered]@{
            included = "所有发布 payload 与 SBOM 文件"
            excluded = $MetadataFileNames
        }
        artifacts = $records
    }

    $manifest |
        ConvertTo-Json -Depth 8 |
        Set-Content -LiteralPath (Join-Path $ManifestDirectory "release-manifest.json") -Encoding UTF8

    Write-Host "已生成 SHA256SUMS 与 release-manifest.json，覆盖 $($records.Count) 个文件。"
}

function Test-ReleaseManifest {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Root,
        [string]$ExpectedCommitSha,
        [string]$ExpectedWorkflowRunId
    )

    $checksumPath = Join-Path $Root "SHA256SUMS"
    if (-not (Test-Path -LiteralPath $checksumPath -PathType Leaf)) {
        throw "缺少 SHA256SUMS：$checksumPath"
    }

    $expected = @{}
    foreach ($line in Get-Content -LiteralPath $checksumPath) {
        if ([string]::IsNullOrWhiteSpace($line)) {
            continue
        }
        if ($line -notmatch "^([a-fA-F0-9]{64})\s+\*?(.+)$") {
            throw "SHA256SUMS 行格式非法：$line"
        }

        $expected[$Matches[2]] = $Matches[1].ToLowerInvariant()
    }

    if ($expected.Count -eq 0) {
        throw "SHA256SUMS 未包含任何校验记录。"
    }

    foreach ($relativePath in $expected.Keys) {
        $filePath = Join-Path $Root ($relativePath -replace "/", [System.IO.Path]::DirectorySeparatorChar)
        if (-not (Test-Path -LiteralPath $filePath -PathType Leaf)) {
            throw "SHA256SUMS 指向的文件不存在：$relativePath"
        }

        $actualHash = (Get-FileHash -LiteralPath $filePath -Algorithm SHA256).Hash.ToLowerInvariant()
        if ($actualHash -ne $expected[$relativePath]) {
            throw "SHA256 校验失败：$relativePath"
        }
    }

    $payloadFiles = Get-ManifestInputFiles -Root $Root
    foreach ($file in $payloadFiles) {
        $relativePath = Get-RelativePath -Root $Root -Path $file.FullName
        if (-not $expected.ContainsKey($relativePath)) {
            throw "发布文件未被 SHA256SUMS 覆盖：$relativePath"
        }
    }

    $manifestPath = Join-Path $Root "release-manifest.json"
    if (-not (Test-Path -LiteralPath $manifestPath -PathType Leaf)) {
        throw "缺少 release-manifest.json：$manifestPath"
    }

    $manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
    if ($ExpectedCommitSha -and $manifest.commit_sha -and $manifest.commit_sha -ne $ExpectedCommitSha) {
        throw "manifest commit_sha 不匹配：期望 $ExpectedCommitSha，实际 $($manifest.commit_sha)"
    }
    if ($ExpectedWorkflowRunId -and $manifest.workflow_run_id -and [string]$manifest.workflow_run_id -ne [string]$ExpectedWorkflowRunId) {
        throw "manifest workflow_run_id 不匹配：期望 $ExpectedWorkflowRunId，实际 $($manifest.workflow_run_id)"
    }

    Write-Host "SHA256SUMS 与 release-manifest.json 校验通过，覆盖 $($expected.Count) 个文件。"
}

function Get-PnpmLockPackages {
    param(
        [Parameter(Mandatory = $true)]
        [string]$LockfilePath
    )

    if (-not (Test-Path -LiteralPath $LockfilePath -PathType Leaf)) {
        throw "缺少 pnpm-lock.yaml：$LockfilePath"
    }

    $packages = New-Object System.Collections.Generic.List[object]
    $seen = @{}
    $inPackages = $false
    foreach ($line in Get-Content -LiteralPath $LockfilePath) {
        if ($line -match "^\S") {
            $inPackages = $line -eq "packages:"
            continue
        }

        if (-not $inPackages) {
            continue
        }

        if ($line -match "^\s{2}('(@[^/']+/[^/']+)@([^']+)'|(@[^/']+/[^/']+)@([^:]+))\s*:\s*$") {
            $name = if ($Matches[2]) { $Matches[2] } else { $Matches[4] }
            $versionText = if ($Matches[3]) { $Matches[3] } else { $Matches[5] }
            $version = ($versionText -split "\(")[0]
            $encodedName = $name -replace "/", "%2F"
            $key = "$name@$version"
            if (-not $seen.ContainsKey($key)) {
                $seen[$key] = $true
                $packages.Add([ordered]@{
                    type = "library"
                    name = $name
                    version = $version
                    purl = "pkg:npm/$encodedName@$version"
                }) | Out-Null
            }
            continue
        }

        if ($line -match "^\s{2}('([^/@][^/']*)@([^']+)'|([^/@][^/']*)@([^:]+))\s*:\s*$") {
            $name = if ($Matches[2]) { $Matches[2] } else { $Matches[4] }
            $versionText = if ($Matches[3]) { $Matches[3] } else { $Matches[5] }
            $version = ($versionText -split "\(")[0]
            $key = "$name@$version"
            if (-not $seen.ContainsKey($key)) {
                $seen[$key] = $true
                $packages.Add([ordered]@{
                    type = "library"
                    name = $name
                    version = $version
                    purl = "pkg:npm/$name@$version"
                }) | Out-Null
            }
        }
    }

    return @($packages | Sort-Object name, version)
}

function New-NodeSbomFromPnpmLock {
    param(
        [Parameter(Mandatory = $true)]
        [string]$RepositoryRoot,
        [Parameter(Mandatory = $true)]
        [string]$OutputFile
    )

    $rootPackagePath = Join-Path $RepositoryRoot "package.json"
    if (-not (Test-Path -LiteralPath $rootPackagePath -PathType Leaf)) {
        throw "缺少 package.json：$rootPackagePath"
    }

    $rootPackage = Get-Content -LiteralPath $rootPackagePath -Raw | ConvertFrom-Json
    $packages = Get-PnpmLockPackages -LockfilePath (Join-Path $RepositoryRoot "pnpm-lock.yaml")
    $sbom = [ordered]@{
        bomFormat = "CycloneDX"
        specVersion = "1.5"
        version = 1
        metadata = [ordered]@{
            timestamp = (Get-Date).ToUniversalTime().ToString("o")
            component = [ordered]@{
                type = "application"
                name = $rootPackage.name
                version = if ($rootPackage.PSObject.Properties.Name -contains "version") { $rootPackage.version } else { "0.0.0" }
                purl = "pkg:npm/$($rootPackage.name)"
            }
            properties = @(
                [ordered]@{
                    name = "subforge:sbom:source"
                    value = "pnpm-lock.yaml"
                }
            )
        }
        components = $packages
    }

    $outputParent = Split-Path -Parent $OutputFile
    if ($outputParent -and -not (Test-Path -LiteralPath $outputParent)) {
        New-Item -ItemType Directory -Path $outputParent -Force | Out-Null
    }
    $sbom |
        ConvertTo-Json -Depth 8 |
        Set-Content -LiteralPath $OutputFile -Encoding UTF8
    Write-Host "已生成 Node/pnpm CycloneDX SBOM：$OutputFile，组件数：$($packages.Count)"
}

function Test-WorkflowReferences {
    param(
        [Parameter(Mandatory = $true)]
        [string]$RepositoryRoot
    )

    $releaseCiPath = Join-Path $RepositoryRoot ".github/workflows/release_ci.yml"
    $releasePublishPath = Join-Path $RepositoryRoot ".github/workflows/release_publish.yml"
    $releaseDocPath = Join-Path $RepositoryRoot "docs/deploy/release-artifacts.md"
    foreach ($requiredPath in @($releaseCiPath, $releasePublishPath, $releaseDocPath)) {
        if (-not (Test-Path -LiteralPath $requiredPath -PathType Leaf)) {
            throw "缺少 R14 必需文件：$requiredPath"
        }
    }

    $releaseCi = Get-Content -LiteralPath $releaseCiPath -Raw
    $releasePublish = Get-Content -LiteralPath $releasePublishPath -Raw
    $releaseDoc = Get-Content -LiteralPath $releaseDocPath -Raw

    $expectations = [ordered]@{
        "release-ci attestation permission" = $releaseCi.Contains("attestations: write")
        "release-ci OIDC permission" = $releaseCi.Contains("id-token: write")
        "release-ci provenance action" = $releaseCi.Contains("actions/attest-build-provenance@v2")
        "release-ci metadata job" = $releaseCi.Contains("release-metadata:")
        "release-ci Rust SBOM" = $releaseCi.Contains("cargo-cyclonedx")
        "release-ci Node SBOM" = $releaseCi.Contains("-GenerateNodeSbom")
        "release-ci metadata artifact" = $releaseCi.Contains("subforge-release-metadata")
        "release-publish checks out source script" = $releasePublish.Contains("actions/checkout@v4")
        "release-publish stages assets" = $releasePublish.Contains("-StageUploadAssets")
        "release-publish verifies manifest" = $releasePublish.Contains("-Verify")
        "release doc SHA256SUMS" = $releaseDoc.Contains("SHA256SUMS")
        "release doc attestation verify" = $releaseDoc.Contains("gh attestation verify")
        "release doc dry run" = $releaseDoc.Contains("generate_release_manifest.ps1 -DryRun")
    }

    foreach ($item in $expectations.GetEnumerator()) {
        if (-not $item.Value) {
            throw "R14 workflow/doc 自检失败：$($item.Key)"
        }
    }

    Write-Host "R14 workflow/doc 自检通过。"
}

function Invoke-DryRun {
    $dryRunRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("subforge-release-manifest-" + [System.Guid]::NewGuid().ToString("N"))
    $downloadRoot = Join-Path $dryRunRoot "release-assets"
    $uploadRoot = Join-Path $dryRunRoot "release-assets-upload"
    $publishDownloadRoot = Join-Path $dryRunRoot "release-assets-publish-download"
    $publishUploadRoot = Join-Path $dryRunRoot "release-assets-publish-upload"
    New-Item -ItemType Directory -Path $downloadRoot -Force | Out-Null

    $coreDir = Join-Path $downloadRoot "subforge-core-ubuntu-22.04-x86_64-unknown-linux-musl"
    $nsisDir = Join-Path $downloadRoot "subforge-desktop-windows-10-x86_64-pc-windows-msvc-nsis"
    New-Item -ItemType Directory -Path $coreDir -Force | Out-Null
    New-Item -ItemType Directory -Path $nsisDir -Force | Out-Null
    Set-Content -LiteralPath (Join-Path $coreDir "subforge-core") -Value "dry-run core binary" -Encoding ASCII
    Set-Content -LiteralPath (Join-Path $nsisDir "SubForge_0.0.0_x64-setup.exe") -Value "dry-run nsis bundle" -Encoding ASCII

    Copy-ReleaseArtifactsForUpload -SourceRoot $downloadRoot -DestinationRoot $uploadRoot -MetadataArtifact $MetadataArtifactName | Out-Null
    $rustSbom = [ordered]@{
        bomFormat = "CycloneDX"
        specVersion = "1.5"
        version = 1
        metadata = [ordered]@{
            component = [ordered]@{
                type = "application"
                name = "subforge-core"
            }
        }
        components = @()
    }
    $rustSbom |
        ConvertTo-Json -Depth 8 |
        Set-Content -LiteralPath (Join-Path $uploadRoot "subforge-rust-sbom.cdx.json") -Encoding UTF8
    New-NodeSbomFromPnpmLock `
        -RepositoryRoot (Get-Location).Path `
        -OutputFile (Join-Path $uploadRoot "subforge-node-sbom.cdx.json")

    $script:CommitSha = "dry-run"
    $script:WorkflowRunId = "dry-run"
    $script:Repository = "local/subforge"
    $script:WorkflowName = "release-ci"
    New-ReleaseManifest -InputRoot $uploadRoot -ManifestDirectory $uploadRoot
    Test-ReleaseManifest -Root $uploadRoot -ExpectedCommitSha "dry-run" -ExpectedWorkflowRunId "dry-run"

    New-Item -ItemType Directory -Path $publishDownloadRoot -Force | Out-Null
    Copy-Item -LiteralPath $coreDir -Destination (Join-Path $publishDownloadRoot (Split-Path -Leaf $coreDir)) -Recurse -Force
    Copy-Item -LiteralPath $nsisDir -Destination (Join-Path $publishDownloadRoot (Split-Path -Leaf $nsisDir)) -Recurse -Force
    $metadataDir = Join-Path $publishDownloadRoot $MetadataArtifactName
    New-Item -ItemType Directory -Path $metadataDir -Force | Out-Null
    foreach ($metadataFile in @("SHA256SUMS", "release-manifest.json", "subforge-rust-sbom.cdx.json", "subforge-node-sbom.cdx.json")) {
        Copy-Item -LiteralPath (Join-Path $uploadRoot $metadataFile) -Destination (Join-Path $metadataDir $metadataFile) -Force
    }
    Copy-ReleaseArtifactsForUpload -SourceRoot $publishDownloadRoot -DestinationRoot $publishUploadRoot -MetadataArtifact $MetadataArtifactName | Out-Null
    Test-ReleaseManifest -Root $publishUploadRoot -ExpectedCommitSha "dry-run" -ExpectedWorkflowRunId "dry-run"
    Test-WorkflowReferences -RepositoryRoot (Get-Location).Path
    Write-Host "DryRun 通过。临时输出目录：$uploadRoot"
}

if ($DryRun) {
    Invoke-DryRun
    exit 0
}

$artifactsRootFull = Resolve-AbsolutePath -Path $ArtifactsRoot
$outputDirectoryFull = Resolve-AbsolutePath -Path $OutputDirectory

if ($StageUploadAssets) {
    Copy-ReleaseArtifactsForUpload `
        -SourceRoot $artifactsRootFull `
        -DestinationRoot $outputDirectoryFull `
        -MetadataArtifact $MetadataArtifactName | Out-Null
}

if ($GenerateNodeSbom) {
    New-NodeSbomFromPnpmLock `
        -RepositoryRoot (Get-Location).Path `
        -OutputFile (Join-Path $outputDirectoryFull "subforge-node-sbom.cdx.json")
}

if ($Generate -or (-not $StageUploadAssets -and -not $GenerateNodeSbom -and -not $Verify)) {
    New-ReleaseManifest -InputRoot $artifactsRootFull -ManifestDirectory $outputDirectoryFull
}

if ($Verify) {
    Test-ReleaseManifest `
        -Root $outputDirectoryFull `
        -ExpectedCommitSha $CommitSha `
        -ExpectedWorkflowRunId $WorkflowRunId
}
