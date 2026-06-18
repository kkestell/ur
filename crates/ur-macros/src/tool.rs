//! Implementation of the `#[ur::tool]` attribute macro.

use proc_macro2::TokenStream;
use quote::quote;
use syn::parse::Parser;
use syn::punctuated::Punctuated;
use syn::{
    Attribute, Expr, ExprLit, FnArg, ItemFn, Lit, MetaNameValue, Pat, PatType, ReturnType,
    Signature, Token, Type, parse2,
};

/// A single tool parameter, reduced to the binding name and its type.
struct Param {
    ident: syn::Ident,
    ty: Box<Type>,
}

/// Parsed `#[ur::tool(...)]` attribute arguments.
#[cfg_attr(test, derive(Debug))]
struct ToolConfig {
    description: Option<String>,
    name: Option<String>,
    param_docs: Vec<(String, String)>,
}

/// Expands `#[ur::tool]` on a function into a same-named tool type.
pub fn expand(attr: TokenStream, item: TokenStream) -> syn::Result<TokenStream> {
    let func: ItemFn = parse2(item)?;
    validate_signature(&func.sig)?;
    let params = parse_params(&func.sig)?;
    let param_names: Vec<String> = params.iter().map(|p| p.ident.to_string()).collect();
    let config = parse_config(attr, &param_names)?;
    Ok(generate(&func, &params, &config))
}

/// Rejects function signatures the macro cannot turn into a tool.
fn validate_signature(sig: &Signature) -> syn::Result<()> {
    if let Some(c) = &sig.constness {
        return Err(syn::Error::new_spanned(
            c,
            "`#[ur::tool]` does not support const functions",
        ));
    }
    if let Some(u) = &sig.unsafety {
        return Err(syn::Error::new_spanned(
            u,
            "`#[ur::tool]` does not support unsafe functions",
        ));
    }
    if let Some(abi) = &sig.abi {
        return Err(syn::Error::new_spanned(
            abi,
            "`#[ur::tool]` does not support extern functions",
        ));
    }
    if !sig.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &sig.generics,
            "`#[ur::tool]` does not support generic functions",
        ));
    }
    if let Some(w) = &sig.generics.where_clause {
        return Err(syn::Error::new_spanned(
            w,
            "`#[ur::tool]` does not support where clauses",
        ));
    }
    if let Some(v) = &sig.variadic {
        return Err(syn::Error::new_spanned(
            v,
            "`#[ur::tool]` does not support variadic functions",
        ));
    }
    if let ReturnType::Type(_, ty) = &sig.output
        && matches!(**ty, Type::ImplTrait(_))
    {
        return Err(syn::Error::new_spanned(
            ty,
            "`#[ur::tool]` does not support `impl Trait` return types",
        ));
    }
    Ok(())
}

/// Reduces the argument list to simple `name: Type` bindings, rejecting anything else.
fn parse_params(sig: &Signature) -> syn::Result<Vec<Param>> {
    let mut params = Vec::new();
    for input in &sig.inputs {
        match input {
            FnArg::Receiver(r) => {
                return Err(syn::Error::new_spanned(
                    r,
                    "`#[ur::tool]` does not support methods with a `self` receiver",
                ));
            }
            FnArg::Typed(PatType { pat, ty, .. }) => {
                let ident = match &**pat {
                    Pat::Ident(pi) if pi.by_ref.is_none() && pi.subpat.is_none() => {
                        pi.ident.clone()
                    }
                    _ => {
                        return Err(syn::Error::new_spanned(
                            pat,
                            "`#[ur::tool]` parameters must be simple `name: Type` bindings",
                        ));
                    }
                };
                params.push(Param {
                    ident,
                    ty: ty.clone(),
                });
            }
        }
    }
    Ok(params)
}

/// Parses the attribute argument list, validating keys against the parameter names.
fn parse_config(attr: TokenStream, param_names: &[String]) -> syn::Result<ToolConfig> {
    let mut config = ToolConfig {
        description: None,
        name: None,
        param_docs: Vec::new(),
    };
    if attr.is_empty() {
        return Ok(config);
    }
    let metas = Punctuated::<MetaNameValue, Token![,]>::parse_terminated.parse2(attr)?;
    for meta in metas {
        let key = meta
            .path
            .get_ident()
            .ok_or_else(|| syn::Error::new_spanned(&meta.path, "expected a simple attribute key"))?
            .to_string();
        let value = lit_str_value(&meta.value)?;
        match key.as_str() {
            "description" => config.description = Some(value),
            "name" => {
                if !is_valid_tool_name(&value) {
                    return Err(syn::Error::new_spanned(
                        &meta.value,
                        "tool name must match `[a-zA-Z0-9_-]{1,64}`",
                    ));
                }
                config.name = Some(value);
            }
            other if param_names.iter().any(|p| p == other) => {
                config.param_docs.push((other.to_string(), value));
            }
            other => {
                return Err(syn::Error::new_spanned(
                    &meta.path,
                    format!(
                        "unknown `#[ur::tool]` attribute `{other}`; expected `description`, `name`, or a parameter name"
                    ),
                ));
            }
        }
    }
    Ok(config)
}

