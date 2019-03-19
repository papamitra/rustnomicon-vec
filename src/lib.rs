#![feature(ptr_internals, alloc)]
use core::ptr::{self, Unique};
use std::alloc::{alloc, dealloc, realloc, Layout};
use std::mem;
use std::ops::{Deref, DerefMut};

struct RawValIter<T> {
    start: *const T,
    end: *const T,
}

impl<T> RawValIter<T> {
    // unsafe to construct because it has no associated lifetimes.
    // This is necessary to store a RawValIter in the same struct as
    // its actual allocation. OK since it's a private implementation
    // detail.
    unsafe fn new(slice: &[T]) -> Self {
        RawValIter {
            start: slice.as_ptr(),
            end: if slice.len() == 0 {
                // if `len = 0`, then this is not actually allocated memory.
                // Need to avoid offsetting because that will give wrong
                // information to LLVM via GEP.
                slice.as_ptr()
            } else {
                slice.as_ptr().offset(slice.len() as isize)
            },
        }
    }
}

pub struct Vec<T> {
    ptr: Unique<T>,
    cap: usize,
    len: usize,
}

impl<T> Vec<T> {
    fn new() -> Self {
        assert!(mem::size_of::<T>() != 0, "We're not ready to handle ZSTs");
        Vec {
            ptr: Unique::empty(),
            len: 0,
            cap: 0,
        }
    }
    fn grow(&mut self) {
        // this is all pretty delicate, so let's say it's all unsafe
        unsafe {
            let (new_cap, ptr) = if self.cap == 0 {
                let ptr = alloc(Layout::new::<T>());
                (1, ptr)
            } else {
                let elem_size = mem::size_of::<T>();
                // as an invariant, we can assume that `self.cap < isize::MAX`,
                // so this doesn't need to be checked.
                let new_cap = self.cap * 2;
                // Similarly this can't overflow due to previously allocating this
                let old_num_bytes = self.cap * elem_size;

                // check that the new allocation doesn't exceed `isize::MAX` at all
                // regardless of the actual size of the capacity. This combines the
                // `new_cap <= isize::MAX` and `new_num_bytes <= usize::MAX` checks
                // we need to make. We lose the ability to allocate e.g. 2/3rds of
                // the address space with a single Vec of i16's on 32-bit though.
                // Alas, poor Yorick -- I knew him, Horatio.
                assert!(
                    old_num_bytes <= (::std::isize::MAX as usize) / 2,
                    "capacity overflow"
                );

                let new_num_bytes = old_num_bytes * 2;
                let layout = Layout::from_size_align_unchecked(old_num_bytes, mem::align_of::<T>());
                let ptr = realloc(self.ptr.as_ptr() as *mut _, layout, new_num_bytes);
                (new_cap, ptr)
            };

            self.ptr = Unique::new_unchecked(ptr as *mut _);
            self.cap = new_cap;
        }
    }

    pub fn push(&mut self, elem: T) {
        if self.len == self.cap {
            self.grow();
        }

        unsafe {
            ptr::write(self.ptr.as_ptr().offset(self.len as isize), elem);
        }

        // Can't fail, we'll OOM first.
        self.len += 1;
    }

    pub fn pop(&mut self) -> Option<T> {
        if self.len == 0 {
            None
        } else {
            self.len -= 1;
            unsafe { Some(ptr::read(self.ptr.as_ptr().offset(self.len as isize))) }
        }
    }

    pub fn insert(&mut self, index: usize, elem: T) {
        // Note: `<=` because it's valid to insert after everything
        // which would be equivalent to push.
        assert!(index <= self.len, "index out of bounds");
        if self.cap == self.len {
            self.grow();
        }

        unsafe {
            if index < self.len {
                // ptr::copy(src, dest, len): "copy from source to dest len elems"
                ptr::copy(
                    self.ptr.as_ptr().offset(index as isize),
                    self.ptr.as_ptr().offset(index as isize + 1),
                    self.len - index,
                );
            }
            ptr::write(self.ptr.as_ptr().offset(index as isize), elem);
            self.len += 1;
        }
    }

