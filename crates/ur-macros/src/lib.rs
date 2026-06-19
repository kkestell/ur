//! Procedural macros for `ur`.

#![forbid(unsafe_code)]

use proc_macro::TokenStream;

mod tool;
mod tools;

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

/// Turns an inherent impl block into a value implementing `ur::ToolSet`, where
/// each `#[ur::tool]`-marked `&self` method becomes a tool backed by a clone of
/// the owning state.
///
/// See the `ur` crate documentation for the full `#[ur::tools]` contract.
#[proc_macro_attribute]
pub fn tools(attr: TokenStream, item: TokenStream) -> TokenStream {
    tools::expand(attr.into(), item.into())
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}
