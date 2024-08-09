//@ force-host
//@ no-prefer-dynamic

// #![feature(proc_macro_cacheable)]

#![crate_type = "proc-macro"]

extern crate proc_macro;

use proc_macro::TokenStream;

#[proc_macro_derive(SomeMacro)]
#[proc_macro_cacheable] //~ error: the `#[proc_macro_cacheable]` attribute is an experimental feature
pub fn derive(input: TokenStream) -> TokenStream {
    "".parse().unwrap()
}
