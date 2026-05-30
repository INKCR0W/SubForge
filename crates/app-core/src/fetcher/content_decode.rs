use brotli::Decompressor;
use flate2::read::{DeflateDecoder, GzDecoder, ZlibDecoder};
use std::io::Read;

const DECODED_BODY_LIMIT: usize = super::MAX_SUBSCRIPTION_BYTES;
const DECODED_BODY_LIMIT_PLUS_ONE: u64 = (DECODED_BODY_LIMIT as u64) + 1;

pub(super) fn decode_response_body(
    raw: Vec<u8>,
    content_encoding: Option<&str>,
) -> Result<Vec<u8>, String> {
    let Some(content_encoding) = content_encoding
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(raw);
    };

    let mut encodings = content_encoding
        .split(',')
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty() && value != "identity")
        .collect::<Vec<_>>();
    if encodings.is_empty() {
        return Ok(raw);
    }

    let mut payload = raw;
    while let Some(encoding) = encodings.pop() {
        payload = match encoding.as_str() {
            "br" => decode_brotli(&payload)?,
            "gzip" | "x-gzip" => decode_gzip(&payload)?,
            "deflate" => decode_deflate(&payload)?,
            _ => payload,
        };
    }
    Ok(payload)
}

fn decode_brotli(payload: &[u8]) -> Result<Vec<u8>, String> {
    let mut decoder = Decompressor::new(payload, 4096);
    read_bounded_decode(&mut decoder, "br")
}

fn decode_gzip(payload: &[u8]) -> Result<Vec<u8>, String> {
    let mut decoder = GzDecoder::new(payload);
    read_bounded_decode(&mut decoder, "gzip")
}

fn decode_deflate(payload: &[u8]) -> Result<Vec<u8>, String> {
    let mut zlib_decoder = ZlibDecoder::new(payload);
    match read_bounded_decode(&mut zlib_decoder, "deflate") {
        Ok(decoded) => Ok(decoded),
        Err(zlib_error) if is_decoded_body_too_large(&zlib_error) => Err(zlib_error),
        Err(_) => {
            let mut raw_decoder = DeflateDecoder::new(payload);
            read_bounded_decode(&mut raw_decoder, "deflate")
        }
    }
}

fn read_bounded_decode<R: Read>(decoder: &mut R, label: &str) -> Result<Vec<u8>, String> {
    let mut decoded = Vec::with_capacity(DECODED_BODY_LIMIT.min(8192));
    decoder
        .take(DECODED_BODY_LIMIT_PLUS_ONE)
        .read_to_end(&mut decoded)
        .map_err(|error| format!("{label} 解压失败：{error}"))?;
    if decoded.len() > DECODED_BODY_LIMIT {
        return Err(response_body_too_large_message(decoded.len()));
    }
    Ok(decoded)
}

fn response_body_too_large_message(actual_bytes: usize) -> String {
    format!("上游响应体过大：{actual_bytes} bytes（限制 {DECODED_BODY_LIMIT} bytes）")
}

fn is_decoded_body_too_large(error: &str) -> bool {
    error.starts_with("上游响应体过大：")
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::Compression;
    use flate2::write::{GzEncoder, ZlibEncoder};
    use std::io::Write;

    #[test]
    fn gzip_decode_rejects_output_over_subscription_limit() {
        let compressed = gzip_encode(&vec![b'a'; super::super::MAX_SUBSCRIPTION_BYTES + 1]);

        let error = expect_decode_limit_error(compressed, "gzip");

        assert!(error.contains("上游响应体过大"));
    }

    #[test]
    fn brotli_decode_rejects_output_over_subscription_limit() {
        let compressed = brotli_encode(&vec![b'a'; super::super::MAX_SUBSCRIPTION_BYTES + 1]);

        let error = expect_decode_limit_error(compressed, "br");

        assert!(error.contains("上游响应体过大"));
    }

    #[test]
    fn deflate_decode_rejects_output_over_subscription_limit() {
        let compressed = deflate_encode(&vec![b'a'; super::super::MAX_SUBSCRIPTION_BYTES + 1]);

        let error = expect_decode_limit_error(compressed, "deflate");

        assert!(error.contains("上游响应体过大"));
    }

    fn expect_decode_limit_error(compressed: Vec<u8>, encoding: &str) -> String {
        match decode_response_body(compressed, Some(encoding)) {
            Ok(decoded) => panic!(
                "{encoding} 解压后超过订阅限制必须失败，实际解出 {} bytes",
                decoded.len()
            ),
            Err(error) => error,
        }
    }

    fn gzip_encode(payload: &[u8]) -> Vec<u8> {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(payload).expect("写入 gzip 压缩流失败");
        encoder.finish().expect("完成 gzip 压缩失败")
    }

    fn brotli_encode(payload: &[u8]) -> Vec<u8> {
        let mut output = Vec::new();
        {
            let mut encoder = brotli::CompressorWriter::new(&mut output, 4096, 5, 22);
            encoder.write_all(payload).expect("写入 br 压缩流失败");
        }
        output
    }

    fn deflate_encode(payload: &[u8]) -> Vec<u8> {
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(payload).expect("写入 deflate 压缩流失败");
        encoder.finish().expect("完成 deflate 压缩失败")
    }
}
