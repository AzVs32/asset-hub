use serde::{Deserialize, Deserializer, Serialize};

/// Version of the host content functions exposed to Wasm plugins.
pub const CONTENT_ABI_VERSION: u32 = 1;
pub const CONTENT_OPEN_FN: &str = "asset_hub_content_open";
pub const CONTENT_SIZE_FN: &str = "asset_hub_content_size";
pub const CONTENT_READ_RANGE_FN: &str = "asset_hub_content_read";
pub const CONTENT_CLOSE_FN: &str = "asset_hub_content_close";

/// A validated half-open byte range `[offset, offset + length)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PluginContentRange {
    offset: u64,
    length: u64,
}

impl PluginContentRange {
    pub fn new(offset: u64, length: u64) -> Result<Self, ContentRangeError> {
        offset
            .checked_add(length)
            .ok_or(ContentRangeError::Overflow)?;
        Ok(Self { offset, length })
    }

    pub fn end(self) -> u64 {
        self.offset
            .checked_add(self.length)
            .expect("PluginContentRange construction validates its end")
    }

    pub fn offset(self) -> u64 {
        self.offset
    }

    pub fn length(self) -> u64 {
        self.length
    }

    pub fn bounded(self, size: u64, max_length: u64) -> Result<Self, ContentRangeError> {
        if self.offset > size {
            return Err(ContentRangeError::OutOfBounds);
        }
        Self::new(
            self.offset,
            self.length.min(max_length).min(size - self.offset),
        )
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PluginContentRangeDocument {
    offset: u64,
    length: u64,
}

impl<'de> Deserialize<'de> for PluginContentRange {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let document = PluginContentRangeDocument::deserialize(deserializer)?;
        Self::new(document.offset, document.length).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentRangeError {
    Overflow,
    OutOfBounds,
}

impl std::fmt::Display for ContentRangeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Overflow => "content range overflows u64",
            Self::OutOfBounds => "content range starts beyond the content size",
        })
    }
}

impl std::error::Error for ContentRangeError {}

/// Safe guest-side client for the Extism content ABI.
#[cfg(all(feature = "extism-guest", target_arch = "wasm32"))]
pub mod guest {
    use super::PluginContentRange;
    use extism_pdk::{Error, FnResult, host_fn};

    #[host_fn]
    extern "ExtismHost" {
        fn asset_hub_content_open(reference: String) -> String;
        fn asset_hub_content_size(handle: String) -> u64;
        fn asset_hub_content_read(handle: String, offset: u64, length: u64) -> Vec<u8>;
        fn asset_hub_content_close(handle: String);
    }

    pub fn read_all(reference: &str, max_size: u64, chunk_size: u64) -> FnResult<Vec<u8>> {
        if chunk_size == 0 {
            return Err(Error::msg("content ABI chunk size must be greater than zero").into());
        }
        with_content(reference, |handle, size| {
            if size > max_size {
                return Err(Error::msg(format!(
                    "content is {size} bytes, plugin limit is {max_size}"
                ))
                .into());
            }
            read_open_range(handle, size, PluginContentRange::new(0, size)?, chunk_size)
        })
    }

    pub fn read_range(
        reference: &str,
        range: PluginContentRange,
        max_size: u64,
        chunk_size: u64,
    ) -> FnResult<Vec<u8>> {
        if chunk_size == 0 {
            return Err(Error::msg("content ABI chunk size must be greater than zero").into());
        }
        with_content(reference, |handle, size| {
            if size > max_size {
                return Err(Error::msg(format!(
                    "content is {size} bytes, plugin limit is {max_size}"
                ))
                .into());
            }
            if range.end() > size {
                return Err(Error::msg("content ABI range is out of bounds").into());
            }
            read_open_range(handle, size, range, chunk_size)
        })
    }

    fn with_content<T>(
        reference: &str,
        operation: impl FnOnce(&str, u64) -> FnResult<T>,
    ) -> FnResult<T> {
        let handle = unsafe { asset_hub_content_open(reference.to_string()) }?;
        let result = (|| {
            let size = unsafe { asset_hub_content_size(handle.clone()) }?;
            operation(&handle, size)
        })();
        let close = unsafe { asset_hub_content_close(handle) };
        match (result, close) {
            (Ok(value), Ok(())) => Ok(value),
            (Err(error), _) => Err(error),
            (Ok(_), Err(error)) => Err(error.into()),
        }
    }

    fn read_open_range(
        handle: &str,
        _size: u64,
        range: PluginContentRange,
        chunk_size: u64,
    ) -> FnResult<Vec<u8>> {
        let capacity = usize::try_from(range.length())
            .map_err(|_| Error::msg("content ABI range does not fit guest memory"))?;
        let mut bytes = Vec::with_capacity(capacity);
        let mut offset = range.offset();
        while offset < range.end() {
            let requested = (range.end() - offset).min(chunk_size);
            let chunk = unsafe { asset_hub_content_read(handle.to_string(), offset, requested) }?;
            if chunk.is_empty() || chunk.len() as u64 > requested {
                return Err(Error::msg("content ABI host returned an invalid chunk").into());
            }
            offset += chunk.len() as u64;
            bytes.extend_from_slice(&chunk);
        }
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests;
