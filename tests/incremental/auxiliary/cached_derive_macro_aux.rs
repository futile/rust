//@ force-host
//@ no-prefer-dynamic
//@ rustc-env:RUSTC_LOG=rustc_expand::base,rustc_builtin_macros::proc_macro_harness

// TODO: ^ remove rustc-env (used this for testing only)

#![feature(proc_macro_cacheable)]

#![crate_type = "proc-macro"]

extern crate proc_macro;

use proc_macro::TokenStream;

#[proc_macro_derive(IncrementalMacro)]
#[proc_macro_cacheable]
pub fn derive(input: TokenStream) -> TokenStream {
    "".parse().unwrap()
}
