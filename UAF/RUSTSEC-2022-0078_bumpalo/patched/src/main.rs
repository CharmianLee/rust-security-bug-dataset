// main.rs

use std::alloc::{alloc, dealloc, Layout};
use std::cell::Cell;
use std::marker::PhantomData;
use std::mem;
use std::ptr::{self, NonNull};

// SECTION 1: MINIMAL TYPES, TRAITS, AND HELPER FUNCTIONS
// --- Helper constants and functions ---
const CHUNK_ALIGN: usize = 16;
const FOOTER_SIZE: usize = mem::size_of::<ChunkFooter>();
const DEFAULT_CHUNK_SIZE_WITHOUT_FOOTER: usize = 1024 - FOOTER_SIZE;
#[inline(never)]
#[cold]
fn oom() -> ! {
    panic!("out of memory");
}
#[inline]
unsafe fn arith_offset<T>(p: *const T, offset: isize) -> *const T {
    (p as *const u8).offset(offset as isize * mem::size_of::<T>() as isize) as *const T
}
// --- Allocator related structs ---
#[repr(C)]
struct ChunkFooter {
    data: NonNull<u8>,
    layout: Layout,
    prev: Cell<NonNull<ChunkFooter>>,
    ptr: Cell<NonNull<u8>>,
}
#[derive(Debug)]
pub struct Bump {
    current_chunk_footer: Cell<NonNull<ChunkFooter>>,
}
impl Bump {
    pub fn new() -> Bump {
        static mut EMPTY_CHUNK_FOOTER: ChunkFooter = ChunkFooter {
            data: NonNull::dangling(),
            layout: unsafe { Layout::from_size_align_unchecked(0, CHUNK_ALIGN) },
            prev: Cell::new(NonNull::dangling()),
            ptr: Cell::new(NonNull::dangling()),
        };
        unsafe {
            EMPTY_CHUNK_FOOTER
                .prev
                .set(NonNull::from(&EMPTY_CHUNK_FOOTER));
            Bump {
                current_chunk_footer: Cell::new(NonNull::from(&EMPTY_CHUNK_FOOTER)),
            }
        }
    }
    #[allow(clippy::mut_from_ref)]
    pub fn alloc<T>(&self, val: T) -> &mut T {
        let layout = Layout::new::<T>();
        unsafe {
            let p = self.alloc_layout(layout);
            let p = p.as_ptr() as *mut T;
            ptr::write(p, val);
            &mut *p
        }
    }
    #[inline(always)]
    pub fn alloc_layout(&self, layout: Layout) -> NonNull<u8> {
        if let Some(p) = self.try_alloc_layout_fast(layout) {
            p
        } else {
            self.alloc_layout_slow(layout).unwrap_or_else(|| oom())
        }
    }
    #[inline(always)]
    fn try_alloc_layout_fast(&self, layout: Layout) -> Option<NonNull<u8>> {
        unsafe {
            let footer = self.current_chunk_footer.get().as_ref();
            if footer.data == NonNull::dangling() {
                return None;
            }
            let ptr = footer.ptr.get().as_ptr();
            let start = footer.data.as_ptr();
            let aligned_ptr = (ptr as usize).saturating_sub(layout.size()) & !(layout.align() - 1);
            if aligned_ptr >= start as usize {
                let aligned_ptr = NonNull::new_unchecked(aligned_ptr as *mut u8);
                footer.ptr.set(aligned_ptr);
                Some(aligned_ptr)
            } else {
                None
            }
        }
    }
    #[inline(never)]
    fn alloc_layout_slow(&self, layout: Layout) -> Option<NonNull<u8>> {
        unsafe {
            let new_chunk_size =
                (layout.size() + FOOTER_SIZE).max(DEFAULT_CHUNK_SIZE_WITHOUT_FOOTER + FOOTER_SIZE);
            let new_chunk_layout = Layout::from_size_align(new_chunk_size, CHUNK_ALIGN).ok()?;
            let new_chunk_data = alloc(new_chunk_layout);
            if new_chunk_data.is_null() {
                return None;
            }
            let footer_ptr = new_chunk_data.add(new_chunk_size - FOOTER_SIZE) as *mut ChunkFooter;
            ptr::write(
                footer_ptr,
                ChunkFooter {
                    data: NonNull::new_unchecked(new_chunk_data),
                    layout: new_chunk_layout,
                    prev: self.current_chunk_footer.clone(),
                    ptr: Cell::new(NonNull::new_unchecked(footer_ptr as *mut u8)),
                },
            );
            self.current_chunk_footer
                .set(NonNull::new_unchecked(footer_ptr));
            self.try_alloc_layout_fast(layout)
        }
    }
}
unsafe fn dealloc_chunk_list(mut footer: NonNull<ChunkFooter>) {
    while footer.as_ref().prev.get() != footer {
        let f = footer;
        footer = f.as_ref().prev.get();
        dealloc(f.as_ref().data.as_ptr(), f.as_ref().layout);
    }
}
impl Drop for Bump {
    fn drop(&mut self) {
        unsafe {
            dealloc_chunk_list(self.current_chunk_footer.get());
        }
    }
}
// --- Vector related structs ---
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
    fn reserve(&mut self, len: usize, additional: usize) {
        if self.cap - len >= additional {
            return;
        }
        let new_cap = (len + additional).next_power_of_two().max(4);
        let new_layout = Layout::array::<T>(new_cap).expect("Layout error");
        let new_ptr = self.a.alloc_layout(new_layout);
        unsafe {
            if self.cap > 0 && len > 0 {
                ptr::copy_nonoverlapping(self.ptr.as_ptr(), new_ptr.as_ptr() as *mut T, len);
            }
            self.ptr = NonNull::new(new_ptr.as_ptr() as *mut T).unwrap();
            self.cap = new_cap;
        }
    }
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
    pub fn push(&mut self, value: T) {
        if self.len == self.buf.cap {
            self.buf.reserve(self.len, 1);
        }
        unsafe {
            ptr::write(self.buf.ptr.as_ptr().add(self.len), value);
            self.len += 1;
        }
    }
}
impl<'bump, T: 'bump> Extend<T> for Vec<'bump, T> {
    fn extend<I: IntoIterator<Item = T>>(&mut self, iter: I) {
        let iter = iter.into_iter();
        self.buf.reserve(self.len, iter.size_hint().0);
        for t in iter {
            self.push(t);
        }
    }
}
impl<'bump, T> Drop for Vec<'bump, T> {
    fn drop(&mut self) {
        if self.buf.cap > 0 {
            unsafe {
                ptr::drop_in_place(ptr::slice_from_raw_parts_mut(
                    self.buf.ptr.as_ptr(),
                    self.len,
                ));
            }
        }
    }
}

