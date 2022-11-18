use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::{mem, ptr};

pub type NanoTime = Box<dyn Fn(i64) -> i64>;
pub type CPUTicks = Box<dyn Fn(i64) -> i64>;
pub type Memhash = Box<dyn Fn(*const str, *const usize, *const usize) -> u64>;

unsafe fn any_as_u8_slice<T: Sized>(p: &T) -> &[u8] {
    ::std::slice::from_raw_parts(
        (p as *const T) as *const u8,
        ::std::mem::size_of::<T>(),
    )
}


#[repr(C)]
pub struct StringStruct([u8; 32]);

impl Hash for StringStruct {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.hash(state)
    }
}

pub fn MemHash(data: &[u8]) -> u64 {
    // let (head, body, _tail) = unsafe { data.align_to::<StringStruct>() };
    // assert!(head.is_empty(), "Data was not aligned");
    // let my_struct = &body[0];

    let hash = seahash::hash(data);
    // let my_struct =   unsafe { data.as_mut_ptr() as *mut StringStruct };
    // let mut s = DefaultHasher::new();
    // my_struct.hash(&mut s);
    // let hash = s.finish();
    hash
}
pub fn MemHashByte(data: &[u8]) -> u64 {
    // let (head, body, _tail) = unsafe { data.align_to::<StringStruct>() };
    // assert!(head.is_empty(), "Data was not aligned");
    // let my_struct = &body[0];

    let hash = seahash::hash(data);
    // let my_struct =   unsafe { data.as_mut_ptr() as *mut StringStruct };
    // let mut s = DefaultHasher::new();
    // my_struct.hash(&mut s);
    // let hash = s.finish();
    hash
}