String Pools / Strings Interning

### Features

- quick `Deref<str>` implementation - one pointer resolution, one comparison, and one pointer increment
- thin [`PoolStr`] type - a pointer
- The pool is deallocated only when every object referencing it have been dropped.
- no_std, but `alloc` is required
- thread-safe
- [`Pool`]'s `Debug` implementation allows you to see all of its strings
- simple O(n / `P`) insertion/search, where `P` is `Pool`'s const generic parameter

### Example

```rust
# use {strpool::{Pool, PoolStr}, core::ops::Deref};

// you can reduce insertion/search complexity by raising this number,
// at the expense of more frequent allocations for small strings.
// strings are spread evenly into these subpools based on their hash.
// => must be a non-zero power of two.
const SUB_POOLS: usize = 1;

// no need for mutability, the pool uses atomic operations
let pool: Pool<SUB_POOLS> = Pool::new();

// use Pool::intern(&self, &str) to insert a string slice into the pool
// if the string was already present, that PoolStr will be reused.
let pool_string = pool.intern("Hello world!");

// you can obtain a &str with the Deref implementation
assert_eq!(pool_string.deref(), "Hello world!");
// Hash, Eq, Debug, Display are implemented as well.

// you can use Pool::find(&self, &str) to check if the pool contains a string
assert_eq!(pool.find("oh hi mark"), None);

// the empty string doesn't rely on a pool, it's always there
assert_eq!(pool.find(""), Some(PoolStr::empty()));

// See all interned strings via the Debug implementation
println!("{:#?}", pool);
```

### Internal Memory Structure (Example)

![memory.png](https://i.ibb.co/9bXH3Zg/memory.png)
