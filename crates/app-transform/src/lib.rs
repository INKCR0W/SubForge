//! app-transform：订阅输出格式转换（clash/sing-box/base64/raw）。

mod base64;
mod clash;
mod error;
mod non_uri_text;
mod shared;
mod singbox;
mod stash;
mod template_context;
mod transformer;
mod v2ray_uri;

#[cfg(test)]
mod tests;

pub use base64::{Base64Transformer, build_share_uri};
pub use clash::ClashTransformer;
pub use error::{TransformError, TransformResult};
pub use non_uri_text::NonUriTextTransformer;
pub use singbox::SingboxTransformer;
pub use stash::StashTransformer;
pub use template_context::RoutingTemplateExportContext;
pub use transformer::Transformer;
pub use v2ray_uri::V2RayUriTransformer;
