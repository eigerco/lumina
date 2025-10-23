//! Binary executable for generating UniFFI bindings.
//!
//! This binary is used by the build system to generate language bindings.

/// This is the entry point for the uniffi-bindgen binary.
fn main() {
    uniffi::uniffi_bindgen_main()
}
