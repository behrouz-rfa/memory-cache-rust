use std::fmt::format;
use std::ops::Add;
use std::time;
use std::time::SystemTime;
use rand::distributions::Uniform;
use rand::{Rng, SeedableRng};
use rand::rngs::StdRng;

const cmDepth: usize = 4;

struct CmRows(Vec<u8>);

pub struct CmSketch {
    rows: Vec<CmRows>,
    seed: [u64; cmDepth],
    mask: u64,
}

impl CmSketch {
   pub fn new(num_counter: i64) -> Self {
        assert!(num_counter > 0, "cmSketch: bad numCounters");

        let d = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("Duration since UNIX_EPOCH failed");
        let num_counter = next_2_power(num_counter);

        let mut skatch = CmSketch {
            rows: Vec::with_capacity(cmDepth),
            seed: [0; cmDepth],
            mask: (num_counter - 1) as u64,
        };

        let mut raange = StdRng::seed_from_u64(d.as_secs());
        let source = raange.gen::<u64>();
        for i in 0..cmDepth {
            skatch.seed[i] = source;
            skatch.rows.push(new_cm_row(num_counter));
        }

        skatch
    }

  pub  fn increment(&mut self, hashed: u64) {
        for i in 0..self.rows.len() {
            self.rows[i].increment(((hashed ^ self.seed[i]) & self.mask))
        }
    }

    pub fn estimate(&self, hashed: u64) -> i64 {
        let mut min = 255u8;
        for i in 0..self.rows.len() {
            let val = self.rows[i].get((hashed ^ self.seed[i]) & self.mask);
            if val < min {
                min = val
            }
        }

        min as i64
    }
    pub  fn reset(&mut self) {
        for i in 0..self.rows.len() {
            self.rows[i].reset();
        }
    }

    pub  fn clear(&mut self) {
        for i in 0..self.rows.len() {
            self.rows[i].clear()
        }
    }
}

impl CmRows {
    fn increment(&mut self, n: u64) {
        let i = n / 2;
        let s = (n & 1) * 4;
        let v = (self.0[i as usize] >> s) & 0x0f;
        if v < 15 {
            self.0[i as usize] += 1 << s
        }
    }

    fn get(&self, n: u64) -> u8 {
        self.0[(n / 2) as usize] >> ((n & 1) * 4) & 0x0f
    }

    fn reset(&mut self) {
        for i in 0..self.0.len() {
            self.0[i] = (self.0[i] >> 1) & 0x77
        }
    }

    fn string(&self) -> String {
        let mut s = "".to_owned();
        for i in 0..self.0.len() * 2 {
            s.push_str(format!("{:#02} ", (self.0[(i / 2)] >> ((i & 1) * 4)) & 0x0f).as_str());
        }
        let s = s;
        s
    }

    fn clear(&mut self) {
        // zero each counter
        for i in 0..self.0.len()
        {
            self.0[i] = 0
        }
    }
}

fn new_cm_row(x: i64) -> CmRows {
    CmRows((vec![0u8; (x / 2) as usize]))
}


fn next_2_power(x: i64) -> i64 {
    let mut x = x;
    x -= 1;
    x |= x >> 1;
    x |= x >> 2;
    x |= x >> 4;
    x |= x >> 8;
    x |= x >> 16;
    x |= x >> 32;
    x += 1;
    x
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn TestSketchClear() {
        let mut s = CmSketch::new(16);
        for i in 0..16 {
            s.increment(i);
        }
        s.clear()
    }

    #[test]
    fn test_sketch_reset() {
        let mut s = CmSketch::new(16);
        s.increment(1);
        s.increment(1);
        s.increment(1);
        s.increment(1);
        s.reset();

        assert_eq!(s.estimate(1), 2);
    }

    #[test]
    fn test_sketch_estimate() {
        let mut s = CmSketch::new(16);
        s.increment(1);
        s.increment(1);
        s.increment(9);
        assert_eq!(s.estimate(1), 2);
        assert_eq!(s.estimate(0), 0);
    }

    #[test]
    fn test_sketch_increment() {
        let mut s = CmSketch::new(16);
        s.increment(1);
        s.increment(5);
        s.increment(9);

        for i in 0..cmDepth {
            if s.rows[i].string() == s.rows[0].string() {
                println!("{}", s.rows[i].string());
                break;
            }

            assert_eq!(i, cmDepth - 1, "identical rows, bad seeding");
        }
    }

    #[test]
    fn test_sketch() {
        let mut s = CmSketch::new(5);
        assert_eq!(s.mask, 7)
    }

    #[test]
    fn test_next_2_power() {
        let x: i64 = 10;
        let x = next_2_power(x);
        println!("{}", x)
    }
}