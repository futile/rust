//@ force-host
//@ no-prefer-dynamic

#![feature(proc_macro_cacheable)]

#![crate_type = "proc-macro"]

extern crate proc_macro;

use proc_macro::TokenStream;

#[proc_macro_derive(IncrementalMacro)]
#[proc_macro_cacheable]
pub fn derive(input: TokenStream) -> TokenStream {
    "".parse().unwrap()
}
