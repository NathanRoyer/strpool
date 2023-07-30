use super::PoolStr;
use core::ops::Deref;

impl core::fmt::Debug for PoolStr {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.deref().fmt(f)
    }
}

impl core::fmt::Display for PoolStr {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.deref().fmt(f)
    }
}

impl PartialEq for PoolStr {
    fn eq(&self, other: &Self) -> bool {
           self.len_ptr == other.len_ptr
        || self.deref() == other.deref()
    }
}

impl Eq for PoolStr {}

impl PartialEq<str> for PoolStr {
    fn eq(&self, other: &str) -> bool {
        self.deref() == other
    }
}

impl AsRef<str> for PoolStr {
    fn as_ref(&self) -> &str {
        self.deref()
    }
}

impl Default for PoolStr {
    fn default() -> Self {
        Self::empty()
    }
}

impl core::hash::Hash for PoolStr {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        self.deref().hash(state);
    }
}

impl<I> core::ops::Index<I> for PoolStr
where I: core::slice::SliceIndex<str>,
{
    type Output = I::Output;

    #[inline]
    fn index(&self, index: I) -> &I::Output {
        &self.deref()[index]
    }
}

impl PartialOrd<str> for PoolStr {
    #[inline]
    fn partial_cmp(&self, other: &str) -> Option<core::cmp::Ordering> {
        self.deref().partial_cmp(other)
    }
}

impl PartialOrd<PoolStr> for PoolStr {
    #[inline]
    fn partial_cmp(&self, other: &PoolStr) -> Option<core::cmp::Ordering> {
        self.deref().partial_cmp(other.deref())
    }
}
