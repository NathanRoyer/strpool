use core::sync::atomic::{Ordering::*, AtomicPtr};
use core::{mem::{size_of, align_of}, ptr::copy};
use alloc::alloc::{Layout, alloc, dealloc};

use super::{PoolInner, PoolStr, hash::hash_str, string_from_len_u8};

const LARGE_STR_ADVANCE: usize = {
      size_of::<usize>()
    + size_of::<u64>()
    + size_of::<*const PoolInner<0>>() // P doesn't influence a pointer's size
    + size_of::<AtomicPtr<LargeStringHeader<0>>>()
};

#[repr(C)]
#[derive(Debug)]
pub(crate) struct LargeStringHeader<const P: usize> {
    len: usize,
    hash: u64,
    pool: *const PoolInner<P>,
    next: AtomicPtr<LargeStringHeader<P>>,
    len_zero: u8,
}

impl<const P: usize> PoolInner<P> {
    pub(crate) fn find_large(&self, string: &str) -> Option<PoolStr<P>> {
        let hash = hash_str(string);
        let pool_index = Self::index_from_hash(hash);
        let mut ptr = self.first_large_string[pool_index].load(Relaxed);

        while let Some(large_string) = unsafe { ptr.as_ref() } {
            if large_string.hash == hash {
                self.inc_ref_count();
                return Some(PoolStr::new(&large_string.len_zero));
            }

            ptr = large_string.next.load(Relaxed);
        }

        None
    }

    pub(crate) fn intern_large(&self, string: &str) -> PoolStr<P> {
        let hash = hash_str(string);
        let pool_index = Self::index_from_hash(hash);
        let mut ptr = &self.first_large_string[pool_index];
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
                let layout = large_string_layout::<P>(len);

                let large_string = unsafe {
                    let ptr = alloc(layout) as *mut LargeStringHeader<P>;

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

    pub(crate) fn debug_large_strings(&self, output: &mut core::fmt::DebugList) {
        for pool_index in 0..P {
            let mut ptr = self.first_large_string[pool_index].load(Relaxed);

            while let Some(large_string) = unsafe { ptr.as_ref() } {
                let string = string_from_len_u8::<P>(&large_string.len_zero);
                output.entry(&string);
                ptr = large_string.next.load(Relaxed);
            }
        }
    }
}

fn large_string_layout<const P: usize>(len: usize) -> Layout {
    // this currently wastes 3-7 bytes (todo)
    let size = size_of::<LargeStringHeader<P>>() + len;
    Layout::from_size_align(size, align_of::<usize>()).unwrap()
}

fn get_large_string<const P: usize>(len_u8_ptr: &u8) -> &LargeStringHeader<P> {
    unsafe {
        (len_u8_ptr as *const u8)
            .sub(LARGE_STR_ADVANCE)
            .cast::<LargeStringHeader<P>>()
            .as_ref()
            .unwrap()
    }
}

pub(crate) fn string_pool_ptr<const P: usize>(len_u8_ptr: &u8) -> *const PoolInner<P> {
    get_large_string(len_u8_ptr).pool
}

pub(crate) fn read_actual_string_len<const P: usize>(len_u8_ptr: &u8) -> usize {
    get_large_string::<P>(len_u8_ptr).len
}

pub(crate) fn deep_drop<const P: usize>(mut ptr: *const LargeStringHeader<P>) {
    while let Some(large_string) = unsafe { ptr.as_ref() } {
        let mut_ptr = (ptr as usize) as *mut u8;
        ptr = large_string.next.load(Relaxed);
        unsafe { dealloc(mut_ptr, large_string_layout::<P>(large_string.len)) };
    }
}
