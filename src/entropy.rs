// (c) 2021 Alexander Gryaznov (agryaznov.com)
//
//! Entropy module for
//! Candle Auction implemented with Ink! smartcontract

use ink_env::Environment;

/// Number of blocks to wait until acceptable randomness is available
/// see const RANDOM_MATERIAL_LEN
/// in https://github.com/paritytech/substrate/blob/v3.0.0/frame/randomness-collective-flip/src/lib.rs
pub const RF_DELAY: u32 = 81;

/// Function to provide randomness to Candle Auction.  
/// Can be, for instance:
///   1. `ink_env::random()` (implemented variant)
///   2. `rand_extension` (see Ink! contract examples)
///   3. whatever else you'd like to use
pub fn random<T>(seed: &[u8]) -> (T::Hash, T::BlockNumber)
where
    T: Environment,
{
    ink_env::random::<T>(seed).expect("cannot get randomness!")
}
