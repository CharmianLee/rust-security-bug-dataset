#![allow(clippy::new_without_default)]
// SECTION 1: MINIMAL DEPENDENCIES

use std::alloc::Layout;
use std::cell::RefCell;
use std::marker::PhantomData;
use std::mem;
use std::ptr::{self, NonNull};

// Helper functions
fn capacity_overflow() -> ! {
    panic!("capacity overflow")
}

fn handle_alloc_error(layout: Layout) -> ! {
    panic!("encountered allocation error: {:?}", layout)
}

unsafe fn arith_offset<T>(p: *const T, offset: isize) -> *const T {
    p.offset(offset)
}

unsafe fn offset_from<T>(p: *const T, origin: *const T) -> isize
where
    T: Sized,
{
    let pointee_size = mem::size_of::<T>();
    assert!(0 < pointee_size && pointee_size <= isize::MAX as usize);
    isize::wrapping_sub(p as _, origin as _) / (pointee_size as isize)
}

// Simplified Bump allocator that correctly models arena-like deallocation on drop.
pub struct Bump {
    // Use the standard library's Vec for internal bookkeeping.
    allocations: RefCell<std::vec::Vec<(NonNull<u8>, Layout)>>,
}

impl Bump {
    pub fn new() -> Self {
        Bump {
            // Call the standard Vec's `new` method.
            allocations: RefCell::new(std::vec::Vec::new()),
        }
    }

    #[inline(always)]
    pub fn alloc<T>(&self, val: T) -> &mut T {
        self.alloc_with(|| val)
    }

    #[inline(always)]
    pub fn alloc_with<F, T>(&self, f: F) -> &mut T
    where
        F: FnOnce() -> T,
    {
        let layout = Layout::new::<T>();
        unsafe {
            let p = self.alloc_layout(layout).as_ptr() as *mut T;
            ptr::write(p, f());
            &mut *p
        }
    }

    // This function now panics on allocation failure, removing the need for the unstable `AllocError`.
    fn alloc_layout(&self, layout: Layout) -> NonNull<u8> {
        let ptr = unsafe { std::alloc::alloc(layout) };
        let non_null_ptr = match NonNull::new(ptr) {
            Some(p) => p,
            None => handle_alloc_error(layout),
        };
        self.allocations.borrow_mut().push((non_null_ptr, layout));
        non_null_ptr
    }
}

impl Drop for Bump {
    fn drop(&mut self) {
        for (ptr, layout) in self.allocations.get_mut().iter() {
            unsafe {
                std::alloc::dealloc(ptr.as_ptr(), *layout);
            }
        }
    }
}

// Minimal definitions for Vec (as defined in the crate)
pub struct RawVec<'a, T> {
    ptr: NonNull<T>,
    cap: usize,
    a: &'a Bump,
}

impl<'a, T> RawVec<'a, T> {
    pub fn new_in(a: &'a Bump) -> Self {
        RawVec {
            ptr: NonNull::dangling(),
            cap: 0,
            a,
        }
    }

    fn grow(&mut self, len: usize, additional: usize) {
        let required_cap = len.checked_add(additional).unwrap_or_else(|| capacity_overflow());
        let new_cap = required_cap.max(self.cap * 2).max(1);
        let new_layout = Layout::array::<T>(new_cap).unwrap_or_else(|_| capacity_overflow());

        let new_ptr = self.a.alloc_layout(new_layout);

        if self.cap > 0 {
            unsafe {
                ptr::copy_nonoverlapping(self.ptr.as_ptr(), new_ptr.as_ptr() as *mut T, self.cap);
            };
        }
        self.ptr = new_ptr.cast();
        self.cap = new_cap;
    }

    pub fn reserve(&mut self, len: usize, additional: usize) {
        if self.cap - len < additional {
            self.grow(len, additional);
        }
    }

    fn ptr(&self) -> *mut T { self.ptr.as_ptr() }

    fn cap(&self) -> usize { self.cap }
}

pub struct Vec<'bump, T: 'bump> {
    buf: RawVec<'bump, T>,
    len: usize,
}

