//! TOML 分区路径的校验、读取与回写。

use crate::ConfigError;

pub(crate) fn validate_section_name(section: &str) -> Result<(), ConfigError> {
    let valid = !section.is_empty()
        && section.split('.').all(|segment| {
            !segment.is_empty()
                && segment
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        });
    if valid {
        Ok(())
    } else {
        Err(ConfigError::InvalidSectionName {
            section: section.to_owned(),
        })
    }
}

pub(crate) fn paths_overlap(left: &str, right: &str) -> bool {
    let left = left.split('.').collect::<Vec<_>>();
    let right = right.split('.').collect::<Vec<_>>();
    left.iter().zip(&right).all(|(left, right)| left == right)
        && left.len().min(right.len()) < left.len().max(right.len())
}

pub(crate) fn find_path<'a>(
    document: &'a toml::Table,
    path: &str,
) -> Result<Option<&'a toml::Value>, ConfigError> {
    let segments = path.split('.').collect::<Vec<_>>();
    let Some(mut value) = document.get(segments[0]) else {
        return Ok(None);
    };

    for index in 1..segments.len() {
        let namespace = segments[..index].join(".");
        let table = value
            .as_table()
            .ok_or(ConfigError::InvalidNamespace { namespace })?;
        let Some(next) = table.get(segments[index]) else {
            return Ok(None);
        };
        value = next;
    }
    Ok(Some(value))
}

pub(crate) fn insert_path(document: &mut toml::Table, path: &str, value: toml::Value) {
    let segments = path.split('.').collect::<Vec<_>>();
    let mut table = document;
    for segment in &segments[..segments.len() - 1] {
        let entry = table
            .entry((*segment).to_owned())
            .or_insert_with(|| toml::Value::Table(toml::Table::new()));
        table = entry
            .as_table_mut()
            .expect("loaded configuration namespace must remain a table");
    }
    table.insert(segments[segments.len() - 1].to_owned(), value);
}
