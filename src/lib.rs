#![doc = include_str!("../README.md")]
#![no_std]

extern crate alloc;

use alloc::{alloc::{Layout, alloc, dealloc}, boxed::Box};
use core::{mem::{size_of, align_of, drop}, ptr::copy, ops::Deref};
use core::sync::atomic::{Ordering::*, AtomicPtr, AtomicUsize, AtomicU8};
use core::{slice::from_raw_parts, str::from_utf8};

mod hash;
mod traits;

const PAGE_SIZE: usize = 1024;
const PAGE_ALIGN_MASK: usize = !(PAGE_SIZE - 1);
const PAGE_CAPACITY: usize = PAGE_SIZE - size_of::<PageHeader>();
const PAGE_LAYOUT: Layout = unsafe { Layout::from_size_align_unchecked(PAGE_SIZE, PAGE_SIZE) };
const NOT_READY: u8 = 0x80;
const LEN_MASK: u8 = 0x7f;
const LARGE_STR_ADVANCE: usize = {
      size_of::<usize>()
    + size_of::<u64>()
    + size_of::<*const PoolInner>()
    + size_of::<AtomicPtr<LargeStringHeader>>()
};

struct PoolInner {
    ref_count: AtomicUsize,
    first_page: AtomicPtr<Page>,
    first_large_string: AtomicPtr<LargeStringHeader>,
}

/// String pool
pub struct Pool {
    inner: *const PoolInner,
}

struct PageHeader {
    next: AtomicPtr<Page>,
    pool: *const PoolInner,
}

struct Page {
    header: PageHeader,
    entries: [u8; PAGE_CAPACITY],
}

#[repr(C)]
#[derive(Debug)]
struct LargeStringHeader {
    len: usize,
    hash: u64,
    pool: *const PoolInner,
    next: AtomicPtr<LargeStringHeader>,
    len_zero: u8,
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

    fn find_1_127(&self, string: &str) -> Option<PoolStr> {
        let slice = string.as_bytes();
        let mut ptr = self.first_page.load(Relaxed);

        while let Some(page) = unsafe { ptr.as_ref() } {
            let mut i = 0;
            loop {
                let (len, ready) = get_len(&page.entries[i]);
                i += 1;

                if ready {
                    if len == slice.len() {
                        let j = i + len;
                        if &page.entries[i..j] == slice {
                            self.inc_ref_count();
                            return Some(PoolStr::new(&page.entries[i - 1]));
                        }
                    } else if len == 0 {
                        break;
                    }
                }

                i += len;
            }

            ptr = page.header.next.load(Relaxed);
        }

        None
    }

    fn find_127_n(&self, string: &str) -> Option<PoolStr> {
        let hash = hash::hash_str(string);
        let mut ptr = self.first_large_string.load(Relaxed);

        while let Some(large_string) = unsafe { ptr.as_ref() } {
            if large_string.hash == hash {
                self.inc_ref_count();
                return Some(PoolStr::new(&large_string.len_zero));
            }

            ptr = large_string.next.load(Relaxed);
        }

        None
    }

    fn find(&self, string: &str) -> Option<PoolStr> {
        match string.len() {
            0 => Some(PoolStr::empty()),
            1..=126 => self.find_1_127(string),
            _ => self.find_127_n(string),
        }
    }

