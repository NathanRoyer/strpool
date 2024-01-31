#![doc = include_str!("../README.md")]
#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(test)]
extern crate std;

extern crate alloc;

use core::sync::atomic::{Ordering::*, AtomicPtr, AtomicUsize};
use core::{slice::from_raw_parts, str::from_utf8, ops::Deref};
use alloc::boxed::Box;

mod hash;
mod small;
mod large;
mod traits;

#[cfg(feature = "std")]
mod static_pool;

#[cfg(feature = "std")]
pub use static_pool::PoolCell;

#[cfg(feature = "serde")]
pub mod serde;

struct PoolInner<const P: usize> {
    ref_count: AtomicUsize,
    first_page: [AtomicPtr<small::Page<P>>; P],
    first_large_string: [AtomicPtr<large::LargeStringHeader<P>>; P],
}

/// String pool
pub struct Pool<const P: usize = 1> {
    inner: *const PoolInner<P>,
}

/// `&str` equivalent
pub struct PoolStr<const P: usize> {
    len_ptr: *const u8,
    _phantom: [(); P],
}

impl<const P: usize> PoolInner<P> {
    const FIRST_PAGE_NEW: AtomicPtr<small::Page<P>> = AtomicPtr::new(0 as _);
    const FIRST_LS_NEW: AtomicPtr<large::LargeStringHeader<P>> = AtomicPtr::new(0 as _);
    const NEW: Self = Self {
        ref_count: AtomicUsize::new(1),
        first_page: [Self::FIRST_PAGE_NEW; P],
        first_large_string: [Self::FIRST_LS_NEW; P],
    };

    fn index_from_hash(hash: u64) -> usize {
        assert!(P.is_power_of_two());
        (hash as usize) & (P - 1)
    }

    fn index_for(string: &str) -> usize {
        match P {
            0 => 0,
            _ => Self::index_from_hash(hash::hash_str(string)),
        }
    }

    fn inc_ref_count(&self) {
        self.ref_count.fetch_add(1, SeqCst);
    }

    // returns true if this was the last ref
    fn dec_ref_count(&self) -> bool {
        self.ref_count.fetch_sub(1, SeqCst) == 1
    }

    fn find(&self, string: &str) -> Option<PoolStr<P>> {
        match string.len() {
            0 => Some(PoolStr::empty()),
            1..=126 => self.find_small(string),
            _ => self.find_large(string),
        }
    }

    fn intern(&self, string: &str) -> PoolStr<P> {
        match string.len() {
            0 => PoolStr::empty(),
            1..=126 => self.intern_small(string),
            _ => self.intern_large(string),
        }
    }
}

impl<const P: usize> Pool<P> {
    /// Creates a new pool
    pub fn new() -> Self {
        assert!(P.is_power_of_two());

        // ref_count is set to one in each inner pool
        let boxed = Box::new(PoolInner::NEW);
        Self {
            inner: Box::into_raw(boxed),
        }
    }

    /// Deprecated; same as `Self::new()`
    #[deprecated]
    pub fn get_static_pool() -> Self {
        Self::new()
    }

    fn inner(&self) -> &PoolInner<P> {
        unsafe { self.inner.as_ref() }.unwrap()
    }

    /// Locates an existing [`PoolStr`]
    pub fn find(&self, string: &str) -> Option<PoolStr<P>> {
        self.inner().find(string)
    }

    /// Creates a new [`PoolStr`]
    pub fn intern(&self, string: &str) -> PoolStr<P> {
        self.inner().intern(string)
    }
}

impl<const P: usize> PoolStr<P> {
    fn new(len: &u8) -> Self {
        Self {
            len_ptr: len as *const u8,
            _phantom: [(); P],
        }
    }

    pub fn empty() -> Self {
        Self {
            len_ptr: 0 as *const u8,
            _phantom: [(); P],
        }
    }

    fn pool_ptr(&self) -> Option<*const PoolInner<P>> {
        let len = unsafe { self.len_ptr.as_ref()? };

        let pool_ptr = match *len {
            0 => large::string_pool_ptr(len),
            _ => small::string_pool_ptr(len),
        };

        Some(pool_ptr)
    }
}

// this function assumes that len_u8_ref points
// to a finished/ready slot, for small strings
fn string_from_len_u8<const P: usize>(len_u8_ref: &u8) -> &str {
    let len = match *len_u8_ref {
        0 => large::read_actual_string_len::<P>(len_u8_ref),
        l => l as usize,
    };

    let len_u8_ptr = len_u8_ref as *const u8;
    let start = unsafe { len_u8_ptr.add(1) };
    let slice = unsafe { from_raw_parts(start, len) };
    from_utf8(slice).unwrap()
}

impl<const P: usize> Deref for PoolStr<P> {
    type Target = str;
    fn deref(&self) -> &Self::Target {
        match unsafe { self.len_ptr.as_ref() } {
            Some(len_u8_ptr) => string_from_len_u8::<P>(len_u8_ptr),
            None => "",
        }
    }
}