    pub fn remove(&mut self, index: usize) -> T {
        // Note: `<` because it's *not* valid to remove after everything
        assert!(index < self.len, "index out of bounds");
        unsafe {
            self.len -= 1;
            let result = ptr::read(self.ptr.as_ptr().offset(index as isize));
            ptr::copy(
                self.ptr.as_ptr().offset(index as isize + 1),
                self.ptr.as_ptr().offset(index as isize),
                self.len - index,
            );
            result
        }
    }

    pub fn into_iter(self) -> IntoIter<T> {
        IntoIter {
            raw: unsafe { RawValIter::new(&self) },
            vec: self,
        }
    }
}

impl<T> Drop for Vec<T> {
    fn drop(&mut self) {
        if self.cap != 0 {
            while let Some(_) = self.pop() {}

            let align = mem::align_of::<T>();
            let elem_size = mem::size_of::<T>();
            let num_bytes = elem_size * self.cap;
            unsafe {
                dealloc(
                    self.ptr.as_ptr() as *mut _,
                    Layout::from_size_align_unchecked(num_bytes, align),
                );
            }
        }
    }
}

impl<T> Deref for Vec<T> {
    type Target = [T];
    fn deref(&self) -> &[T] {
        unsafe { ::std::slice::from_raw_parts(self.ptr.as_ptr(), self.len) }
    }
}

impl<T> DerefMut for Vec<T> {
    fn deref_mut(&mut self) -> &mut [T] {
        unsafe { ::std::slice::from_raw_parts_mut(self.ptr.as_ptr(), self.len) }
    }
}

pub struct IntoIter<T> {
    vec: Vec<T>,
    raw: RawValIter<T>,
}

impl<T> Iterator for IntoIter<T> {
    type Item = T;
    fn next(&mut self) -> Option<T> {
        if self.raw.start == self.raw.end {
            None
        } else {
            unsafe {
                self.raw.end = self.raw.end.offset(-1);
                Some(ptr::read(self.raw.end))
            }
        }
    }
}

impl<T> DoubleEndedIterator for IntoIter<T> {
    fn next_back(&mut self) -> Option<T> {
        if self.raw.start == self.raw.end {
            None
        } else {
            unsafe {
                self.raw.start = self.raw.start.offset(1);
                Some(ptr::read(self.raw.start.offset(-1)))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Vec;

    #[test]
    fn test() {
        let mut v = Vec::<i32>::new();
        assert_eq!(v.pop(), None);

        v.push(1);
        v.push(2);
        v.push(3);
        assert_eq!(v.pop(), Some(3));
        assert_eq!(v.pop(), Some(2));
        assert_eq!(v.pop(), Some(1));
        assert_eq!(v.pop(), None);
    }

    #[test]
    fn deref() {
        let mut v = Vec::<i32>::new();

        assert_eq!(v[..], []);

        v.push(1);
        v.push(2);
        v.push(3);

        assert_eq!(v[1..][0], 2);
        v[..][0] = 4;
        assert_eq!(v[0], 4);
    }

    #[test]
    fn insert_remove() {
        let mut v = Vec::<i32>::new();

        v.insert(0, 1);
        assert_eq!(v[0], 1);

        v.insert(0, 2);
        assert_eq!(v[..], [2, 1]);

        v.remove(0);
        assert_eq!(v[..], [1]);
    }

    #[test]
    fn into_iter() {
        let mut v = Vec::new();

        v.push(1);
        v.push(2);
        v.push(3);

        let mut iter = v.into_iter();

        assert_eq!(iter.next(), Some(3));
        assert_eq!(iter.next_back(), Some(1));
        assert_eq!(iter.next(), Some(2));
        assert_eq!(iter.next(), None);
    }
}
