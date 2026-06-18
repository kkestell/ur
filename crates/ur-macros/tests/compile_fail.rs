//! Compile-fail coverage for `#[ur::tool]` validation errors.
//!
//! These cases are rejected by the macro itself, before the generated `::ur`
//! paths need to resolve, so they fail identically without the facade in scope.

#[test]
fn validation_errors() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/*.rs");
}