/// Extracts the value of a string-literal attribute argument.
fn lit_str_value(expr: &Expr) -> syn::Result<String> {
    if let Expr::Lit(ExprLit {
        lit: Lit::Str(s), ..
    }) = expr
    {
        Ok(s.value())
    } else {
        Err(syn::Error::new_spanned(expr, "expected a string literal"))
    }
}

/// Returns whether a tool name satisfies `[a-zA-Z0-9_-]{1,64}`.
fn is_valid_tool_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// Returns whether the return type is a `Result`, requiring error stringification.
fn returns_result(output: &ReturnType) -> bool {
    if let ReturnType::Type(_, ty) = output
        && let Type::Path(tp) = &**ty
        && let Some(seg) = tp.path.segments.last()
    {
        return seg.ident == "Result";
    }
    false
}

/// Returns whether an attribute should also gate the generated impl block.
fn is_cfg_attr(attr: &Attribute) -> bool {
    attr.path().is_ident("cfg") || attr.path().is_ident("cfg_attr")
}

/// Generates the tool type, parameter struct, and `Tool` impl.
fn generate(func: &ItemFn, params: &[Param], config: &ToolConfig) -> TokenStream {
    let vis = &func.vis;
    let fn_ident = &func.sig.ident;
    let tool_name = config.name.clone().unwrap_or_else(|| fn_ident.to_string());

    let attrs = &func.attrs;
    let cfg_attrs: Vec<&Attribute> = attrs.iter().filter(|a| is_cfg_attr(a)).collect();

    let fields = params.iter().map(|p| {
        let ident = &p.ident;
        let ty = &p.ty;
        let ident_str = ident.to_string();
        match config.param_docs.iter().find(|(n, _)| *n == ident_str) {
            Some((_, desc)) => quote! { #[schemars(description = #desc)] #ident: #ty },
            None => quote! { #ident: #ty },
        }
    });

    let asyncness = &func.sig.asyncness;
    let inputs = &func.sig.inputs;
    let output = &func.sig.output;
    let block = &func.block;
    let inner = quote! {
        #asyncness fn __ur_tool_body(#inputs) #output #block
    };

    let arg_idents = params.iter().map(|p| &p.ident);
    let call = if params.is_empty() {
        quote! { __ur_tool_body() }
    } else {
        quote! { __ur_tool_body( #( __ur_args.#arg_idents ),* ) }
    };
    let invoke = if asyncness.is_some() {
        quote! { #call.await }
    } else {
        quote! { #call }
    };

    let deserialize = if params.is_empty() {
        quote! {}
    } else {
        quote! {
            let __ur_args: __UrParams = match args.parse::<__UrParams>() {
                ::core::result::Result::Ok(__v) => __v,
                ::core::result::Result::Err(__e) => {
                    return ::core::result::Result::Err(::std::string::ToString::to_string(&__e));
                }
            };
        }
    };

    let finish = if returns_result(output) {
        quote! {
            match __ur_outcome {
                ::core::result::Result::Ok(__v) => match ::ur::__rt::serde_json::to_string(&__v) {
                    ::core::result::Result::Ok(__s) => ::core::result::Result::Ok(__s),
                    ::core::result::Result::Err(__e) =>
                        ::core::result::Result::Err(::std::string::ToString::to_string(&__e)),
                },
                ::core::result::Result::Err(__e) =>
                    ::core::result::Result::Err(::std::string::ToString::to_string(&__e)),
            }
        }
    } else {
        quote! {
            match ::ur::__rt::serde_json::to_string(&__ur_outcome) {
                ::core::result::Result::Ok(__s) => ::core::result::Result::Ok(__s),
                ::core::result::Result::Err(__e) =>
                    ::core::result::Result::Err(::std::string::ToString::to_string(&__e)),
            }
        }
    };

    let schema_desc = match &config.description {
        Some(d) => quote! { .description(#d) },
        None => quote! {},
    };

    quote! {
        #[allow(non_camel_case_types)]
        #(#attrs)*
        #vis struct #fn_ident;

        #(#cfg_attrs)*
        const _: () = {
            #[derive(::ur::__rt::serde::Deserialize, ::ur::__rt::schemars::JsonSchema)]
            #[serde(crate = "::ur::__rt::serde")]
            #[schemars(crate = "::ur::__rt::schemars")]
            struct __UrParams {
                #(#fields),*
            }

            impl ::ur::Tool for #fn_ident {
                fn name(&self) -> &str {
                    #tool_name
                }

                fn schema(&self) -> ::ur::ToolSchema {
                    let __schema = ::ur::__rt::schemars::SchemaGenerator::default()
                        .into_root_schema_for::<__UrParams>()
                        .to_value();
                    ::ur::ToolSchema::new(#tool_name, __schema) #schema_desc
                }

                fn call(&self, args: ::ur::ToolArguments)
                    -> ::ur::BoxFuture<
                        'static,
                        ::core::result::Result<::std::string::String, ::std::string::String>,
                    >
                {
                    #inner
                    ::std::boxed::Box::pin(async move {
                        #deserialize
                        let __ur_outcome = #invoke;
                        #finish
                    })
                }
            }
        };
    }
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
    fn validates_tool_names() {
        assert!(is_valid_tool_name("add"));
        assert!(is_valid_tool_name("add_ints"));
        assert!(is_valid_tool_name("get-weather"));
        assert!(is_valid_tool_name("A1_-"));
        assert!(is_valid_tool_name(&"a".repeat(64)));

        assert!(!is_valid_tool_name(""));
        assert!(!is_valid_tool_name("bad name"));
        assert!(!is_valid_tool_name("bad!"));
        assert!(!is_valid_tool_name(&"a".repeat(65)));
    }

    #[test]
    fn detects_result_return() {
        let f: ItemFn = parse2(ts("fn f() -> Result<i64, String> {}")).unwrap();
        assert!(returns_result(&f.sig.output));
        let f: ItemFn = parse2(ts("fn f() -> std::io::Result<i64> {}")).unwrap();
        assert!(returns_result(&f.sig.output));
        let f: ItemFn = parse2(ts("fn f() -> i64 {}")).unwrap();
        assert!(!returns_result(&f.sig.output));
        let f: ItemFn = parse2(ts("fn f() {}")).unwrap();
        assert!(!returns_result(&f.sig.output));
    }

    #[test]
    fn parses_simple_parameters() {
        let f: ItemFn = parse2(ts("fn f(a: i64, mut b: String) {}")).unwrap();
        let params = parse_params(&f.sig).unwrap();
        let names: Vec<_> = params.iter().map(|p| p.ident.to_string()).collect();
        assert_eq!(names, ["a", "b"]);
    }

    #[test]
    fn rejects_receiver_and_patterns() {
        let f: ItemFn = parse2(ts("fn f(&self) {}")).unwrap();
        assert!(parse_params(&f.sig).is_err());
        let f: ItemFn = parse2(ts("fn f((a, b): (i64, i64)) {}")).unwrap();
        assert!(parse_params(&f.sig).is_err());
    }

    #[test]
    fn parses_config_keys() {
        let config = parse_config(
            ts(r#"description = "d", name = "add", a = "first""#),
            &["a".to_string()],
        )
        .unwrap();
        assert_eq!(config.description.as_deref(), Some("d"));
        assert_eq!(config.name.as_deref(), Some("add"));
        assert_eq!(config.param_docs, [("a".to_string(), "first".to_string())]);
    }

    #[test]
    fn rejects_unknown_attribute_key() {
        let err = parse_config(ts(r#"nonsense = "x""#), &[]).unwrap_err();
        assert!(err.to_string().contains("unknown"));
    }

    #[test]
    fn rejects_invalid_name_value() {
        let err = parse_config(ts(r#"name = "bad name""#), &[]).unwrap_err();
        assert!(err.to_string().contains("must match"));
    }

    #[test]
    fn classifies_cfg_attributes() {
        let f: ItemFn = parse2(ts(
            "#[doc = \"x\"] #[cfg(test)] #[cfg_attr(test, allow(dead_code))] fn f() {}",
        ))
        .unwrap();
        let cfg: Vec<_> = f.attrs.iter().filter(|a| is_cfg_attr(a)).collect();
        assert_eq!(cfg.len(), 2);
    }

    #[test]
    fn generates_same_identifier_and_name() {
        let out = expand_str(
            r#"description = "Add two integers.""#,
            "async fn add(a: i64, b: i64) -> i64 { a + b }",
        )
        .unwrap();
        assert!(out.contains("struct add"));
        assert!(out.contains("impl :: ur :: Tool for add"));
        assert!(out.contains("\"add\""));
        assert!(out.contains("parse ::"));
    }

    #[test]
    fn name_override_keeps_struct_identifier() {
        let out = expand_str(r#"name = "add_ints""#, "fn add(a: i64) -> i64 { a }").unwrap();
        assert!(out.contains("struct add"));
        assert!(out.contains("\"add_ints\""));
    }

    #[test]
    fn forwards_attributes_to_struct_and_gates_impl() {
        let out = expand_str(
            "",
            "#[doc = \"docs\"] #[cfg(feature = \"x\")] fn f() -> i64 { 1 }",
        )
        .unwrap();
        // The doc and cfg are forwarded onto the generated struct.
        assert!(out.contains("doc = \"docs\""));
        // The cfg also gates the impl block (appears twice: struct + const).
        assert_eq!(out.matches("cfg (feature = \"x\")").count(), 2);
    }

    #[test]
    fn no_argument_tool_skips_parsing() {
        let out = expand_str("", "fn ping() -> i64 { 1 }").unwrap();
        assert!(out.contains("struct __UrParams"));
        assert!(!out.contains("args . parse"));
    }

    #[test]
    fn folds_parameter_description_into_schema() {
        let out = expand_str(r#"a = "the addend""#, "fn add(a: i64) -> i64 { a }").unwrap();
        assert!(out.contains("schemars (description = \"the addend\")"));
    }
}
