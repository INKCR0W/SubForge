use super::*;

#[test]
fn request_log_redacts_sensitive_headers() {
    let mut headers = ReqwestHeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_static("Bearer sensitive-token"),
    );
    headers.insert(COOKIE, HeaderValue::from_static("sid=secret-cookie"));
    headers.insert(ACCEPT, HeaderValue::from_static("text/plain"));

    let redacted = redact_headers_for_log(&headers);
    assert!(redacted.contains("authorization=***"));
    assert!(redacted.contains("cookie=***"));
    assert!(redacted.contains("accept=text/plain"));
    assert!(!redacted.contains("sensitive-token"));
    assert!(!redacted.contains("secret-cookie"));
}

#[test]
fn request_log_redacts_sensitive_query_parameters() {
    let original =
        Url::parse("https://example.com/subscription?token=abc123&password=pwd&region=sg")
            .expect("构建测试 URL 失败");
    let redacted = redact_url_for_log(&original);
    let parsed = Url::parse(&redacted).expect("脱敏后的 URL 应可解析");
    let query = parsed
        .query_pairs()
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect::<BTreeMap<_, _>>();

    assert_eq!(query.get("token"), Some(&"***".to_string()));
    assert_eq!(query.get("password"), Some(&"***".to_string()));
    assert_eq!(query.get("region"), Some(&"sg".to_string()));
    assert!(!redacted.contains("abc123"));
    assert!(!redacted.contains("pwd"));
}
