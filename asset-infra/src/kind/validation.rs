use asset_core::CoreError;
use std::collections::{HashMap, HashSet};

pub(super) fn ensure_unique_id<'a>(
    target: &str,
    id: &str,
    existing: impl IntoIterator<Item = &'a str>,
) -> Result<(), CoreError> {
    if existing.into_iter().any(|candidate| candidate == id) {
        return Err(CoreError::configuration(format!(
            "duplicate {target} `{id}`"
        )));
    }
    Ok(())
}

pub(super) fn ensure_unique_scoped_action<'a>(
    target: &str,
    id: &str,
    scopes: &[String],
    source: &str,
    existing: impl IntoIterator<Item = (&'a str, &'a [String])>,
) -> Result<(), CoreError> {
    if existing.into_iter().any(|(candidate, candidate_scopes)| {
        candidate == id && scopes_overlap(candidate_scopes, scopes)
    }) {
        return Err(CoreError::configuration(format!(
            "duplicate global {target} action `{id}` from {source}"
        )));
    }
    Ok(())
}

fn scopes_overlap(left: &[String], right: &[String]) -> bool {
    left.is_empty()
        || right.is_empty()
        || left
            .iter()
            .any(|value| right.iter().any(|candidate| candidate == value))
}

pub(super) fn validate_hierarchy(
    target: &str,
    nodes: Vec<(&str, Option<&str>)>,
) -> Result<(), CoreError> {
    let parents = nodes.into_iter().collect::<HashMap<_, _>>();
    for &kind in parents.keys() {
        let mut current = Some(kind);
        let mut visited = HashSet::new();
        while let Some(candidate) = current {
            if !visited.insert(candidate) {
                return Err(CoreError::configuration(format!(
                    "{target} kind hierarchy contains a cycle at `{candidate}`"
                )));
            }
            let Some(parent) = parents.get(candidate) else {
                return Err(CoreError::configuration(format!(
                    "{target} kind `{kind}` references unknown parent `{candidate}`"
                )));
            };
            current = *parent;
        }
    }
    Ok(())
}
