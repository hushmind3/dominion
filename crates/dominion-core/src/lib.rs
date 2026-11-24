//! Compatibility facade for the original public dominion-core paths.
//! Implementation lives exactly once in dominion-contracts and dominion-harness.
pub use dominion_contracts::*;
pub use dominion_harness::{hosted,runtime_port};

#[cfg(test)]
mod compatibility {
    #[test]
    fn facade_and_split_packages_share_types(){
        let snapshot:dominion_harness::hosted::Snapshot=super::hosted::Snapshot::new("facade","test",3).unwrap();
        let _:dominion_contracts::domain::Run=snapshot.native;
        assert_eq!(super::identity::stable_id(&["same"]),dominion_contracts::identity::stable_id(&["same"]));
    }
}
