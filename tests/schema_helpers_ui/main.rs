//! Accepted schema-helper call sites compile with this integration target.

mod param_name_call {
    //! Compile and execute the accepted `ParamName` call-site fixture.

    include!("pass/param_name_call.rs");

    /// Run the accepted `ParamName` call-site fixture.
    pub(super) fn run() {
        main();
    }
}

mod str_call {
    //! Compile and execute the accepted `&str` call-site fixture.

    include!("pass/str_call.rs");

    /// Run the accepted `&str` call-site fixture.
    pub(super) fn run() {
        main();
    }
}

/// Execute every accepted schema-helper call-site fixture.
#[test]
fn ui() {
    param_name_call::run();
    str_call::run();
}
