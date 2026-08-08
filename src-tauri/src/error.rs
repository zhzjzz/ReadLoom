use serde::{Serialize, Serializer};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AppErrorDto {
    pub code: &'static str,
    pub message: String,
    pub recoverable: bool,
    pub suggested_action: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{message}")]
    Validation {
        code: &'static str,
        message: String,
        suggested_action: Option<String>,
    },
    #[error("internal operation failed: {context}")]
    Internal {
        code: &'static str,
        context: &'static str,
    },
}

impl AppError {
    pub fn validation(
        code: &'static str,
        message: impl Into<String>,
        suggested_action: impl Into<String>,
    ) -> Self {
        Self::Validation {
            code,
            message: message.into(),
            suggested_action: Some(suggested_action.into()),
        }
    }

    pub fn internal(code: &'static str, context: &'static str) -> Self {
        Self::Internal { code, context }
    }

    pub fn to_dto(&self) -> AppErrorDto {
        match self {
            Self::Validation {
                code,
                message,
                suggested_action,
            } => AppErrorDto {
                code,
                message: message.clone(),
                recoverable: true,
                suggested_action: suggested_action.clone(),
            },
            Self::Internal { code, .. } => AppErrorDto {
                code,
                message: "Readloom 暂时无法完成此操作。".to_owned(),
                recoverable: true,
                suggested_action: Some("请重试；如果问题持续存在，请重新启动应用。".to_owned()),
            },
        }
    }
}

impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.to_dto().serialize(serializer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validation_errors_keep_actionable_user_details() {
        let error = AppError::validation("INPUT_EMPTY", "请输入测试文字。", "输入文字后重试。");

        assert_eq!(
            error.to_dto(),
            AppErrorDto {
                code: "INPUT_EMPTY",
                message: "请输入测试文字。".to_owned(),
                recoverable: true,
                suggested_action: Some("输入文字后重试。".to_owned()),
            }
        );
    }

    #[test]
    fn internal_errors_do_not_expose_diagnostic_context() {
        let error = AppError::internal("METRIC_WRITE_FAILED", "permission denied at C:\\private");
        let serialized = serde_json::to_string(&error).expect("serialize safe error DTO");

        assert!(!serialized.contains("private"));
        assert!(serialized.contains("METRIC_WRITE_FAILED"));
    }
}
