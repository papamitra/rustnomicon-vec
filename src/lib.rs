#![feature(ptr_internals, alloc)]
use core::ptr::{self, Unique};
use std::alloc::{alloc, dealloc, realloc, Layout};
use std::mem;
use std::ops::{Deref, DerefMut};

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
}