    fn intern_1_127(&self, string: &str) -> PoolStr {
        let slice = string.as_bytes();
        let mut ptr = &self.first_page;

        loop {
            while let Some(page) = unsafe { ptr.load(Relaxed).as_mut() } {
                let mut i = 0;
                loop {
                    let (len, ready) = get_len(&page.entries[i]);
                    let s = i + 1;

                    if ready {
                        if len == slice.len() {
                            let e = s + len;
                            if &page.entries[s..e] == slice {
                                self.inc_ref_count();
                                return PoolStr::new(&page.entries[i]);
                            }
                        } else if len == 0 {
                            // this entry is available
                            if s + slice.len() <= PAGE_CAPACITY {
                                // there is enough space, meaning `string` isn't
                                // present in next pages (except if another thread
                                // was trying to intern the same string at the same
                                // moment, but this is highly unlikely + doesn't
                                // affect the external behaviour, it's only less
                                // efficient).
                                let slice_len = slice.len();
                                let len = slice_len as u8 | NOT_READY;

                                if try_set_len(&page.entries[i], 0, len) {
                                    // the NOT_READY flag is set, we can copy the bytes
                                    let j = s + slice_len;
                                    page.entries[s..j].copy_from_slice(slice);

                                    // remove NOT_READY flag
                                    assert!(try_set_len(&page.entries[i], len, len & LEN_MASK));

                                    self.inc_ref_count();
                                    return PoolStr::new(&page.entries[i]);
                                } else {
                                    // retry this entry
                                    continue;
                                }
                            } else {
                                // to next page
                                break;
                            }
                        }
                    }

                    i = s + len;
                }

                ptr = &page.header.next;
            }

            let backup = ptr;

            let empty_page = unsafe {
                let ptr = alloc(PAGE_LAYOUT) as *mut Page;

                let page = ptr.as_mut().unwrap();
                page.header = PageHeader {
                    next: AtomicPtr::new(0 as _),
                    pool: self as _,
                };
                page.entries.fill(0);

                ptr
            };

            // need to append a page
            loop {
                match ptr.compare_exchange(0 as _, empty_page, SeqCst, Relaxed) {
                    Ok(_) => break,
                    Err(new_page_ptr) => {
                        // another thread appended a new page too
                        // append the page we allocated to that new one
                        let page = unsafe { new_page_ptr.as_ref() }.unwrap();
                        ptr = &page.header.next;
                    },
                }
            }

            // restart the search from the first unexplored page
            ptr = backup;
        }
    }

    fn intern_127_n(&self, string: &str) -> PoolStr {
        let hash = hash::hash_str(string);
        let mut ptr = &self.first_large_string;
        let mut allocation = None;

        loop {
            while let Some(large_string) = unsafe { ptr.load(Relaxed).as_ref() } {
                if large_string.hash == hash {
                    if let Some((new, layout)) = allocation {
                        unsafe { dealloc(new as _, layout) };
                    }
                    self.inc_ref_count();
                    return PoolStr::new(&large_string.len_zero);
                }

                ptr = &large_string.next;
            }

            let large_string = if let Some((large_string, _)) = allocation {
                large_string
            } else {
                let len = string.len();
                let layout = large_string_layout(len);

                let large_string = unsafe {
                    let ptr = alloc(layout) as *mut LargeStringHeader;

                    let mut_ref = ptr.as_mut().unwrap();
                    *mut_ref = LargeStringHeader {
                        len,
                        hash,
                        pool: self as _,
                        next: AtomicPtr::new(0 as _),
                        len_zero: 0,
                    };

                    // copy the string bytes
                    let dst = (&mut mut_ref.len_zero as *mut u8).add(1);
                    copy(string.as_ptr(), dst, len);

                    ptr
                };

                allocation = Some((large_string, layout));

                large_string
            };

            // need to append an entry
            if ptr.compare_exchange(0 as _, large_string, SeqCst, Relaxed).is_ok() {
                let ls_ref = unsafe { large_string.as_ref() }.unwrap();
                self.inc_ref_count();
                break PoolStr::new(&ls_ref.len_zero);
            }

            // if it failed, the search restarts at the
            // large_string that was appended by another
            // thread.
        }
    }

