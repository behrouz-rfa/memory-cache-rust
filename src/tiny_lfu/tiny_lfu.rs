use std::collections::HashMap;
use std::hash::Hash;
use crate::tiny_lfu::list::{LinkedList, Node};
use crate::tiny_lfu::option;
use crate::tiny_lfu::option::{AdmissionPolicy, StatsRecorder, WithSegmentation};
use crate::tiny_lfu::option::StatsRecorder::RecordEviction;


pub struct Policy {
    pub data: HashMap<u64, u64>,
    pub admittor: Option<AdmissionPolicy>,
    pub stats: Option<StatsRecorder>,
    pub window: LinkedList<u64>,
    pub probation: LinkedList<u64>,
    pub protected: LinkedList<u64>,
    pub capacity: usize,
    pub max_window: usize,
    pub max_protected: usize,
}

impl Default for Policy {
    fn default() -> Self {
        Policy {
            data: Default::default(),
            admittor: None,
            stats: None,
            window: Default::default(),
            probation: Default::default(),
            protected: Default::default(),
            capacity: 0,
            max_window: 0,
            max_protected: 0,
        }
    }
}

impl Policy {
    pub fn new(capacity: usize, mut opts: option::Option) -> Self {
        // Consistent behavior relies on capacity for one element in each segment.
        assert!(capacity > 2, "tinylfu: capacity must be positive");
        let mut p = Policy {
            data: HashMap::default(),
            admittor: None,
            stats: None,
            window: LinkedList::new(),
            probation: LinkedList::new(),
            protected: LinkedList::new(),
            capacity,
            max_window: 0,
            max_protected: 0,
        };
        let segmentation = WithSegmentation(0.99, 0.8)(p);
        let s = opts(segmentation);
        s
    }
    pub fn len(&self) -> usize {
        return self.window.len() + self.probation.len() + self.protected.len();
    }

    pub fn record(&mut self, key: u64) {
        match self.admittor {
            None => {}
            Some(_) => { self.admittor = Some(AdmissionPolicy::Recorde(key)) }
        };

        let node = self.data.get(&key);
        match node {
            None => {
                if self.stats.is_none() {
                    self.stats = Option::from(StatsRecorder::RecordMiss)
                }
                self.on_miss(key);
                return;
            }
            Some(_) => {
                match node {
                    None => {}
                    Some(v) => {}
                }
            }
        }
    }

    fn on_miss(&mut self, key: u64) {
        if self.window.len() < self.max_window {
            self.insert_window(key);
            return;
        }
        let candidate = self.window.back();

        self.probation.push_front(candidate.unwrap());
        // This assumes capacity >= 2 or the following eviction panics.
        if self.data.len() < self.capacity {
            self.insert_window(key);
            return;
        }
        let victim = self.probation.back();
        let mut evict: u64 = 0;
        match self.admittor {
            None => {
                evict = victim.unwrap();
            }
            Some(ref mut v) => {
                match v {
                    AdmissionPolicy::Recorde(_) => {}
                    AdmissionPolicy::Admit(candidate, victim) => {
                        //ToDdo
                    }
                }
            }
        }

        self.data.remove(&evict);
        evict = key;
        self.data.insert(key, evict);
        self.window.push_front(evict);
        if self.stats.is_some() {
            self.stats = Some(RecordEviction)
        }


    }


    fn insert_window(&mut self, key: u64) {
        self.window.push_front(key);
        self.data.insert(key, key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        let mut v = Policy::new(3, Box::new(|p| p));

        assert_eq!(v.capacity, 3);
        assert_eq!(v.max_window, 1);
        assert_eq!(v.max_protected, 1);


        let mut v = Policy::new(1000, Box::new(|p| p));


        assert_eq!(v.max_window, 10);
        assert_eq!(v.max_protected, 792);

        v = Policy::new(50, WithSegmentation(0.8, 0.5));

        assert_eq!(v.max_window, 10);
        assert_eq!(v.max_protected, 20);


        v = Policy::new(1000, WithSegmentation(1.0, 0.0));

        assert_eq!(v.max_window, 1);
        assert_eq!(v.max_protected, 1);

        v = Policy::new(1000, WithSegmentation(1.0, 1.0));

        assert_eq!(v.max_window, 1);
        assert_eq!(v.max_protected, 998);
    }
}