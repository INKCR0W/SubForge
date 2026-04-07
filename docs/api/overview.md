# HTTP API 总览

## 健康检查

- `GET /health`（无需鉴权）

## 管理 API（admin_token）

- `GET /api/system/status`
- `PUT /api/system/settings`
- `GET /api/plugins`
- `POST /api/plugins/import`
- `DELETE /api/plugins/{id}`
- `PUT /api/plugins/{id}/toggle`
- `GET /api/plugins/{id}/schema`
- `GET /api/sources`
- `POST /api/sources`
- `PUT /api/sources/{id}`
- `DELETE /api/sources/{id}`
- `POST /api/sources/{id}/refresh`
- `GET /api/profiles`
- `POST /api/profiles`
- `PUT /api/profiles/{id}`
- `DELETE /api/profiles/{id}`
- `POST /api/profiles/{id}/refresh`
- `POST /api/tokens/{profile_id}/rotate`
- `GET /api/events`（SSE）

## 配置读取 API（export_token）

- `GET /api/profiles/{id}/clash?token=...`
- `GET /api/profiles/{id}/sing-box?token=...`
- `GET /api/profiles/{id}/base64?token=...`
- `GET /api/profiles/{id}/raw?token=...`

说明：
- 四个端点共享同一份最终聚合节点集。
- 如果 Profile 绑定了路由模板来源，`/clash` 与 `/sing-box` 会保留模板分组/规则语义，并把聚合节点追加到输出节点集与可注入分组中。
- `/base64` 与 `/raw` 仅输出最终聚合节点集，不包含模板规则。

## 统一错误结构

```json
{
  "code": "E_AUTH",
  "message": "...",
  "retryable": false
}
```