    fn intern(&self, string: &str) -> PoolStr {
        match string.len() {
            0 => PoolStr::empty(),
            1..=126 => self.intern_1_127(string),
            _ => self.intern_127_n(string),
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
        if !self.len_ptr.is_null() {
            let len = unsafe { self.len_ptr.read() };
            if len == 0 {
                let large_string = unsafe {
                    self.len_ptr
                        .sub(LARGE_STR_ADVANCE)
                        .cast::<LargeStringHeader>()
                        .as_ref()
                        .unwrap()
                };

                Some(large_string.pool)
            } else {
                let addr = self.len_ptr as usize;
                let page_ptr = (addr & PAGE_ALIGN_MASK) as *const Page;
                let page = unsafe { page_ptr.as_ref() }.unwrap();
                Some(page.header.pool)
            }
        } else {
            None
        }
    }
}

impl Deref for PoolStr {
    type Target = str;
    fn deref(&self) -> &Self::Target {
        if !self.len_ptr.is_null() {
            let mut len = unsafe { self.len_ptr.read() } as usize;

            // this is unlikely but `intrinsics::unlikely` is unstable
            if len == 0 {
                let large_string = unsafe {
                    self.len_ptr
                        .sub(LARGE_STR_ADVANCE)
                        .cast::<LargeStringHeader>()
                        .as_ref()
                        .unwrap()
                };

                len = large_string.len;
            }

            let start = unsafe { self.len_ptr.add(1) };
            let slice = unsafe { from_raw_parts(start, len) };
            from_utf8(slice).unwrap()
        } else {
            ""
        }
    }
}

fn large_string_layout(len: usize) -> Layout {
    // this currently wastes 3-7 bytes (todo)
    let size = size_of::<LargeStringHeader>() + len;
    Layout::from_size_align(size, align_of::<usize>()).unwrap()
}

fn deep_drop_pool(pool_ptr: *const PoolInner) {
    let pool = unsafe { pool_ptr.as_ref() }.unwrap();

    let mut ptr = pool.first_large_string.load(Relaxed);
    while let Some(large_string) = unsafe { ptr.as_ref() } {
        let mut_ptr = (ptr as usize) as *mut u8;
        ptr = large_string.next.load(Relaxed);
        unsafe { dealloc(mut_ptr, large_string_layout(large_string.len)) };
    }

    let mut ptr = pool.first_page.load(Relaxed);
    while let Some(page) = unsafe { ptr.as_ref() } {
        let mut_ptr = (ptr as usize) as *mut u8;
        ptr = page.header.next.load(Relaxed);
        unsafe { dealloc(mut_ptr, PAGE_LAYOUT) };
    }

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

// returns (bytes_to_skip, ready)
fn get_len(len: &u8) -> (usize, bool) {
    let len = unsafe {
        (len as *const u8)
            .cast::<AtomicU8>()
            .as_ref()
            .unwrap()
    }.load(Relaxed);

    match len & NOT_READY {
        0 => (len as usize, true),
        _ => ((len & LEN_MASK) as usize, false),
    }
}

fn try_set_len(len: &u8, prev: u8, new: u8) -> bool {
    let len = unsafe {
        (len as *const u8)
            .cast::<AtomicU8>()
            .as_ref()
            .unwrap()
    };

    len.compare_exchange(prev, new, SeqCst, Relaxed).is_ok()
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
    assert_eq!(pool.find(""), Some(PoolStr::empty()));
    assert_eq!(pool.find(small_string_1), None);
    assert_eq!(pool.find(large_string_1), None);
    assert_eq!(pool.find(small_string_2), None);
    assert_eq!(pool.find(large_string_2), None);
    assert_eq!(pool.find(small_string_3), None);
    assert_eq!(pool.find(large_string_3), None);

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

    assert_eq!(&*pool.find(large_string_1).unwrap(), large_string_1);
    assert_eq!(&*pool.find(small_string_1).unwrap(), small_string_1);
    assert_eq!(&*pool.find(large_string_2).unwrap(), large_string_2);
    assert_eq!(&*pool.find(small_string_2).unwrap(), small_string_2);
    assert_eq!(&*pool.find(large_string_3).unwrap(), large_string_3);
    assert_eq!(&*pool.find(small_string_3).unwrap(), small_string_3);
}
