//! HTTP plumbing shared across registries: response builders, capped upstream
//! fetch, and HTTP-date handling.

pub mod conditional;
pub mod fetch;
pub mod httpdate;
pub mod response;

pub use conditional::{conditional_get, ConditionalResponse};
pub use fetch::{read_capped, FetchError};
pub use httpdate::{fmt_http_date, parse_http_date};
pub use response::{
    data_response, error_response, json_escape, json_response, method_not_allowed, text_response,
};
