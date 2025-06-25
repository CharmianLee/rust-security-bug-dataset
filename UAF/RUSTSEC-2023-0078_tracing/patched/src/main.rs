// Minimal use, example:
use std::mem::{self, ManuallyDrop};

// SECTION 1: MINIMAL TYPES, TRAITS, AND HELPER FUNCTIONS
/// A minimal stub for `tracing::Span`.
#[derive(Debug, Clone, Copy)]
pub struct Span {
    // A dummy field to give the struct a non-zero size
    id: u64,
}

impl Span {
    /// A simple constructor for the stub type.
    pub fn new() -> Self {
        Self { id: 1 }
    }
}

/// A simple type to be wrapped by `Instrumented`.
#[derive(Debug)]
struct MyType {
    data: u32,
}

/// A minimal definition of `Instrumented<T>` containing only the fields
#[derive(Debug)]
pub struct Instrumented<T> {
    inner: ManuallyDrop<T>,
    span: Span,
}

// SECTION 2: PATCHED CODE
impl<T> Instrumented<T> {
    /// A simplified constructor to create an `Instrumented` instance for the PoC.
    pub fn new(value: T, span: Span) -> Self {
        Self {
            inner: ManuallyDrop::new(value),
            span,
        }
    }

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
    // 1. Setup vulnerable object
    let my_value = MyType { data: 42 };
    let span = Span::new();
    let instrumented_object = Instrumented::new(my_value, span);
    
    // 2. Trigger BUG
    let extracted_value = instrumented_object.into_inner();
    println!("{:?}", extracted_value);
}