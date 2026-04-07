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
