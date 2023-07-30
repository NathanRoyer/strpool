String Pools / Strings Interning

### Features

- owned string pools (also reference-counted)
- no_std (but `alloc` is required)
- quick `Deref<str>` implementation (one pointer resolution, one comparison, and one pointer increment)
- thin [`PoolStr`] type

### Example: Initiating a fetch from github

```rust
# use {strpool::{Pool, PoolStr}, core::ops::Deref};
// no need for mutability, the pool uses atomic operations
let pool = Pool::new();

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
```

### Internal Memory Structure (Example)

![memory.png](https://i.ibb.co/9bXH3Zg/memory.png)
