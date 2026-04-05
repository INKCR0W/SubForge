# 插件体系

SubForge 插件遵循统一 `plugin.json + schema.json` 规范。

## 插件类型

- `static`：直接拉取配置 URL
- `script`：通过登录/刷新/抓取脚本获取配置数据

## 通用必填字段（plugin.json）

- `plugin_id`
- `spec_version`（当前仅支持 `1.x`）
- `name`
- `version`
- `type`
- `config_schema`

## 目录结构

```text
static 插件：
my-plugin/
  plugin.json
  schema.json

script 插件：
my-plugin/
  plugin.json
  schema.json
  scripts/
    fetch.lua
    login.lua (可选)
    refresh.lua (可选)
```

script 插件还要求 `entrypoints.fetch` 非空。

## 导入打包注意事项

- 插件 zip 中必须且只能有一个 `plugin.json`
- `plugin.json` 可以在 zip 根目录，也可以在某个子目录中
- 若 zip 中存在多个 `plugin.json`，导入会被拒绝

更多字段说明见下级文档。

## 推荐阅读顺序

1. `plugins/static`：了解固定 URL 来源。
2. `plugins/script`：了解脚本入口契约、Runtime API、运行限制与安全边界。
