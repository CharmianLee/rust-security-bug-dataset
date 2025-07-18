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


// SECTION 2: VULNERABLE CODE

// This `impl` block contains the vulnerable function `into_inner`, copied from the
// affected version of the `tracing` crate.
impl<T> Instrumented<T> {
    /// Consumes the `Instrumented`, returning the wrapped type.
    ///
    /// Note that this drops the span.
    pub fn into_inner(self) -> T {
        // To manually destructure `Instrumented` without `Drop`, we save
        // pointers to the fields and use `mem::forget` to leave those pointers
        // valid.
        let span: *const Span = &self.span;
        let inner: *const ManuallyDrop<T> = &self.inner;
        mem::forget(self);
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
    // 1. Setup vulnerable object.
    // If the stack memory for `instrumented` is corrupted after `mem::forget`,
    // the `read` operation in `into_inner` could create a `String` with a
    // dangling pointer, leading to a crash upon use or drop.
    let original_string = String::from("this value should be preserved");
    let instrumented = Instrumented::new(original_string.clone());

    // 2. Trigger BUG.
    // After `mem::forget(self)` inside `into_inner`, the stack memory for `instrumented`
    // is considered free and can be overwritten. The subsequent `unsafe` reads from
    // that memory may read corrupted data.
    let returned_string = instrumented.into_inner();

    // 3. Verify UAF.
    // Accessing `returned_string` might cause a crash if its internal pointer is invalid.
    // The assertion checks if the data was corrupted during the unsafe `into_inner` call.
    println!("Original string:  \"{}\"", original_string);
    println!("Returned string:  \"{}\"", returned_string);
    assert_eq!(original_string, returned_string);
    println!("Verification successful: The string data was not corrupted.");
}