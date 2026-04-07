# Changelog

All notable changes to this project will be documented in this file.

The format is based on Keep a Changelog, and this project follows Semantic Versioning.

## [Unreleased]

### Changed

- Project license switched to `GPL-3.0-only`.
- 发布流水线产物命名切换为稳定平台标签，并统一对齐 `14` 天 CI artifact 保留基线。
- 工作流与仓库包管理声明统一对齐到 `pnpm 10.32.0`。

## [0.1.0] - 2026-04-04

### Added

- Core + Desktop 分离架构与基础运行链路。
- 插件系统（`static` / `script`）与配置 schema 校验。
- 订阅获取、解析、聚合去重、缓存与四种导出格式（Clash / sing-box / Base64 / Raw）。
- 管理 API、SSE 事件流、token 轮换与鉴权中间件。
- Lua 沙箱执行环境与脚本能力白名单。
- SecretStore 多后端实现（memory / keyring / env / file）与命名空间隔离。
- 无头部署能力：配置文件驱动、Docker 非 root 运行。
- 文档站点与 `docs-site-sync` 自动发布工作流。

### Changed

- 发布流水线扩展为三平台构建矩阵，补齐 Core 与 Desktop 工件命名规范。
- 增加 Windows NSIS 安装后 Core 自动拉起冒烟校验，并补充失败诊断日志上传。

### Security

- 固化 Host header 校验、CORS 拒绝策略与敏感日志脱敏基线。
- 强化插件上传与脚本运行限制（超时、内存、请求数、SSRF 防护）。
