#[allow(clippy::module_inception)]
mod directory;
mod kind;
mod path;

use crate::error::DirectoryError;

pub use directory::{Directory, DirectoryId, DirectoryRef, DirectorySnapshot};
pub use kind::DirectoryKind;
pub use path::{DirectoryPath, INTERNAL_STORAGE_DIRECTORY_NAME};

const MAX_DIRECTORY_SEGMENT_LEN: usize = 255;

fn validate_required_text_exact(
    field: &'static str,
    value: &str,
    max: usize,
) -> Result<String, DirectoryError> {
    if value.trim().is_empty() {
        return Err(DirectoryError::Blank { field });
    }
    if value.chars().count() > max {
        return Err(DirectoryError::TooLong { field, max });
    }
    if value.chars().any(char::is_control) {
        return Err(DirectoryError::InvalidFormat {
            field,
            reason: "control characters are not allowed",
        });
    }
    Ok(value.to_owned())
}

#[cfg(test)]
mod tests;
