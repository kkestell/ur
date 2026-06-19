//! Implementation of the `#[ur::tools]` attribute macro.

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{Attribute, ImplItem, ImplItemFn, ItemImpl, Meta, Type, parse2};

use crate::tool;

/// Expands `#[ur::tools]` on an inherent impl block into a `ToolSet` impl whose
/// `into_tools` yields one tool per `#[ur::tool]`-marked method.
pub fn expand(attr: TokenStream, item: TokenStream) -> syn::Result<TokenStream> {
    if !attr.is_empty() {
        return Err(syn::Error::new_spanned(
            attr,
            "`#[ur::tools]` does not take arguments; configure each tool with \
             `#[ur::tool(...)]` on its method",
        ));
    }

    let mut item_impl: ItemImpl = parse2(item)?;
    validate_impl(&item_impl)?;
    let self_ty = (*item_impl.self_ty).clone();

    let mut methods = Vec::new();
    for impl_item in &mut item_impl.items {
        if let ImplItem::Fn(method) = impl_item
            && let Some(marker) = take_tool_attr(method)?
        {
            methods.push((&*method, marker));
        }
    }

    // The last tool can move `self` in; only the earlier ones need a clone.
    let last = methods.len().saturating_sub(1);
    let mut registrations = Vec::with_capacity(methods.len());
    let mut seen_names: Vec<String> = Vec::new();
    for (i, (method, marker)) in methods.into_iter().enumerate() {
        let state = if i == last {
            quote! { self }
        } else {
            quote! { ::std::clone::Clone::clone(&self) }
        };
        let generated = generate_tool(&self_ty, method, marker, state)?;
        // Reject colliding wire names at expansion time rather than letting the
        // duplicate surface as a stream error at `send()`. `#[cfg]`-gated methods
        // are exempt: complementary cfgs legitimately reuse a single name.
        if !generated.cfg_gated {
            if seen_names.contains(&generated.name) {
                return Err(syn::Error::new_spanned(
                    &method.sig.ident,
                    format!(
                        "duplicate tool name `{}`; each `#[ur::tool]` in an \
                         `#[ur::tools]` block must resolve to a unique name",
                        generated.name
                    ),
                ));
            }
            seen_names.push(generated.name);
        }
        registrations.push(generated.tokens);
    }

    Ok(quote! {
        #item_impl

        // Localize the diagnostic when the state type is not `Clone`: each tool
        // clones the receiver to drive an owned `'static` future, so without this
        // assertion the failure surfaces only inside macro-generated `Clone::clone`
        // calls. Spanning the bound at the user's `self` type points the error there.
        const _: () = {
            fn __ur_assert_clone_state<__T: ::std::clone::Clone>() {}
            let _ = __ur_assert_clone_state::<#self_ty>;
        };

        impl ::ur::ToolSet for #self_ty {
            fn into_tools(self) -> ::std::vec::Vec<::std::sync::Arc<dyn ::ur::Tool>> {
                let mut __ur_tools: ::std::vec::Vec<::std::sync::Arc<dyn ::ur::Tool>> =
                    ::std::vec::Vec::new();
                #(#registrations)*
                __ur_tools
            }
        }
    })
}

/// Rejects impl blocks the macro cannot turn into a `ToolSet`.
fn validate_impl(item: &ItemImpl) -> syn::Result<()> {
    if let Some((_, path, _)) = &item.trait_ {
        return Err(syn::Error::new_spanned(
            path,
            "`#[ur::tools]` must be applied to an inherent impl block, not a trait impl",
        ));
    }
    if !item.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &item.generics,
            "`#[ur::tools]` does not support generic impl blocks",
        ));
    }
    if let Some(w) = &item.generics.where_clause {
        return Err(syn::Error::new_spanned(
            w,
            "`#[ur::tools]` does not support where clauses",
        ));
    }
    Ok(())
}

/// Removes the `#[ur::tool]` marker from a method, returning its argument tokens,
/// or `None` when the method is unmarked.
fn take_tool_attr(method: &mut ImplItemFn) -> syn::Result<Option<TokenStream>> {
    let Some(index) = method.attrs.iter().position(is_tool_marker) else {
        return Ok(None);
    };
    if let Some(extra) = method.attrs[index + 1..].iter().find(|a| is_tool_marker(a)) {
        return Err(syn::Error::new_spanned(
            extra,
            "a method may carry only one `#[ur::tool]` attribute",
        ));
    }
    let tokens = match method.attrs.remove(index).meta {
        Meta::Path(_) => TokenStream::new(),
        Meta::List(list) => list.tokens,
        Meta::NameValue(nv) => {
            return Err(syn::Error::new_spanned(
                nv,
                "`#[ur::tool]` does not take a `= value` form",
            ));
        }
    };
    Ok(Some(tokens))
}