impl<'bump, T: 'bump> Vec<'bump, T> {
    pub fn new_in(bump: &'bump Bump) -> Vec<'bump, T> {
        Vec {
            buf: RawVec::new_in(bump),
            len: 0,
        }
    }

    #[inline] pub fn len(&self) -> usize { self.len }
    #[inline] pub fn as_mut_ptr(&mut self) -> *mut T { self.buf.ptr() }

    pub fn reserve(&mut self, additional: usize) {
        self.buf.reserve(self.len, additional);
    }

    #[inline]
    pub fn push(&mut self, value: T) {
        if self.len == self.buf.cap() {
            self.reserve(1);
        }
        unsafe {
            let end = self.buf.ptr().add(self.len);
            ptr::write(end, value);
            self.len += 1;
        }
    }
}

impl<'bump, T: 'bump> Extend<T> for Vec<'bump, T> {
    #[inline]
    fn extend<I: IntoIterator<Item = T>>(&mut self, iter: I) {
        let iter = iter.into_iter();
        self.reserve(iter.size_hint().0);
        for t in iter {
            self.push(t);
        }
    }
}

// SECTION 2: PATCHED CODE

// The IntoIter struct is now correctly parameterized with the lifetime `'bump`
// from the allocator, and its PhantomData field ties the iterator to this lifetime.
pub struct IntoIter<'bump, T> {
    phantom: PhantomData<&'bump [T]>,
    ptr: *const T,
    end: *const T,
}

impl<'bump, T: 'bump> IntoIterator for Vec<'bump, T> {
    type Item = T;
    type IntoIter = IntoIter<'bump, T>;

    #[inline]
    fn into_iter(mut self) -> IntoIter<'bump, T> {
        unsafe {
            let begin = self.as_mut_ptr();
            let end = if mem::size_of::<T>() == 0 {
                arith_offset(begin as *const i8, self.len() as isize) as *const T
            } else {
                begin.add(self.len())
            };
            mem::forget(self);
            IntoIter {
                phantom: PhantomData,
                ptr: begin,
                end,
            }
        }
    }
}

impl<'bump, T: 'bump> Iterator for IntoIter<'bump, T> {
    type Item = T;

    #[inline]
    fn next(&mut self) -> Option<T> {
        unsafe {
            if self.ptr as *const _ == self.end {
                None
            } else if mem::size_of::<T>() == 0 {
                self.ptr = arith_offset(self.ptr as *const i8, 1) as *mut T;
                Some(mem::zeroed())
            } else {
                let old = self.ptr;
                self.ptr = self.ptr.offset(1);
                Some(ptr::read(old))
            }
        }
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let exact = if mem::size_of::<T>() == 0 {
            (self.end as usize).wrapping_sub(self.ptr as usize)
        } else {
            unsafe { offset_from(self.end, self.ptr) as usize }
        };
        (exact, Some(exact))
    }
}

// SECTION 3: PROOF-OF-CONCEPT (DEMONSTRATES THE FIX)

fn main() {
    // 1. Setup a vector allocated within a bump arena.
    let bump = Bump::new();
    let mut vec = Vec::new_in(&bump);
    vec.extend([0x01u8; 32]);
    let mut into_iter = vec.into_iter();

    // 2. THIS NOW CAUSES A COMPILE-TIME ERROR.
    // The borrow checker sees that `into_iter` holds a reference to `bump`.
    // Dropping `bump` while `into_iter` is still in scope is now forbidden.
    drop(bump); // <-- COMPILE ERROR: `bump` is dropped here but borrowed later

    // 3. This section is now unreachable due to the compile error above.
    for _ in 0..100 {
        let reuse_bump = Bump::new();
        let _reuse_alloc = reuse_bump.alloc([0x41u8; 64]);
    }

    // 4. This use of `into_iter` is what causes the borrow checker to report the error.
    let first_val = into_iter.next().unwrap_or(0);
    println!("Read from iterator: 0x{:02x}", first_val);

    assert_eq!(first_val, 0x01);
}