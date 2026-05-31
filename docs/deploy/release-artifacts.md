# 发布产物

## CI 流水线

仓库使用以下 GitHub Actions 流水线完成三平台构建与发布：

- `release-ci`
  - 负责质量门禁、三平台构建、桌面打包、Windows 安装后冒烟验证。
- `release-publish`
  - 监听 `release-ci` 成功结果，将构建产物发布到 GitHub Releases。
  - `main` 分支推送产出 `pre-release`。
  - `v*` 标签推送产出正式 release。

## 产物命名基线

CI artifact 与最终发布资产统一遵循以下命名规则：

```text
{component}-{platform}-{arch-or-target}-{package_kind?}
```

说明：

- `component`
  - `subforge-core`
  - `subforge-desktop`
- `platform`
  - `ubuntu-22.04`
  - `windows-10`
  - `macos-13`
  - 这里使用稳定的对外发布标签，不直接复用 `windows-latest` / `macos-latest` 这类会漂移的 CI runner 名称
- `arch-or-target`
  - 优先使用 Rust target triple，例如 `x86_64-unknown-linux-musl`
- `package_kind`
  - 仅对安装包类产物追加，例如 `nsis`、`dmg`、`deb`、`appimage`

示例：

- `subforge-core-ubuntu-22.04-x86_64-unknown-linux-musl`
- `subforge-core-windows-10-x86_64-pc-windows-msvc`
- `subforge-desktop-macos-13-aarch64-apple-darwin-dmg`
- `subforge-desktop-ubuntu-22.04-x86_64-unknown-linux-gnu-appimage`

## 保留策略

- CI artifact 默认保留 `14` 天，用于回归排查与安装包追溯。
- `release-publish` 会将 `subforge-*` 前缀产物同步到 GitHub Releases。
- 仅用于流水线诊断的日志工件不会进入正式发布资产列表。
- 每次发布会同时上传：
  - `SHA256SUMS`：所有发布资产与 SBOM 的 SHA-256 校验清单。
  - `release-manifest.json`：记录资产名、平台/target、commit SHA、workflow run id、大小与 SHA-256。
  - `subforge-rust-sbom.cdx.json`：`subforge-core` 及其 Rust 依赖的 CycloneDX SBOM；Desktop/Tauri 的 Rust 依赖通过 sidecar 与 Node SBOM 一并追溯。
  - `subforge-node-sbom.cdx.json`：基于 `pnpm-lock.yaml` 生成的 Node/pnpm workspace CycloneDX SBOM。
- `release-ci` 会为二进制、安装包与元数据生成 GitHub Artifact Attestation；`release-publish` 上传前必须先校验 `SHA256SUMS` 与 `release-manifest.json`。

## 完整性与来源验证

下载方建议按“完整性 → manifest → provenance”的顺序验证：

1. 从 GitHub Release 下载目标资产、`SHA256SUMS` 与 `release-manifest.json`。
2. 校验 SHA-256：

   ```bash
   sha256sum -c SHA256SUMS
   ```

   Windows PowerShell 可用：

   ```powershell
   Get-Content .\SHA256SUMS | ForEach-Object {
     if ($_ -match '^([a-fA-F0-9]{64})\s+\*?(.+)$') {
       $actual = (Get-FileHash -Algorithm SHA256 -LiteralPath $Matches[2]).Hash.ToLowerInvariant()
       if ($actual -ne $Matches[1].ToLowerInvariant()) {
         throw "SHA256 校验失败：$($Matches[2])"
       }
     }
   }
   ```

3. 核对 `release-manifest.json`：
   - `commit_sha` 应等于 Release 对应 commit。
   - `workflow_run_id` 应等于发布说明中的 Source run。
   - `artifacts[].sha256` 应与 `SHA256SUMS` 一致。
4. 验证 GitHub Artifact Attestation：

   ```bash
   gh attestation verify ./subforge-core-ubuntu-22.04-x86_64-unknown-linux-musl \
     --repo OWNER/REPO
   ```

   对安装包、`SHA256SUMS`、`release-manifest.json` 与 `*-sbom.cdx.json` 也应执行同类验证。

### 本地 manifest 脚本

仓库提供 `scripts/generate_release_manifest.ps1`，用于 CI 中暂存发布资产、生成 checksum/manifest，以及在发布前复验。

常用命令：

```powershell
# 本地自检脚本行为，不依赖真实构建产物
powershell -ExecutionPolicy Bypass -File scripts/generate_release_manifest.ps1 -DryRun

# 将 actions/download-artifact 下载后的分目录产物展平成最终 release 上传名
powershell -ExecutionPolicy Bypass -File scripts/generate_release_manifest.ps1 `
  -ArtifactsRoot release-assets `
  -OutputDirectory release-assets-upload `
  -StageUploadAssets

# 对展平后的发布目录生成 SHA256SUMS 与 release-manifest.json
powershell -ExecutionPolicy Bypass -File scripts/generate_release_manifest.ps1 `
  -ArtifactsRoot release-assets-upload `
  -OutputDirectory release-assets-upload `
  -Generate

# 发布前复验
powershell -ExecutionPolicy Bypass -File scripts/generate_release_manifest.ps1 `
  -OutputDirectory release-assets-upload `
  -Verify
```

## 当前覆盖范围

- Core 独立二进制
  - Linux GNU
  - Linux musl
  - Windows x64
  - macOS x64 / arm64
- Desktop 构建产物
  - Windows x64
  - macOS x64 / arm64
  - Linux x64
- Desktop 安装包
  - Windows `NSIS`
  - macOS `DMG`
  - Linux `DEB` / `AppImage`
