mod request;
mod response;

pub use request::{encode_native_responses_request_body, encode_responses_request};
pub use response::decode_responses_response;

#[cfg(test)]
#[path = "responses/tests.rs"]
mod tests;
