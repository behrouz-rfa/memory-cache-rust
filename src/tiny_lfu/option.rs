use crate::tiny_lfu::tiny_lfu::Policy;

pub enum AdmissionPolicy {
    Recorde(u64),
    Admit(u64, u64),
}

pub enum StatsRecorder {
    RecordMiss,
    RecordHit,
    RecordEviction,
}


pub type Option= Box<dyn FnMut(Policy) -> Policy>;

/*pub fn with_recorder<T>(recorder: AdmissionPolicy) -> Box<dyn FnMut(Policy<T>) -> Policy<T>> {
    Box::new( move|mut p| {
        p.admittor = recorder;
        p
    })
}*/


pub fn WithSegmentation(main: f64, protected: f64)  -> Option{
    assert_eq!((main < 0.0 || main > 1.0 || protected < 0.0 || protected > 1.0), false, "tinylfu: cache segment ratios must be within the range [0, 1]");

    Box::new(move |mut p| {
        let mut max_main = (p.capacity as f64 * main) as usize;
        if max_main < 2 {
            max_main = 2;
        }
        if max_main == p.capacity {
            max_main = p.capacity - 1
        }

        p.max_window = p.capacity - max_main;
        p.max_protected = (max_main as f64 * protected) as usize;
        if p.max_protected < 1 {
            p.max_protected = 1
        }

        if p.max_protected == max_main {
            // Leave at least one element for probation.
            p.max_protected = max_main - 1
        }
        p
    })
}

