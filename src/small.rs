use core::sync::atomic::{Ordering::*, AtomicPtr, AtomicU8};
use alloc::alloc::{Layout, alloc, dealloc};
use core::mem::size_of;

use super::{PoolInner, PoolStr, string_from_len_u8};

const PAGE_SIZE: usize = 1024;
const PAGE_ALIGN_MASK: usize = !(PAGE_SIZE - 1);
const PAGE_CAPACITY: usize = PAGE_SIZE - size_of::<PageHeader>();
const PAGE_LAYOUT: Layout = unsafe { Layout::from_size_align_unchecked(PAGE_SIZE, PAGE_SIZE) };
const NOT_READY: u8 = 0x80;
const LEN_MASK: u8 = 0x7f;

#[derive(Debug)]
struct PageHeader {
    next: AtomicPtr<Page>,
    pool: *const PoolInner,
}

pub(crate) struct Page {
    header: PageHeader,
    entries: [u8; PAGE_CAPACITY],
}

impl Page {
    fn find(&self, slice: &[u8]) -> Option<PoolStr> {
        let mut i = 0;
        while i < PAGE_CAPACITY {
            let (len, ready) = read_atomic_slot_len(&self.entries[i]);
            // skip len byte:
            i += 1;

            if ready {
                if len == slice.len() {
                    let j = i + len;
                    if &self.entries[i..j] == slice {
                        return Some(PoolStr::new(&self.entries[i - 1]));
                    }
                } else if len == 0 {
                    break;
                }
            }

            // skip string bytes:
            i += len;
        }

        None
    }

    fn try_intern(&mut self, slice: &[u8]) -> Option<PoolStr> {
        let mut i = 0;
        while i < PAGE_CAPACITY {
            let (len, ready) = read_atomic_slot_len(&self.entries[i]);
            let s = i + 1;

            if ready {
                if len == slice.len() {
                    // same length... does this entry correspond to an equal string?
                    let e = s + len;
                    if &self.entries[s..e] == slice {
                        // yes; we'll re-use it then
                        return Some(PoolStr::new(&self.entries[i]));
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

                        if try_set_len(&self.entries[i], 0, len) {
                            // the NOT_READY flag is set, we can copy the bytes
                            let j = s + slice_len;
                            self.entries[s..j].copy_from_slice(slice);

                            // remove NOT_READY flag
                            assert!(try_set_len(&self.entries[i], len, len & LEN_MASK));

                            return Some(PoolStr::new(&self.entries[i]));
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

        None
    }

    // used by Debug for Page
    fn debug_slot(&self, len_index: usize) -> Option<(Option<&str>, usize)> {
        if len_index < PAGE_CAPACITY {
            let len_u8_ref = &self.entries[len_index];
            let (len, ready) = read_atomic_slot_len(len_u8_ref);

            if len != 0 {
                let string = match ready {
                    true => Some(string_from_len_u8(len_u8_ref)),
                    false => None,
                };

                Some((string, len_index + 1 + len))
            } else {
                None
            }
        } else {
            None
        }
    }
}

impl PoolInner {
    pub(crate) fn find_small(&self, string: &str) -> Option<PoolStr> {
        let slice = string.as_bytes();
        let mut ptr = self.first_page.load(Relaxed);

        while let Some(page) = unsafe { ptr.as_ref() } {
            if let Some(pool_str) = page.find(slice) {
                self.inc_ref_count();
                return Some(pool_str);
            }

            ptr = page.header.next.load(Relaxed);
        }

        None
    }

    pub(crate) fn intern_small(&self, string: &str) -> PoolStr {
        let slice = string.as_bytes();
        let mut page_ptr_ref = &self.first_page;

        loop {
            while let Some(page) = unsafe { page_ptr_ref.load(Relaxed).as_mut() } {
                if let Some(pool_str) = page.try_intern(slice) {
                    self.inc_ref_count();
                    return pool_str;
                }

                page_ptr_ref = &page.header.next;
            }

            let last_searched_page_next_ptr_ref = page_ptr_ref;

            let new_page_ptr = unsafe {
                let new_page_ptr = alloc(PAGE_LAYOUT) as *mut Page;

                let new_page = new_page_ptr.as_mut().unwrap();
                new_page.header = PageHeader {
                    next: AtomicPtr::new(0 as _),
                    pool: self as _,
                };
                new_page.entries.fill(0);

                new_page_ptr
            };

            // need to append a page
            loop {
                // (re)try to append it
                match page_ptr_ref.compare_exchange(0 as _, new_page_ptr, SeqCst, Relaxed) {
                    Ok(_) => break,
                    Err(new_page_ptr) => {
                        // another thread appended a new page before we could do it
                        // try to append the page we allocated to that new one
                        let page = unsafe { new_page_ptr.as_ref() }.unwrap();
                        page_ptr_ref = &page.header.next;
                        // we have effectively pre-allocated a page for (much) later use
                    },
                }
            }

            // restart the search from the first unexplored page
            page_ptr_ref = last_searched_page_next_ptr_ref;
        }
    }

    pub(crate) fn debug_pages(&self, output: &mut core::fmt::DebugList) {
        let mut ptr = self.first_page.load(Relaxed);

        while let Some(page) = unsafe { ptr.as_ref() } {
            output.entry(&page);
            ptr = page.header.next.load(Relaxed);
        }
    }
}

// returns (bytes_to_skip, ready)
fn read_atomic_slot_len(len: &u8) -> (usize, bool) {
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

pub(crate) fn string_pool_ptr(len_u8_ptr: &u8) -> *const PoolInner {
    let addr_usize = (len_u8_ptr as *const _) as usize;
    let page_ptr_usize = addr_usize & PAGE_ALIGN_MASK;
    let page_ptr = page_ptr_usize as *const Page;
    let page = unsafe { page_ptr.as_ref() }.unwrap();

    page.header.pool
}

pub(crate) fn deep_drop(mut ptr: *const Page) {
    while let Some(page) = unsafe { ptr.as_ref() } {
        let mut_ptr = (ptr as usize) as *mut u8;
        ptr = page.header.next.load(Relaxed);
        unsafe { dealloc(mut_ptr, PAGE_LAYOUT) };
    }
}

impl core::fmt::Debug for Page {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let mut output = f.debug_list();
        let mut i = 0;

        while let Some((string, next)) = self.debug_slot(i) {
            match string {
                Some(string) => output.entry(&string),
                none => output.entry(&none),
            };

            i = next;
        }

        output.finish()
    }
}
