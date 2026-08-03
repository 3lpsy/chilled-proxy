use super::*;

#[test]
fn fetch_error_displays() {
    assert_eq!(FetchError::TooLarge.to_string(), "response body too large");
}
