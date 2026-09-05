use asset_core::CoreError;
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