fn deep_drop_pool<const P: usize>(pool_ptr: *const PoolInner<P>) {
    let pool = unsafe { pool_ptr.as_ref() }.unwrap();

    for pool_index in 0..P {
        large::deep_drop(pool.first_large_string[pool_index].load(Relaxed));
        small::deep_drop(pool.first_page[pool_index].load(Relaxed));
    }

    let mut_ptr = (pool_ptr as usize) as *mut PoolInner<P>;
    drop(unsafe { Box::from_raw(mut_ptr) });
}

impl<const P: usize> Drop for PoolStr<P> {
    fn drop(&mut self) {
        if let Some(pool_ptr) = self.pool_ptr() {
            let pool = unsafe { pool_ptr.as_ref() }.unwrap();
            if pool.dec_ref_count() {
                deep_drop_pool(pool_ptr);
            }
        }
    }
}

impl<const P: usize> Drop for Pool<P> {
    fn drop(&mut self) {
        let pool = unsafe { self.inner.as_ref() }.unwrap();
        if pool.dec_ref_count() {
            deep_drop_pool(self.inner);
        }
    }
}

impl<const P: usize> Clone for PoolStr<P> {
    fn clone(&self) -> Self {
        if let Some(pool_ptr) = self.pool_ptr() {
            unsafe { pool_ptr.as_ref() }.unwrap().inc_ref_count();
        }

        Self {
            len_ptr: self.len_ptr,
            _phantom: [(); P],
        }
    }
}

impl<const P: usize> Clone for Pool<P> {
    fn clone(&self) -> Self {
        self.inner().inc_ref_count();
        Self {
            inner: self.inner,
        }
    }
}

// Safe because of proper atomic operations
unsafe impl<const P: usize> Send for PoolStr<P> {}
unsafe impl<const P: usize> Sync for PoolStr<P> {}
unsafe impl<const P: usize> Send for Pool<P> {}
unsafe impl<const P: usize> Sync for Pool<P> {}

struct PoolPages<'a, const P: usize>(&'a PoolInner<P>);

impl<'a, const P: usize> core::fmt::Debug for PoolPages<'a, P> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let mut output = f.debug_list();
        self.0.debug_pages(&mut output);
        output.finish()
    }
}

struct PoolLargeStrings<'a, const P: usize>(&'a PoolInner<P>);

impl<'a, const P: usize> core::fmt::Debug for PoolLargeStrings<'a, P> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let mut output = f.debug_list();
        self.0.debug_large_strings(&mut output);
        output.finish()
    }
}

impl<const P: usize> core::fmt::Debug for Pool<P> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let inner = self.inner();
        let mut output = f.debug_struct("Pool");
        output.field("reference_count", &inner.ref_count);
        output.field("small_string_pages", &PoolPages(inner));
        output.field("largs_strings", &PoolLargeStrings(inner));
        output.finish()
    }
}

#[test]
fn edge_case_1() {
    let small_string_1 = "00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000001";
    let small_string_2 = "00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000002";
    let small_string_3 = "00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000003";
    let small_string_4 = "00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000004";
    let small_string_5 = "00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000005";
    let small_string_6 = "00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000006";
    let small_string_7 = "00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000007";
    let small_string_8 = "00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000008";
    // now we have 8*(1+125) bytes taken = one page;
    let small_string_9 = "yikes";

    let pool: Pool<1> = Pool::new();
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

    std::println!("EDGE CASE: {:#?}", pool);
}

#[test]
fn various_tests() {
    let small_string_1 = "gjnberguieriu";
    let small_string_2 = "krjgegyhergyeurgyeyrg";
    let small_string_3 = "ryjtyjty";
    let large_string_1 = "gjnberguieriugjnberguieriugjnberguieriugjnberguieriugjnberguieriugjnberguieriugjnberguieriugjnberguieriugjnberguieriugjnberguieriugjnberguieriugjnberguieriugjnberguieriugjnberguieriugjnberguieriugjnberguieriugjnberguieriugjnberguieriu";
    let large_string_2 = "rgyeurgyeyrgrgyeurgyeyrgrgyeurgyeyrgrgyeurgyeyrgrgyeurgyeyrgrgyeurgyeyrgrgyeurgyeyrgrgyeurgyeyrgrgyeurgyeyrgrgyeurgyeyrgrgyeurgyeyrgrgyeurgyeyrg";
    let large_string_3 = "rjuebuinh99€€rjuebuinh99€€rjuebuinh99€€rjuebuinh99€€rjuebuinh99€€rjuebuinh99€€rjuebuinh99€€rjuebuinh99€€rjuebuinh99€€rjuebuinh99€€";

    let pool: Pool<4> = Pool::new();
    std::println!("BEFORE VARIOUS TESTS: {:#?}", pool);

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

    std::println!("AFTER VARIOUS TESTS: {:#?}", pool);
}
