# 脚本开发

脚本插件适用于“需要登录、动态刷新 token、需要流程编排”的来源。

## 插件目录结构

```text
my-plugin/
  plugin.json
  schema.json
  scripts/
    fetch.lua
    login.lua (可选)
    refresh.lua (可选)
```

最小可导入要求：

- `plugin.json` 必填：`plugin_id`、`spec_version`、`name`、`version`、`type`、`config_schema`
- `type` 必须为 `"script"`
- `entrypoints.fetch` 必填且非空
- `schema.json` 必须可解析且满足受限 JSON Schema 子集

说明：`fetch.lua` 文件存在性在脚本执行阶段校验（导入阶段只校验 `entrypoints.fetch` 字段本身）。

常见导入报错对应关系：

- `plugin.json 解析失败：missing field 'name'`：缺少必填字段（如 `name`、`version`、`config_schema`）
- `script 插件必须提供 entrypoints.fetch`：未声明 `entrypoints.fetch` 或值为空
- `schema 顶层字段不支持：...`：`schema.json` 使用了当前不支持的关键字（如 `oneOf`）

## plugin.json 示例（可直接作为起点）

```json
{
  "plugin_id": "vendor.example.dynamic-sub",
  "spec_version": "1.0",
  "name": "Dynamic Subscription",
  "version": "1.0.0",
  "type": "script",
  "config_schema": "schema.json",
  "entrypoints": {
    "login": "scripts/login.lua",
    "refresh": "scripts/refresh.lua",
    "fetch": "scripts/fetch.lua"
  },
  "capabilities": ["http", "cookie", "json", "html", "base64", "secret", "log", "time"],
  "secret_fields": ["password"],
  "network_profile": "browser_chrome"
}
```

## manifest 字段规则（与当前代码一致）

必填字段：

- `plugin_id`：不能为空，且不能包含 `..`、`/`、`\`
- `spec_version`：仅支持 `1.x`
- `name`：不能为空
- `version`：不能为空
- `type`：`"script"`
- `config_schema`：不能为空，且路径必须位于插件目录内

脚本类型额外要求：

- `entrypoints.fetch` 必填且非空
- `entrypoints.login` / `entrypoints.refresh` 可选

可选字段与默认值：

- `secret_fields`：默认 `[]`，每个字段必须出现在 `schema.properties`
- `entrypoints`：默认空对象
- `capabilities`：默认 `[]`，若填写仅允许：
  `http / cookie / json / html / base64 / secret / log / time`
  - 运行时 API 按该字段按需注入；未声明的能力不会注册到 Lua 全局。
  - 例如脚本调用 `http.request` 但未声明 `http`，会报：`attempt to index a nil value (global 'http')`。
- `network_profile`：默认 `standard`，可选：
  `standard / browser_chrome / browser_firefox / webview_assisted`
- `anti_bot_level`：默认 `low`

## 入口函数契约

- `login(ctx, config, state) -> { ok, state?, error? }`
- `refresh(ctx, config, state) -> { ok, state?, error? }`
- `fetch(ctx, config, state) -> { ok, subscription?, state?, error? }`

`subscription` 支持两种返回：
- `{ url = "https://..." }`
- `{ content = "base64 or uri lines text" }`

当返回 `subscription.url` 时，支持可选扩展字段：
- `headers`：对象（`string -> string`），用于二次拉取订阅 URL 的附加请求头
- `user_agent`：字符串，用于二次拉取订阅 URL 的 `User-Agent`

二次拉取默认行为：
- 若未指定 `headers/user_agent`，Core 使用 `standard` 精简请求头拉取（避免浏览器模板头导致返回网页）
- 若指定 `headers/user_agent`，Core 会按脚本返回值覆盖默认拉取请求头

推荐约定：
- 失败统一 `ok = false` 并返回结构化 `error`。
- 非敏感上下文写入 `state`，敏感值写入 `secret` API。

## schema.json 约束（导入时校验）

顶层仅支持这些 key：

- `$schema`
- `type`
- `required`
- `properties`
- `additionalProperties`

`properties.<field>` 仅支持：

- `type`、`title`、`description`、`default`、`enum`
- `format`、`minLength`、`maxLength`、`minimum`、`maximum`、`pattern`
- `x-ui`（仅支持 `widget`、`placeholder`、`help`、`group`、`order`）

额外限制：

- `schema.type` 必须为 `object`
- `schema.properties` 不能为空
- 字段类型仅支持 `string / number / integer / boolean`
- `required` 中的字段必须在 `properties` 中定义

## Runtime API 白名单

- `http.request({ method, url, headers, body, timeout_ms })`
- `cookie.get(name)` / `cookie.set(name, value, attrs)`
- `json.parse(str)` / `json.stringify(obj)`
- `html.query(html, selector)`
- `base64.encode(str)` / `base64.decode(str)`
- `secret.get(key)` / `secret.set(key, value)`
- `log.info(msg)` / `log.warn(msg)` / `log.error(msg)`
- `time.now()`

`http.request` 额外约定：
- `method` 可省略，默认 `GET`。
- 返回状态码非 2xx 时会直接抛出运行时错误（不会返回 `resp.status` 让脚本自行判断）。

不允许：
- 系统命令
- 文件系统访问
- 任意 socket
- 动态模块加载

## 运行限制（MVP）

- 单次脚本执行超时：`20s`
- 单次 `http.request` 超时：`15s`
- 单次执行最大 HTTP 请求数：`20`
- 单次执行内存上限：`64MB`
- 单次 HTTP 响应体上限：`5MB`

常见错误码：
- `E_SCRIPT_TIMEOUT`
- `E_SCRIPT_LIMIT`
- `E_SCRIPT_RUNTIME`

## 常见运行时报错排查

- 报错：`attempt to index a nil value (global 'http')`
  - 原因：`plugin.json` 未在 `capabilities` 中声明 `http`。
  - 修复：在 `capabilities` 增加 `"http"`，并确保入口脚本实际使用到的 API（如 `json/base64/secret/log/time`）也一并声明。
- 报错：`attempt to index a nil value (global 'json'/'secret'/...)`
  - 原因：对应 capability 未声明。
  - 修复：把缺失能力加入 `capabilities`，名称需与白名单完全一致。

## SSRF 与安全边界

`http.request` 会在 DNS 解析后做 IP 校验，命中内网/回环网段会直接拒绝。

示例（会被拒绝）：

```lua
http.request({ method = "GET", url = "http://127.0.0.1:18118/health" })
```

此外，插件只能访问自身 `plugin_id` 的 secret 命名空间，不能读取其他插件或系统密钥。

## 示例：最小 fetch.lua

```lua
function fetch(ctx, config, state)
  local ok, resp = pcall(http.request, {
    method = "GET",
    url = config.subscription_url,
    timeout_ms = 10000
  })

  if not ok then
    return { ok = false, error = { code = "E_HTTP", message = tostring(resp) } }
  end

  return {
    ok = true,
    subscription = { content = resp.body },
    state = state
  }
end
```

## 示例：返回 URL 并自定义二次拉取请求头

```lua
function fetch(ctx, config, state)
  return {
    ok = true,
    subscription = {
      url = config.subscription_url,
      headers = {
        ["accept"] = "text/plain",
        ["x-sub-token"] = config.sub_token
      },
      user_agent = "clash.meta"
    },
    state = state
  }
end
```

## 调试建议

- 先在 mock 服务上验证 `login -> refresh -> fetch`。
- `log.*` 只记录必要上下文，避免输出敏感字段。
- 对 429/403 场景设计可恢复重试，不要写死无限循环。