/// Returns whether an attribute is the fully-qualified `#[ur::tool]` marker.
///
/// Only the two-segment `ur::tool` path is recognized; a bare `#[tool]` is left
/// untouched so the macro never silently claims an unrelated attribute of the
/// same name. Inside an `#[ur::tools]` block, mark each tool with `#[ur::tool]`.
fn is_tool_marker(attr: &Attribute) -> bool {
    let segments: Vec<&syn::PathSegment> = attr.path().segments.iter().collect();
    matches!(
        segments.as_slice(),
        [first, seg] if first.ident == "ur" && seg.ident == "tool",
    )
}

/// One expanded tool: its resolved wire name, whether the source method is
/// `#[cfg]`-gated, and the tokens that declare the tool and push it onto the
/// accumulator.
struct GeneratedTool {
    name: String,
    cfg_gated: bool,
    tokens: TokenStream,
}

/// Generates one tool's local type, parameter struct, `Tool` impl, and the line
/// that pushes it onto the accumulator, gated by the method's `#[cfg]` attrs.
fn generate_tool(
    self_ty: &Type,
    method: &ImplItemFn,
    marker: TokenStream,
    state: TokenStream,
) -> syn::Result<GeneratedTool> {
    tool::validate_signature(&method.sig)?;
    let params = tool::parse_method_params(&method.sig)?;
    let param_names: Vec<String> = params.iter().map(|p| p.ident.to_string()).collect();
    let config = tool::parse_config(marker, &param_names)?;

    let method_ident = &method.sig.ident;
    let tool_name = config
        .name
        .clone()
        .unwrap_or_else(|| method_ident.to_string());
    let tool_struct = format_ident!("__UrTool_{}", method_ident);

    let cfg_attrs: Vec<&Attribute> = method
        .attrs
        .iter()
        .filter(|a| tool::is_cfg_attr(a))
        .collect();

    let fields = tool::params_fields(&params, &config.param_docs);
    let deserialize = tool::deserialize_block(params.is_empty());
    let finish = tool::finish_block(tool::returns_result(&method.sig.output));
    let schema = tool::schema_body(&tool_name, &config.description);

    let invoke = tool::invoke_expr(
        quote! { __state.#method_ident },
        &params,
        method.sig.asyncness.is_some(),
    );
    let prelude = quote! { let __state = ::std::clone::Clone::clone(&self.0); };
    let tool_impl = tool::emit_tool_impl(
        &tool_struct,
        &tool_name,
        &fields,
        &schema,
        &deserialize,
        &finish,
        &prelude,
        &invoke,
    );

    let cfg_gated = !cfg_attrs.is_empty();
    let tokens = quote! {
        #(#cfg_attrs)*
        {
            #[allow(non_camel_case_types)]
            struct #tool_struct(#self_ty);

            #tool_impl

            __ur_tools.push(
                ::std::sync::Arc::new(#tool_struct(#state))
                    as ::std::sync::Arc<dyn ::ur::Tool>
            );
        }
    };

    Ok(GeneratedTool {
        name: tool_name,
        cfg_gated,
        tokens,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts(src: &str) -> TokenStream {
        src.parse().expect("token stream")
    }

    fn expand_str(attr: &str, item: &str) -> syn::Result<String> {
        super::expand(ts(attr), ts(item)).map(|t| t.to_string())
    }

    #[test]
    fn generates_tool_set_impl_and_registrations() {
        let out = expand_str(
            "",
            r#"
            impl Tools {
                #[ur::tool(description = "Look up a user by id.")]
                async fn get_user(&self, id: u64) -> Result<User, String> {
                    self.db.fetch(id).await.map_err(|e| e.to_string())
                }

                fn cache_key(&self, id: u64) -> String { String::new() }
            }
            "#,
        )
        .unwrap();

        assert!(out.contains("impl :: ur :: ToolSet for Tools"));
        assert!(out.contains("\"get_user\""));
        assert!(out.contains("__state . get_user"));
        // The `&self` receiver is excluded from the params struct.
        assert!(out.contains("struct __UrParams { id : u64 }"));
        // The unmarked method survives untouched on the re-emitted impl block.
        assert!(out.contains("fn cache_key"));
        // A `Clone` bound is asserted against the state type so a non-`Clone`
        // owner fails at the type rather than inside generated `Clone::clone`s.
        assert!(out.contains("__ur_assert_clone_state :: < Tools >"));
    }

    #[test]
    fn sync_method_drops_the_await() {
        let out = expand_str(
            "",
            r#"
            impl Tools {
                #[ur::tool]
                fn key(&self, id: u64) -> String { String::new() }
            }
            "#,
        )
        .unwrap();
        assert!(out.contains("__state . key (__ur_args . id)"));
        assert!(!out.contains(". await"));
    }

    #[test]
    fn zero_argument_method_skips_parsing() {
        let out = expand_str(
            "",
            r#"
            impl Tools {
                #[ur::tool]
                async fn ping(&self) -> i64 { 1 }
            }
            "#,
        )
        .unwrap();
        assert!(out.contains("struct __UrParams"));
        assert!(!out.contains("args . parse"));
    }

    #[test]
    fn bare_tool_attribute_is_not_claimed() {
        let out = expand_str(
            "",
            r#"
            impl Tools {
                #[tool]
                async fn ping(&self) -> i64 { 1 }
            }
            "#,
        )
        .unwrap();
        // A bare `#[tool]` is left on the method and never turned into a tool.
        assert!(!out.contains("__state . ping"));
        assert!(out.contains("# [tool]"));
    }

    #[test]
    fn rejects_duplicate_tool_names() {
        let err = expand_str(
            "",
            r#"
            impl Tools {
                #[ur::tool(name = "x")]
                async fn a(&self) -> i64 { 1 }

                #[ur::tool(name = "x")]
                async fn b(&self) -> i64 { 2 }
            }
            "#,
        )
        .unwrap_err();
        assert!(err.to_string().contains("duplicate tool name `x`"));
    }

    #[test]
    fn rejects_method_name_colliding_with_renamed_tool() {
        let err = expand_str(
            "",
            r#"
            impl Tools {
                #[ur::tool]
                async fn ping(&self) -> i64 { 1 }

                #[ur::tool(name = "ping")]
                async fn pong(&self) -> i64 { 2 }
            }
            "#,
        )
        .unwrap_err();
        assert!(err.to_string().contains("duplicate tool name `ping`"));
    }

    #[test]
    fn allows_cfg_gated_duplicate_names() {
        let out = expand_str(
            "",
            r#"
            impl Tools {
                #[cfg(feature = "a")]
                #[ur::tool(name = "x")]
                async fn a(&self) -> i64 { 1 }

                #[cfg(not(feature = "a"))]
                #[ur::tool(name = "x")]
                async fn b(&self) -> i64 { 2 }
            }
            "#,
        );
        assert!(out.is_ok());
    }

    #[test]
    fn rejects_two_tool_markers() {
        let err = expand_str(
            "",
            "impl Tools { #[ur::tool] #[ur::tool] async fn ping(&self) -> i64 { 1 } }",
        )
        .unwrap_err();
        assert!(err.to_string().contains("only one `#[ur::tool]`"));
    }

    #[test]
    fn rejects_nonempty_attribute() {
        let err = expand_str(
            r#"name = "x""#,
            "impl Tools { #[ur::tool] async fn ping(&self) -> i64 { 1 } }",
        )
        .unwrap_err();
        assert!(err.to_string().contains("does not take arguments"));
    }

    #[test]
    fn rejects_trait_impl() {
        let err = expand_str(
            "",
            "impl Other for Tools { #[ur::tool] async fn ping(&self) -> i64 { 1 } }",
        )
        .unwrap_err();
        assert!(err.to_string().contains("inherent impl block"));
    }

    #[test]
    fn rejects_generic_impl() {
        let err = expand_str(
            "",
            "impl<T> Tools<T> { #[ur::tool] async fn ping(&self) -> i64 { 1 } }",
        )
        .unwrap_err();
        assert!(err.to_string().contains("generic impl"));
    }

    #[test]
    fn rejects_generic_method() {
        let err = expand_str(
            "",
            "impl Tools { #[ur::tool] async fn ping<T>(&self, x: T) -> i64 { 1 } }",
        )
        .unwrap_err();
        assert!(err.to_string().contains("generic"));
    }

    #[test]
    fn rejects_mut_self_receiver() {
        let err = expand_str(
            "",
            "impl Tools { #[ur::tool] async fn ping(&mut self) -> i64 { 1 } }",
        )
        .unwrap_err();
        assert!(err.to_string().contains("&self"));
    }

    #[test]
    fn rejects_value_self_receiver() {
        let err = expand_str(
            "",
            "impl Tools { #[ur::tool] async fn ping(self) -> i64 { 1 } }",
        )
        .unwrap_err();
        assert!(err.to_string().contains("&self"));
    }

    #[test]
    fn rejects_no_receiver_method() {
        let err = expand_str(
            "",
            "impl Tools { #[ur::tool] async fn ping(x: i64) -> i64 { 1 } }",
        )
        .unwrap_err();
        assert!(err.to_string().contains("&self"));
    }

    #[test]
    fn gates_registration_on_method_cfg() {
        let out = expand_str(
            "",
            r#"
            impl Tools {
                #[cfg(feature = "x")]
                #[ur::tool]
                async fn ping(&self) -> i64 { 1 }
            }
            "#,
        )
        .unwrap();
        // The cfg gates both the retained method and the generated registration block.
        assert!(out.matches("cfg (feature = \"x\")").count() >= 2);
    }
}