// SECTION 2: PATCHED CODE

pub struct IntoIter<'bump, T> {
    phantom: PhantomData<&'bump [T]>,
    ptr: *const T,
    end: *const T,
}

impl<'bump, T: 'bump> IntoIterator for Vec<'bump, T> {
    type Item = T;
    type IntoIter = IntoIter<'bump, T>;

    #[inline]
    fn into_iter(self) -> IntoIter<'bump, T> {
        unsafe {
            let begin = self.buf.ptr.as_ptr();
            let end = if mem::size_of::<T>() == 0 {
                arith_offset(begin as *const i8, self.len as isize) as *const T
            } else {
                begin.add(self.len) as *const T
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

impl<'bump, T> Drop for IntoIter<'bump, T> {
    fn drop(&mut self) {
        self.for_each(drop);
    }
}
impl<'bump, T> Iterator for IntoIter<'bump, T> {
    type Item = T;
    #[inline]
    fn next(&mut self) -> Option<T> {
        unsafe {
            if self.ptr == self.end {
                None
            } else {
                let old = self.ptr;
                self.ptr = self.ptr.add(1);
                Some(ptr::read(old))
            }
        }
    }
}

// SECTION 3: PROOF-OF-CONCEPT (SAFE VERSION)

fn main() {
    // 1. Setup allocator and vector
    let bump = Bump::new();
    let mut vec = Vec::new_in(&bump);
    vec.extend([0x01u8; 32]);

    // 2. Create an iterator. `into_iter` now borrows `bump`.
    let into_iter = vec.into_iter();

    // 3. Not trigger BUG: Use the iterator while its borrow of `bump` is valid.
    // The for loop consumes the iterator, ending the borrow.
    for x in into_iter {
        print!("0x{:02x} ", x);
    }
    println!();

    // 4. Now that `into_iter` has been dropped, it is safe to drop `bump`.
    // This code now compiles and runs without memory safety issues.
    drop(bump);
}
