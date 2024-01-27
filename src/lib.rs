#![doc = include_str!("../README.md")]
#![no_std]

extern crate alloc;

use core::sync::atomic::{Ordering::*, AtomicPtr, AtomicUsize};
use core::{slice::from_raw_parts, str::from_utf8, ops::Deref};
use alloc::boxed::Box;

mod hash;
mod small;
mod large;
mod traits;

struct PoolInner {
    ref_count: AtomicUsize,
    first_page: AtomicPtr<small::Page>,
    first_large_string: AtomicPtr<large::LargeStringHeader>,
}

/// String pool
pub struct Pool {
    inner: *const PoolInner,
}

/// `&str` equivalent
pub struct PoolStr {
    len_ptr: *const u8,
}

impl PoolInner {
    /// sets the ref_count to 1
    const fn new() -> Self {
        Self {
            ref_count: AtomicUsize::new(1),
            first_page: AtomicPtr::new(0 as _),
            first_large_string: AtomicPtr::new(0 as _),
        }
    }

    fn inc_ref_count(&self) {
        self.ref_count.fetch_add(1, SeqCst);
    }

    // returns true if this was the last ref
    fn dec_ref_count(&self) -> bool {
        self.ref_count.fetch_sub(1, SeqCst) == 1
    }

    fn find(&self, string: &str) -> Option<PoolStr> {
        match string.len() {
            0 => Some(PoolStr::empty()),
            1..=126 => self.find_small(string),
            _ => self.find_large(string),
        }
    }

    fn intern(&self, string: &str) -> PoolStr {
        match string.len() {
            0 => PoolStr::empty(),
            1..=126 => self.intern_small(string),
            _ => self.intern_large(string),
        }
    }
}

impl Pool {
    /// Creates a new pool
    pub fn new() -> Self {
        let boxed = Box::new(PoolInner::new());
        // ref_count is set to one
        Self {
            inner: Box::into_raw(boxed),
        }
    }

    /// Get a handle to a special static string pool which is never dropped, like its content.
    pub fn get_static_pool() -> Self {
        // this must never be dropped
        static STATIC_POOL: PoolInner = PoolInner::new();

        STATIC_POOL.inc_ref_count();
        // now the ref count is at least 2
        // should prevent it from being dropped

        Self {
            inner: &STATIC_POOL as _,
        }
    }

    fn inner(&self) -> &PoolInner {
        unsafe { self.inner.as_ref() }.unwrap()
    }

    /// Locates an existing [`PoolStr`]
    pub fn find(&self, string: &str) -> Option<PoolStr> {
        self.inner().find(string)
    }

    /// Creates a new [`PoolStr`]
    pub fn intern(&self, string: &str) -> PoolStr {
        self.inner().intern(string)
    }
}

impl PoolStr {
    fn new(len: &u8) -> Self {
        Self {
            len_ptr: len as *const u8,
        }
    }

    pub fn empty() -> Self {
        Self {
            len_ptr: 0 as *const u8,
        }
    }

    fn pool_ptr(&self) -> Option<*const PoolInner> {
        let len = unsafe { self.len_ptr.as_ref()? };

        let pool_ptr = match *len {
            0 => large::string_pool_ptr(len),
            _ => small::string_pool_ptr(len),
        };

        Some(pool_ptr)
    }
}

impl Deref for PoolStr {
    type Target = str;
    fn deref(&self) -> &Self::Target {
        let len = unsafe { self.len_ptr.as_ref() };
        if let Some(len_u8) = len {
            let len = match *len_u8 {
                0 => large::read_actual_string_len(len_u8),
                l => l as usize,
            };

            let start = unsafe { self.len_ptr.add(1) };
            let slice = unsafe { from_raw_parts(start, len) };
            from_utf8(slice).unwrap()
        } else {
            ""
        }
    }
}

fn deep_drop_pool(pool_ptr: *const PoolInner) {
    let pool = unsafe { pool_ptr.as_ref() }.unwrap();

    large::deep_drop(pool.first_large_string.load(Relaxed));
    small::deep_drop(pool.first_page.load(Relaxed));

    let mut_ptr = (pool_ptr as usize) as *mut PoolInner;
    drop(unsafe { Box::from_raw(mut_ptr) });
}

impl Drop for PoolStr {
    fn drop(&mut self) {
        if let Some(pool_ptr) = self.pool_ptr() {
            let pool = unsafe { pool_ptr.as_ref() }.unwrap();
            if pool.dec_ref_count() {
                deep_drop_pool(pool_ptr);
            }
        }
    }
}

impl Drop for Pool {
    fn drop(&mut self) {
        let pool = unsafe { self.inner.as_ref() }.unwrap();
        if pool.dec_ref_count() {
            deep_drop_pool(self.inner);
        }
    }
}

