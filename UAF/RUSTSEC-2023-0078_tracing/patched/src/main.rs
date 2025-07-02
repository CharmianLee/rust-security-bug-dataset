// SECTION 1: MINIMAL DEPENDENCIES

use std::mem::{self, ManuallyDrop};
use std::ptr;

// Minimal definition for `tracing::Metadata`
#[derive(Debug, Clone)]
pub struct Metadata<'a> {
    _name: &'a str,
}

static METADATA: Metadata<'static> = Metadata { _name: "poc_span" };

// Minimal definition for `tracing::span::Inner`
#[derive(Debug, Clone)]
pub struct Inner;

// Minimal definition for `tracing::Span`
#[derive(Clone, Debug)]
pub struct Span {
    _inner: Option<Inner>,
    _meta: Option<&'static Metadata<'static>>,
}

impl Span {
    pub fn new() -> Self {
        Self {
            _inner: Some(Inner),
            _meta: Some(&METADATA),
        }
    }
}

// Minimal definition for `tracing::Instrumented<T>`
// This struct mirrors the memory layout of the original without depending on `pin-project`.
#[derive(Debug, Clone)]
pub struct Instrumented<T> {
    inner: ManuallyDrop<T>,
    span: Span,
}

// A constructor is added here to facilitate the PoC setup.
impl<T> Instrumented<T> {
    pub fn new(inner: T) -> Self {
        Self {
            inner: ManuallyDrop::new(inner),
            span: Span::new(),
        }
    }
}


// SECTION 2: PATCHED CODE

// This `impl` block contains the patched, safe version of the `into_inner` function.
impl<T> Instrumented<T> {
    /// Consumes the `Instrumented`, returning the wrapped type.
    ///
    /// Note that this drops the span.
    pub fn into_inner(self) -> T {
        // To manually destructure `Instrumented` without `Drop`, we
        // move it into a ManuallyDrop and use pointers to its fields
        let this = ManuallyDrop::new(self);
        let span: *const Span = &this.span;
        let inner: *const ManuallyDrop<T> = &this.inner;
        // SAFETY: Those pointers are valid for reads, because `Drop` didn't
        //         run, and properly aligned, because `Instrumented` isn't
        //         `#[repr(packed)]`.
        let _span = unsafe { span.read() };
        let inner = unsafe { inner.read() };
        ManuallyDrop::into_inner(inner)
    }
}


// SECTION 3: PROOF-OF-CONCEPT

fn main() {
    // 1. Setup object.
    // The setup remains identical to the vulnerable version.
    let original_string = String::from("this value should be preserved");
    let instrumented = Instrumented::new(original_string.clone());

    // 2. Trigger the safe method.
    // This call now uses the patched `into_inner` function, which is free of
    // undefined behavior.
    let returned_string = instrumented.into_inner();

    // 3. Verify the result.
    // The assertion confirms that the data integrity is maintained, and now
    // this behavior is guaranteed by the language's safety rules, not by chance.
    println!("Original string:  \"{}\"", original_string);
    println!("Returned string:  \"{}\"", returned_string);
    assert_eq!(original_string, returned_string);
    println!("Verification successful: The string data was correctly preserved.");
}