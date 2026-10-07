use std::error::Error;

use utils::ParseIdError;

#[test]
fn parse_error_has_a_standard_contract_without_a_backend_cause() {
    fn assert_error<T: Error + Send + Sync + 'static>() {}
    assert_error::<ParseIdError>();

    let error: &dyn Error = &ParseIdError;
    assert!(!error.to_string().is_empty());
    assert!(error.source().is_none());
}
