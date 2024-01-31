use core::{sync::atomic::{Ordering::*, AtomicPtr}, fmt};
use serde::{Serialize, Serializer, Deserialize, Deserializer, de::{Visitor, Error as DeError}};
use super::{PoolCell, PoolStr};

static POOL_CELL: AtomicPtr<PoolCell<0>> = AtomicPtr::new(0usize as _);

pub fn set_serde_pool<const P: usize>(pool_cell: &'static PoolCell<P>) {
    POOL_CELL.store(pool_cell as *const _ as *mut _, Relaxed);
}

pub fn get_serde_pool<const P: usize>() -> &'static PoolCell<P> {
    let err = "Please set a pool for serde using strpool::serde::set_serde_pool";
    let pool_cell = unsafe { POOL_CELL.load(Relaxed).as_ref() }.expect(err);
    if pool_cell.subpools == P {
        unsafe { (POOL_CELL.load(Relaxed) as *const PoolCell<P>).as_ref() }.unwrap()
    } else {
        panic!("The current serde pool has a different subpools generic parameter")
    }
}

impl<'a, const P: usize> Deserialize<'a> for PoolStr<P> {
    fn deserialize<D: Deserializer<'a>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_str(PoolStrVisitor::<P>)
    }
}

struct PoolStrVisitor<const P: usize>;

impl<'de, const P: usize> Visitor<'de> for PoolStrVisitor<P> {
    type Value = PoolStr<P>;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        write!(formatter, "a string")
    }

    fn visit_str<E: DeError>(self, s: &str) -> Result<Self::Value, E> {
        Ok(get_serde_pool().pool().intern(s))
    }
}

impl<const P: usize> Serialize for PoolStr<P> {
    // Required method
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&*self)
    }
}

#[test]
fn test_serde() {
    use serde::{Serialize, Deserialize};

    #[derive(Serialize, Deserialize)]
    struct Test {
        test1: PoolStr<16>,
        test2: PoolStr<16>,
        test3: u8,
        test4: String,
    }

    static POOL: PoolCell<16> = PoolCell::new();

    set_serde_pool(&POOL);

    let data = r#"{"test1":"John Doe","test2":"Jack","test3":5,"test4":"Oh String"}"#;
    let p: Test = serde_json::from_str(data).unwrap();
    assert_eq!(data, &*serde_json::to_string(&p).unwrap());

    core::mem::drop(p);

    println!("SERDE: {:#?}", POOL);
}
