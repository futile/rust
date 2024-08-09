//@ aux-build:cached_derive_macro_aux.rs
//@ revisions: cfail1
//@ build-pass

#![crate_type = "rlib"]

#[macro_use]
extern crate cached_derive_macro_aux;

#[derive(IncrementalMacro)]
pub struct Foo {
    x: u32
}
