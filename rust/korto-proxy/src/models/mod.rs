//! Provider request/response structs for cache keying and middleware.

pub mod anthropic;
pub mod openai;

pub use anthropic::MessagesRequest;
pub use openai::ChatCompletionRequest;
