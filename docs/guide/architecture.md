# 架构总览

SubForge 采用 **Core + Desktop** 分离架构：

- `subforge-core`：独立守护进程，承载刷新调度、插件运行、聚合转换、HTTP API。
- `subforge-desktop`：可选 GUI，仅用于管理与观察。

## 通信边界

- 管理与数据接口统一通过 Core HTTP API。
- Desktop 的进程生命周期管理走 Tauri IPC。
- `admin_token` 只在 Rust 侧内存中保存，不落入 WebView JS 上下文。

## 导出语义

- 四种订阅读取接口共享同一份最终聚合节点集。
- 如果 Profile 绑定了路由模板来源，`/api/profiles/{id}/clash` 与 `/api/profiles/{id}/sing-box` 会优先保留模板原有分组与规则结构，再把聚合节点追加到输出节点集和可注入分组中。
- `/api/profiles/{id}/base64` 与 `/api/profiles/{id}/raw` 始终只暴露最终聚合节点集，不携带模板分流规则。

## 运行模式

- 桌面模式：Desktop 可随时开关，Core 常驻。
- 无头模式：仅运行 Core + TOML 配置，适合服务器与容器部署。
