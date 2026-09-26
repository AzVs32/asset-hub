/// A driver-specific path whose syntax is interpreted by the selected driver.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriverPath(String);

impl DriverPath {
    /// Creates an opaque driver path without normalizing or validating its syntax.
    pub fn new(raw: impl Into<String>) -> Self {
        Self(raw.into())
    }

    /// Returns the path value exactly as supplied.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests;
