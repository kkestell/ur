//! Procedural macros for `ur`.

#![forbid(unsafe_code)]

use proc_macro::TokenStream;

/// Placeholder `#[ur::tool]` attribute macro.
#[proc_macro_attribute]
pub fn tool(_attr: TokenStream, item: TokenStream) -> TokenStream {
    item
}
