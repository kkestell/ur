//! DeepSeek provider placeholder for `ur`.

#![forbid(unsafe_code)]

/// Placeholder DeepSeek client handle.
#[derive(Clone, Debug, Default)]
pub struct DeepSeekClient;

impl ur_core::provider::Provider for DeepSeekClient {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_is_a_provider() {
        fn assert_provider<P: ur_core::provider::Provider>() {}

        assert_provider::<DeepSeekClient>();
    }
}
