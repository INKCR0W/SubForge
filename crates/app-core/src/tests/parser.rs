use super::*;

#[test]
fn uri_list_parser_supports_base64_and_skips_invalid_lines() {
    let parser = UriListParser;
    let nodes = parser
        .parse("source-fixture", BASE64_SUBSCRIPTION_FIXTURE)
        .expect("解析 fixture 应成功");

    assert_eq!(nodes.len(), 3);
    let protocols = nodes
        .iter()
        .map(|node| node.protocol.clone())
        .collect::<HashSet<_>>();
    assert!(protocols.contains(&ProxyProtocol::Ss));
    assert!(protocols.contains(&ProxyProtocol::Vmess));
    assert!(protocols.contains(&ProxyProtocol::Trojan));
}

#[test]
fn uri_list_parser_handles_invalid_protocol_lines_without_failing() {
    let parser = UriListParser;
    let payload = "not-uri\nvmess://invalid\nss://invalid\nvless://missing-port";
    let nodes = parser
        .parse("source-invalid", payload)
        .expect("解析过程应不中断");

    assert!(nodes.is_empty());
}
