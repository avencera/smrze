use std::error::Error;
use std::fmt::{self, Display, Formatter};

pub type Result<T> = std::result::Result<T, BuildSupportError>;

#[derive(Debug, Clone)]
pub struct BuildSupportError(String);

impl BuildSupportError {
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl Display for BuildSupportError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for BuildSupportError {}
