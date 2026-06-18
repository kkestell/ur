//! Procedural macros for `ur`.

#![forbid(unsafe_code)]

use proc_macro::TokenStream;

mod tool;

/// Turns an `async` or sync function into a value implementing `ur::Tool`,
/// bound to the same identifier as the function.
///
/// See the `ur` crate documentation for the full `#[ur::tool]` contract.
#[proc_macro_attribute]
pub fn tool(attr: TokenStream, item: TokenStream) -> TokenStream {
    tool::expand(attr.into(), item.into())
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}
