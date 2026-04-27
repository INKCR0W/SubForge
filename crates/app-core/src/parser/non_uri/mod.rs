use app_common::ProxyNode;

use crate::CoreResult;

mod clash;
mod helpers;
mod ini;

pub(crate) fn parse_non_uri_payload(
    source_id: &str,
    payload: &str,
    updated_at: &str,
) -> CoreResult<Vec<ProxyNode>> {
    if payload.trim().is_empty() {
        return Ok(Vec::new());
    }

    if let Some(nodes) = clash::try_parse_clash_like_yaml(source_id, payload, updated_at)? {
        return Ok(nodes);
    }

    if let Some(nodes) = clash::try_parse_clash_like_json(source_id, payload, updated_at)? {
        return Ok(nodes);
    }

    Ok(ini::parse_ini_like_lines(source_id, payload, updated_at))
}
