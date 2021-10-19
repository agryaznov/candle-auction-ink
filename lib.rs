#![cfg_attr(not(feature = "std"), no_std)]

use ink_lang as ink;

#[ink::contract]
mod candle_auction {
    use ink_storage::collections::Vec;
    /// Auction status
    /// logic taken from file:///home/greez/dev/polkadot/polkadot/doc/cargo-doc/src/polkadot_runtime_common/traits.rs.html#153

    #[derive(Debug, PartialEq, Eq, scale::Encode, scale::Decode)]
    #[cfg_attr(feature = "std", derive(::scale_info::TypeInfo))]
    pub enum AuctionStatus {
        /// An auction has not started yet.
        NotStarted,
        /// We are in the starting period of the auction, collecting initial bids.
        OpeningPeriod,
        // / We are in the ending period of the auction, where we are taking snapshots of the winning
        // / bids. This state supports "sampling", where we may only take a snapshot every N blocks.
        // / In this case, the first number is the current sample number, and the second number
        // / is the sub-sample. i.e. for sampling every 20 blocks, the 25th block in the ending period
        // / will be `EndingPeriod(1, 5)`.
        // EndingPeriod(BlockNumber, BlockNumber),
        EndingPeriod,
        Ended
        // / We have completed the bidding process and are waiting for the VRF to return some acceptable
        // / randomness to select the winner. The number represents how many blocks we have been waiting.
        // VrfDelay(BlockNumber),
    }
   
    /// Defines the storage of your contract.
    /// Add new fields to the below struct in order
    /// to add new static storage fields to your contract.
    #[ink(storage)]
    pub struct CandleAuction {
        /// Stores a single `bool` value on the storage.
        // value: bool,
        start_block: BlockNumber,
        /// The number of blocks of Opening period.
        /// We assume this period starts in start_block (default val is the next block after the Auction has been created)
        opening_period: BlockNumber,
        /// The number of blocks of Ending period, over which an auction may be retroactively ended.
        /// We assume this period starts right after Opening perid ends.
        ending_period: BlockNumber,
        /// Bids storage: every bid is stored as a tuple (account, bid_amount)
        bids: Vec<(AccountId,Balance)>,
    }

    impl CandleAuction {
        /// Auction constructor.
        /// initializes the start_block to next block (if not set).
        #[ink(constructor)]
        pub fn new(start_block: Option<BlockNumber>, opening_period: BlockNumber, ending_period: BlockNumber) -> Self {
            Self { 
                start_block: start_block.unwrap_or(Self::env().block_number() + 1),
                opening_period,
                ending_period, 
                bids: Vec::new()
             }
        }

        /// Message to get the status of the auction given the current block number.
        // TODO: see file:///home/greez/dev/polkadot/polkadot/doc/cargo-doc/src/polkadot_runtime_common/auctions.rs.html#322-343 for ref
        #[ink(message)]
    	pub fn get_status(&self) -> AuctionStatus {
            let now = self.env().block_number();
            let opening_period_last_block = self.start_block + self.opening_period - 1;
            let ending_period_last_block = opening_period_last_block + self.ending_period;

            if now >= self.start_block  {
                if now > opening_period_last_block {
                    if now > ending_period_last_block {
                        AuctionStatus::Ended
                    } else {
                        AuctionStatus::EndingPeriod
                    }
                } else {
                        AuctionStatus::OpeningPeriod

                } 
            } else {
                AuctionStatus::NotStarted
            }
        }

        /// Message to place a bid
        #[ink(message)]
        pub fn bid(&self) {
            // TODO: see pub fn handle_bid() for reference 

        }

        // Simply returns the current value of our `bool`.
        // #[ink(message)]
        // pub fn get(&self) -> bool {
        //     self.value
        // }
    }

    /// Unit tests in Rust are normally defined within such a `#[cfg(test)]`
    /// module and test functions are marked with a `#[test]` attribute.
    /// The below code is technically just normal Rust code.
    #[cfg(test)]
    mod tests {
        // TODO: run_to_block() similar to https://github.com/paritytech/polkadot/blob/f520483aa3e7ca93f7adabc0149d880712834eab/runtime/common/src/auctions.rs#L901
        fn run_to_block<T>(n: T::BlockNumber)
        where 
            T: ink_env::Environment,
        {
            let mut block = ink_env::block_number::<T>().unwrap();
            while block < n {
                ink_env::test::advance_block::<T>().unwrap();
                block = ink_env::block_number::<T>().unwrap();
            }
        }
        /// Imports all the definitions from the outer scope so we can use them here.
        use super::*;

        /// Imports `ink_lang` so we can use `#[ink::test]`.
        use ink_lang as ink;

        /// We test if the constructor does its job.
        #[ink::test]
        fn new_works() {
            let candle_auction = CandleAuction::new(Some(10),5,10);
            assert_eq!(candle_auction.start_block, 10);
            assert_eq!(candle_auction.get_status(), AuctionStatus::NotStarted);
        }

        #[ink::test]
        fn new_default_start_block_works() {
            run_to_block::<Environment>(12);
            let candle_auction = CandleAuction::new(None,5,10);
            assert_eq!(candle_auction.start_block, 13);
            assert_eq!(candle_auction.get_status(), AuctionStatus::NotStarted);
        }

        #[ink::test]
        fn auction_statuses_returned_correctly() {
            // an auction with following picture:
            //  [1][2][3][4][5][6][7][8][9][10][11][12][13]
            //     | opening  |             ending    |     
            let candle_auction = CandleAuction::new(Some(2),4,7);

            assert_eq!(candle_auction.get_status(), AuctionStatus::NotStarted);
            run_to_block::<Environment>(1);
            assert_eq!(candle_auction.get_status(), AuctionStatus::NotStarted);
            run_to_block::<Environment>(2);
            assert_eq!(candle_auction.get_status(), AuctionStatus::OpeningPeriod);
            run_to_block::<Environment>(5);
            assert_eq!(candle_auction.get_status(), AuctionStatus::OpeningPeriod);
            run_to_block::<Environment>(6);
            assert_eq!(candle_auction.get_status(), AuctionStatus::EndingPeriod);
            run_to_block::<Environment>(12);
            assert_eq!(candle_auction.get_status(), AuctionStatus::EndingPeriod);
            run_to_block::<Environment>(13);
            assert_eq!(candle_auction.get_status(), AuctionStatus::Ended);
        }
    }
}
