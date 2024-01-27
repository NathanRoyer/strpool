use super::PoolStr;
use core::ops::Deref;

impl<const P: usize> core::fmt::Debug for PoolStr<P> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.deref().fmt(f)
    }
}

impl<const P: usize> core::fmt::Display for PoolStr<P> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.deref().fmt(f)
    }
}

// It's possible that two copies of the same
// string exist at the same time in the store
// even if it's unlikely (see intern_1_127).
// for this reason, we have to fall back to
// a traditional comparison if the pointers
// aren't the same.
impl<const P: usize> PartialEq for PoolStr<P> {
    fn eq(&self, other: &Self) -> bool {
           self.len_ptr == other.len_ptr
        || self.deref() == other.deref()
    }
}

impl<const P: usize> Eq for PoolStr <P>{}

impl<const P: usize> PartialEq<str> for PoolStr<P> {
    fn eq(&self, other: &str) -> bool {
        self.deref() == other
    }
}

impl<const P: usize> PartialEq<PoolStr<P>> for str {
    fn eq(&self, other: &PoolStr<P>) -> bool {
        self == other.deref()
    }
}

impl<const P: usize> AsRef<str> for PoolStr<P> {
    fn as_ref(&self) -> &str {
        self.deref()
    }
}

impl<const P: usize> Default for PoolStr<P> {
    fn default() -> Self {
        Self::empty()
    }
}

impl<const P: usize> core::hash::Hash for PoolStr<P> {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        self.deref().hash(state);
    }
}

impl<const P: usize, I> core::ops::Index<I> for PoolStr<P>
where I: core::slice::SliceIndex<str>,
{
    type Output = I::Output;

    #[inline]
    fn index(&self, index: I) -> &I::Output {
        &self.deref()[index]
    }
}

impl<const P: usize> PartialOrd<str> for PoolStr<P> {
    #[inline]
    fn partial_cmp(&self, other: &str) -> Option<core::cmp::Ordering> {
        self.deref().partial_cmp(other)
    }
}

impl<const P: usize> PartialOrd<PoolStr<P>> for PoolStr<P> {
    #[inline]
    fn partial_cmp(&self, other: &PoolStr<P>) -> Option<core::cmp::Ordering> {
        self.deref().partial_cmp(other.deref())
    }
}

impl<const P: usize> Ord for PoolStr<P> {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.deref().cmp(other.deref())
    }
}
