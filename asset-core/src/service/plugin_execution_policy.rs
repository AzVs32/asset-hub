use crate::CoreError;

/// Runtime limits shared by the core action service and the plugin host.
///
/// The application assembly creates one policy from configuration and injects the same value into
/// both layers. This prevents an action from passing the host limit but failing an unrelated limit
/// hard-coded in the core service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginExecutionPolicy {
    max_content_bytes: u64,
    max_inline_content_bytes: u64,
    max_content_read_bytes: u64,
    max_input_bytes: usize,
    max_output_bytes: usize,
    max_concurrent_calls: usize,
    memory_max_pages: u32,
    timeout_seconds: u64,
}

impl PluginExecutionPolicy {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        max_content_bytes: u64,
        max_inline_content_bytes: u64,
        max_content_read_bytes: u64,
        max_input_bytes: usize,
        max_output_bytes: usize,
        max_concurrent_calls: usize,
        memory_max_pages: u32,
        timeout_seconds: u64,
    ) -> Result<Self, CoreError> {
        let policy = Self {
            max_content_bytes,
            max_inline_content_bytes: max_inline_content_bytes.min(max_content_bytes),
            max_content_read_bytes: max_content_read_bytes.min(max_content_bytes),
            max_input_bytes,
            max_output_bytes,
            max_concurrent_calls,
            memory_max_pages,
            timeout_seconds,
        };
        policy.validate()?;
        Ok(policy)
    }

    fn validate(&self) -> Result<(), CoreError> {
        if self.max_content_bytes == 0
            || self.max_inline_content_bytes == 0
            || self.max_content_read_bytes == 0
            || self.max_input_bytes == 0
            || self.max_output_bytes == 0
            || self.max_concurrent_calls == 0
            || self.memory_max_pages == 0
            || self.timeout_seconds == 0
        {
            return Err(CoreError::configuration(
                "plugin execution limits must all be greater than zero",
            ));
        }
        Ok(())
    }

    pub fn max_content_bytes(&self) -> u64 {
        self.max_content_bytes
    }

    pub fn max_inline_content_bytes(&self) -> u64 {
        self.max_inline_content_bytes
    }

    pub fn max_content_read_bytes(&self) -> u64 {
        self.max_content_read_bytes
    }

    pub fn max_input_bytes(&self) -> usize {
        self.max_input_bytes
    }

    pub fn max_output_bytes(&self) -> usize {
        self.max_output_bytes
    }

    pub fn max_concurrent_calls(&self) -> usize {
        self.max_concurrent_calls
    }

    pub fn memory_max_pages(&self) -> u32 {
        self.memory_max_pages
    }

    pub fn timeout_seconds(&self) -> u64 {
        self.timeout_seconds
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_sub_limits_and_rejects_zero_values() {
        let policy = PluginExecutionPolicy::new(8, 9, 10, 16, 16, 1, 1, 1).unwrap();
        assert_eq!(policy.max_inline_content_bytes(), 8);
        assert_eq!(policy.max_content_read_bytes(), 8);
        assert!(PluginExecutionPolicy::new(8, 4, 4, 0, 16, 1, 1, 1).is_err());
    }
}