impl Clone for PoolStr {
    fn clone(&self) -> Self {
        if let Some(pool_ptr) = self.pool_ptr() {
            unsafe { pool_ptr.as_ref() }.unwrap().inc_ref_count();
        }

        Self {
            len_ptr: self.len_ptr,
        }
    }
}

impl Clone for Pool {
    fn clone(&self) -> Self {
        self.inner().inc_ref_count();
        Self {
            inner: self.inner,
        }
    }
}

// Safe because of proper atomic operations
unsafe impl Send for PoolStr {}
unsafe impl Sync for PoolStr {}
unsafe impl Send for Pool {}
unsafe impl Sync for Pool {}

#[test]
fn edge_case_1() {
    let small_string_1 = "0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000001 000000000000000000000000";
    let small_string_2 = "0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000002 000000000000000000000000";
    let small_string_3 = "0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000003 000000000000000000000000";
    let small_string_4 = "0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000004 000000000000000000000000";
    let small_string_5 = "0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000005 000000000000000000000000";
    let small_string_6 = "0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000006 000000000000000000000000";
    let small_string_7 = "0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000007 000000000000000000000000";
    let small_string_8 = "0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000008 000000000000000000000000";
    // now we have 8*(1+125) bytes taken = one page;
    let small_string_9 = "yikes";

    let pool = Pool::new();
    pool.intern(small_string_1);
    pool.intern(small_string_2);
    pool.intern(small_string_3);
    pool.intern(small_string_4);
    pool.intern(small_string_5);
    pool.intern(small_string_6);
    pool.intern(small_string_7);
    pool.intern(small_string_8);

    // would previously fail
    pool.intern(small_string_9);
}

#[test]
fn various_tests() {
    let small_string_1 = "gjnberguieriu";
    let small_string_2 = "krjgegyhergyeurgyeyrg";
    let small_string_3 = "ryjtyjty";
    let large_string_1 = "gjnberguieriugjnberguieriugjnberguieriugjnberguieriugjnberguieriugjnberguieriugjnberguieriugjnberguieriugjnberguieriugjnberguieriugjnberguieriugjnberguieriugjnberguieriugjnberguieriugjnberguieriugjnberguieriugjnberguieriugjnberguieriu";
    let large_string_2 = "rgyeurgyeyrgrgyeurgyeyrgrgyeurgyeyrgrgyeurgyeyrgrgyeurgyeyrgrgyeurgyeyrgrgyeurgyeyrgrgyeurgyeyrgrgyeurgyeyrgrgyeurgyeyrgrgyeurgyeyrgrgyeurgyeyrg";
    let large_string_3 = "rjuebuinh99€€rjuebuinh99€€rjuebuinh99€€rjuebuinh99€€rjuebuinh99€€rjuebuinh99€€rjuebuinh99€€rjuebuinh99€€rjuebuinh99€€rjuebuinh99€€";

    let pool = Pool::new();

    // check that they're not present initially
    assert_eq!(pool.find(""), Some(PoolStr::empty()));
    assert_eq!(pool.find(small_string_1), None);
    assert_eq!(pool.find(large_string_1), None);
    assert_eq!(pool.find(small_string_2), None);
    assert_eq!(pool.find(large_string_2), None);
    assert_eq!(pool.find(small_string_3), None);
    assert_eq!(pool.find(large_string_3), None);

    // insert them
    // check that they're present

    pool.intern(large_string_1);
    assert_eq!(&*pool.find(large_string_1).unwrap(), large_string_1);

    pool.intern(small_string_1);
    assert_eq!(&*pool.find(small_string_1).unwrap(), small_string_1);

    pool.intern(large_string_2);
    assert_eq!(&*pool.find(large_string_2).unwrap(), large_string_2);

    pool.intern(small_string_2);
    assert_eq!(&*pool.find(small_string_2).unwrap(), small_string_2);

    pool.intern(large_string_3);
    assert_eq!(&*pool.find(large_string_3).unwrap(), large_string_3);

    pool.intern(small_string_3);
    assert_eq!(&*pool.find(small_string_3).unwrap(), small_string_3);

    // check that they're still present
    assert_eq!(&*pool.find(large_string_1).unwrap(), large_string_1);
    assert_eq!(&*pool.find(small_string_1).unwrap(), small_string_1);
    assert_eq!(&*pool.find(large_string_2).unwrap(), large_string_2);
    assert_eq!(&*pool.find(small_string_2).unwrap(), small_string_2);
    assert_eq!(&*pool.find(large_string_3).unwrap(), large_string_3);
    assert_eq!(&*pool.find(small_string_3).unwrap(), small_string_3);
}
