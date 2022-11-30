use std::any::{Any, TypeId};

// pub type KeyHash<T> = Box<dyn FnMut(T) -> (u64, i64)>;



pub fn value_to_int<T: 'static>(key: T) -> i64 {
    if is_cast::<T, u64>(&key) {
        let  v = unsafe { std::mem::transmute::< & T,&u64,>(&key) };
        return *v as i64
    }
    panic!("")
    /*   if equals::<T, u64>() {
           let value = cast_ref::<_, u64>(&key).unwrap();
           return *value as i64;
       }

       if equals::<T, usize>() {
           let value = cast_ref::<_, usize>(&key).unwrap();
           return *value as i64;
       }


       if equals::<T, i64>() {
           let value = cast_ref::<_, i64>(&key).unwrap();
           return *value as i64;
       }


       if equals::<T, i32>() {
           let value = cast_ref::<_, i32>(&key).unwrap();

           return *value as i64;
       }

       if equals::<T, u8>() {
           let value = cast_ref::<_, u8>(&key).unwrap();
           return *value as i64;
       }
       if equals::<T, u16>() {
           let value = cast_ref::<_, u16>(&key).unwrap();

           return *value as i64;
       }
       if equals::<T, u32>() {
           let value = cast_ref::<_, u32>(&key).unwrap();

           return *value as i64;
       }
       if equals::<T, i16>() {
           let value = cast_ref::<_, i16>(&key).unwrap();

           return *value as i64;
       }
       panic!("! value type not supported")*/
}

fn is_string<T: ?Sized + Any>(_s: &T) -> bool {
    TypeId::of::<String>() == TypeId::of::<T>()
}

pub fn is_cast<T: ?Sized + Any, V: 'static>(_s: &T) -> bool {
    TypeId::of::<V>() == TypeId::of::<T>()
}

/*pub fn equals<U:  ?Sized + Any, V: 'static>() -> bool {
    TypeId::of::<U>() == TypeId::of::<V>()
}

pub fn cast_ref<U:  ?Sized + Any, V: 'static>(u: &U) -> Option<&V> {
    if equals::<U, V>() {
        Some(unsafe { std::mem::transmute::<&U, &V>(u) })
    } else {
        None
    }
}

pub fn cast_mut<U: 'static, V: 'static>(u: &mut U) -> Option<&mut V> {
    if equals::<U, V>() {
        Some(unsafe { std::mem::transmute::<&mut U, &mut V>(u) })
    } else {
        None
    }
}
*/
#[cfg(test)]
mod tests {
    

    #[test]
    fn test_hash() {

    }
}