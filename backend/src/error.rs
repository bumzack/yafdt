use axum::{http::StatusCode, response::IntoResponse};
use std::fmt;

#[warn(dead_code)]
#[derive(Debug)]
pub enum YafdError {
    DataError(String),
    SerdeJsonErr(serde_json::error::Error),
}

impl fmt::Display for YafdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            YafdError::DataError(err) => {
                write!(f, "data error. err {}", err)
            }
            YafdError::SerdeJsonErr(err) => write!(f, "serde json error. err: {}", err),
        }
    }
}

impl From<serde_json::Error> for YafdError {
    fn from(error: serde_json::Error) -> Self {
        YafdError::SerdeJsonErr(error)
    }
}

impl From<std::io::Error> for YafdError {
    fn from(error: std::io::Error) -> Self {
        YafdError::DataError(format!("std::io error {:?}", error))
    }
}

impl From<walkdir::Error> for YafdError {
    fn from(error: walkdir::Error) -> Self {
        YafdError::DataError(format!("walkdir error {:?}", error))
    }
}

impl IntoResponse for YafdError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match self {
            YafdError::DataError(msg) => {
                println!("data error to INTERNAL_ERROR. err: {:?}", msg);
                (StatusCode::INTERNAL_SERVER_ERROR, "internal data error")
            }
            YafdError::SerdeJsonErr(error) => {
                println!("serde JSON error to INTERNAL_ERROR. err: {:?}", error);
                (StatusCode::INTERNAL_SERVER_ERROR, "serde JSON error")
            }
        };
        (status, message).into_response()
    }
}
