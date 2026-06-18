//! Compile-pass lock for the documented `#[ur::tool]` examples through the facade.

#[test]
fn api_macro_examples_compile_through_facade() {
    let t = trybuild::TestCases::new();
    t.pass("tests/ui/pass/*.rs");
}
