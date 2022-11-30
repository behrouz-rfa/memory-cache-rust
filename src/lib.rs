pub mod bloom;
mod store;
mod reclaim;
mod ttl;
pub mod cache;
mod policy;
mod cmsketch;
mod ring;

/// Default hasher for [`HashMap`].
pub type DefaultHashBuilder = ahash::RandomState;


#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {}
}
