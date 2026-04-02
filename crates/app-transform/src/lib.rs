//! app-transform：订阅输出格式转换（clash/sing-box/base64/raw）。

mod base64;
mod clash;
mod error;
mod shared;
mod singbox;
mod transformer;

#[cfg(test)]
mod tests;

pub use base64::Base64Transformer;
pub use clash::ClashTransformer;
pub use error::{TransformError, TransformResult};
pub use singbox::SingboxTransformer;
pub use transformer::Transformer;
