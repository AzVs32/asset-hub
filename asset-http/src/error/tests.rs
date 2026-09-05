use super::*;

#[test]
fn caller_errors_and_internal_errors_use_distinct_status_classes() {
    let unsupported = HttpError::from(CoreError::unsupported("resource format", "unknown-format"));
    let invalid_operation = HttpError::from(CoreError::invalid_operation("resource is deleted"));
    let configuration = HttpError::from(CoreError::configuration("executor is missing"));
    let invariant = HttpError::from(CoreError::invariant("adapter returned an invalid snapshot"));

    assert_eq!(unsupported.status, StatusCode::BAD_REQUEST);
    assert_eq!(invalid_operation.status, StatusCode::BAD_REQUEST);
    assert_eq!(configuration.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(invariant.status, StatusCode::INTERNAL_SERVER_ERROR);
}
