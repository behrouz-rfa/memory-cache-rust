//!A Bloom filter is a space-efficient probabilistic data structure,
//! conceived by Burton Howard Bloom in 1970,
//! that is used to test whether an element is a member of a set.
//! False positive matches are possible, but false negatives are not – in other words,
//! a query returns either "possibly in set" or "definitely not in set".
//! Elements can be added to the set, but not removed (though this can be addressed with the counting Bloom filter variant);
//! the more items added, the larger the probability of false positives.




use serde::{Deserialize, Serialize};

const MASK: [u8; 8] = [1, 2, 4, 8, 16, 32, 64, 128];

pub struct Bloom {
    bitset: Vec<i64>,
    elem_num: u64,
    size_exp: u64,
    size: u64,
    set_locs: u64,
    shift: u64,
}

fn calc_size_by_wrong_positives(num_entries: f64, wrongs: f64) -> (u64, u64) {

    let size = -1.0 * num_entries * wrongs.ln() / 0.69314718056_f64.powf(2.0);
    let locs = (0.69314718056_f64 * size / num_entries).ceil() ;
    return (size as u64, locs as u64);
}

impl Bloom {
    ///  returns a new bloom filter.
    pub fn new(num_entries: f64, wrongs: f64) -> Self {
        let mut entries = 0;
        let mut locs = 0;
        if wrongs < 1.0 {
            let (e, l) = calc_size_by_wrong_positives(num_entries, wrongs);
            entries = e;
            locs = l;
        } else {
            entries = num_entries as u64;
            locs = wrongs as u64;
        }

        let (size, exponent) = getSize(entries);
        let mut b = Bloom {
            bitset: vec![],
            elem_num: 0,
            size_exp: exponent,
            size: size - 1,
            set_locs: locs,
            shift: 64 - exponent,
        };
        b.size(size);
        b
    }
    /// <--- http://www.cse.yorku.ca/~oz/hash.html
    /// modified Berkeley DB Hash (32bit)
    /// hash is casted to l, h = 16bit fragments
    /// func (bl Bloom) absdbm(b *[]byte) (l, h uint64) {
    /// 	hash := uint64(len(*b))
    /// 	for _, c := range *b {
    /// 		hash = uint64(c) + (hash << 6) + (hash << bl.sizeExp) - hash
    /// 	}
    /// 	h = hash >> bl.shift
    ///    l = hash << bl.shift >> bl.shift
    /// 	return l, h
    /// }
    pub fn add(&mut self, hash: u64) {
        let h = hash >> self.shift;
        let l = hash << self.shift >> self.shift;

        for i in 0..self.set_locs {
            self.set((h + (i * l)) & self.size);
            self.elem_num += 1;
        };
    }
    /// AddIfNotHas only Adds hash, if it's not present in the bloomfilter.
    /// Returns true if hash was added.
    /// Returns false if hash was already registered in the bloomfilter.
    pub fn add_if_not_has(&mut self, hash: u64) -> bool {
        if self.has(hash) {
            return false;
        }
        self.add(hash);
        true
    }
    /// Clear resets the Bloom filter.
    pub fn clear(&mut self) {
        self.bitset = vec![0; self.bitset.len()]
    }
    /// Set sets the bit[idx] of bitset.
    pub fn set(&mut self, idx: u64) {
        // let b = *self.bitset[(idx >> 6) as usize];

        // let ptr:*mut [i64] =  self.bitset as *mut [i64];
        let mut ptr: *mut i64 = self.bitset.as_mut_ptr();
        unsafe {
            let step = idx >> 6;//((idx >> 6) + ((idx % 64) >> 3));
            ptr = ptr.wrapping_offset(step as isize);

            *ptr |= MASK[(idx % 8) as usize] as i64;
        };

    }
    /// Size makes Bloom filter with as bitset of size sz.
    pub fn size(&mut self, sz: u64) {
        self.bitset = Vec::with_capacity((sz >> 6) as usize); // vec![0i64; (sz >> 6) as usize]
        for i in 0..(sz >> 6) as usize {
            self.bitset.insert(i, 0)
        }
    }
    /// Has checks if bit(s) for entry hash is/are set,
    /// returns true if the hash was added to the Bloom Filter.
    pub fn has(&mut self, hash: u64) -> bool {
        let h = hash >> self.shift;
        let l = hash << self.shift >> self.shift;
        for i in 0..self.set_locs {
            if !self.isset((h + (i * l)) & self.size) {
                return false;
            }
        }

        true
    }
    /// IsSet checks if bit[idx] of bitset is set, returns true/false.
    pub fn isset(&mut self, idx: u64) -> bool {
        let mut ptr: *mut i64 = self.bitset.as_mut_ptr();
        // if ((idx >> 6) + ((idx % 64) >> 3)) as usize > self.bitset.len() {
        //     return false;
        // }
        unsafe {
            let step = idx >> 6 /*+ ((idx % 64) >> 3))*/;
            ptr = ptr.wrapping_offset(step as isize);
        }

        let r = unsafe { (*ptr >> (idx % 8)) & 1 };
        r == 1
    }
    /*  fn json_decode(&mut self, dbData: &[u8]) -> Self {
          let data = serde_json::from_slice::<BloomJsonExport>(dbData);
          i
      }*/
    fn json_encoder(&mut self) -> Vec<u8> {
        let mut bj = BloomJsonExport {
            set_locs: self.set_locs,
            filter_set: vec![0u8; (self.bitset.len() << 3) as usize],
        };

        for i in 0..bj.filter_set.len() {
            let ptr: *mut i64 = self.bitset.as_mut_ptr();
            bj.filter_set[i] = unsafe { ptr.wrapping_offset(i as isize) as u8 }
        }
        let data = serde_json::to_vec(&bj);
        if let Ok(result) = data {
            return result;
        }
        vec![]
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct BloomJsonExport {
    filter_set: Vec<u8>,
    set_locs: u64,
}


fn getSize(mut u_i64: u64) -> (u64, u64) {
    if u_i64 < 512 {
        u_i64 = 512;
    }
    let mut exponent = 0;
    let mut size = 1;
    while size < u_i64 {
        size <<= 1;
        exponent += 1;
    }
    return (size, exponent);
}


#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use uuid::Uuid;

    use crate::bloom::rutil::mem_hash;

    use super::*;

    const N: usize = 1 << 16;


    fn worldlist() -> Vec<Vec<u8>> {
        let _seed = [0u8; 16];
        // let mut rng: StdRng = SeedableRng::from_seed(seed);

        let mut wordlist = Vec::with_capacity(N);
        for _i in 0..wordlist.capacity() {
            // let mut bytes = [0u8; 32];
            let uuid = Uuid::new_v4();
            // rng.fill_bytes(&mut bytes);
            // let v = rand::thread_rng().gen::<[u8; 32]>();

            wordlist.push(Vec::from(uuid.as_bytes().map(|v| v)));
        }
        wordlist
    }

    #[test]
    fn test_number_of_wrong() {
        let mut bf = Bloom::new((N * 10) as f64, 7.0);
        let mut cnt = 0;
        let word_list = worldlist();
        let mut set = HashSet::new();
        // bf.add_if_not_has(1147594788350054766);
        for i in 0..word_list.len() {
            let hash = mem_hash(&*word_list[i]);
            set.insert(hash);
            if !bf.add_if_not_has(hash.into()) {
                cnt += 1;
            }
        }

        assert_eq!(set.len(), word_list.len());

        println!("Bloomfilter New(7* 2**16, 7) \
            (-> size={} bit): \n    \
            Check for 'false positives': {}\
             wrong positive 'Has' results on 2**16 entries => {} %%\n",
                 bf.bitset.len() << 6, cnt, (cnt) as f64 / (N) as f64)
    }

    #[test]
    fn test_has() {
        let mut bf = Bloom::new((N * 10) as f64, 7.0);

        let v = bf.has(18272025040905874063);
        assert_eq!(v, false);

        let _v = bf.has(18272025040905874063);
        bf.add_if_not_has(18272025040905874063);
        let v = bf.has(18272025040905874063);
        assert_eq!(v, true)
    }

    #[test]
    fn oprator_test() {
        //  1 2 4 8 16 32 64
        //0000=0 0001=1 0010=2 0100=4 1000 =8 011111
        let a = 1; //01
        let b = 2;
        assert_eq!(a & b, 0);
        assert_eq!(a | b, 3);
        assert_eq!(a ^ b, 3);
        assert_eq!(a << 4, 16);
        assert_eq!(a >> b, 0);
        assert_eq!(31 >> 4, 1);
        assert_eq!(31 << 2, 124);
        assert_eq!(31 >> 3, 3);
    }
}