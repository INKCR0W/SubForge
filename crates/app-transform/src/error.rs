use thiserror::Error;

pub type TransformResult<T> = Result<T, TransformError>;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum TransformError {
    #[error("节点 `{node_name}` 缺少必填字段 `{field}`")]
    MissingField {
        node_name: String,
        field: &'static str,
    },
    #[error("YAML 序列化失败：{0}")]
    SerializeYaml(String),
    #[error("JSON 序列化失败：{0}")]
    SerializeJson(String),
}

impl TransformError {
    pub fn code(&self) -> &'static str {
        "E_TRANSFORM"
    }
}

impl From<serde_yaml::Error> for TransformError {
    fn from(error: serde_yaml::Error) -> Self {
        Self::SerializeYaml(error.to_string())
    }
}

impl From<serde_json::Error> for TransformError {
    fn from(error: serde_json::Error) -> Self {
        Self::SerializeJson(error.to_string())
    }
}
