#![cfg_attr(not(feature = "std"), no_std)]

use ink_lang as ink;

#[ink::contract]
mod candle_auction {
    use ink_storage::collections::HashMap as StorageHashMap;

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
        /// Bids storage: we store only one last (top) bid per user as a hashmap (account => amount)
        /// it is also serves as users balances ledger
        // TODO: in order to make it 'candle' like, we'll need to store such a hashmap for each time (bidding) slot (e.g. block)
        bids: StorageHashMap<AccountId,Balance>,
        /// winner = current top bidder
        winner: Option<AccountId>,
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
                bids: StorageHashMap::new(),
                winner: None,
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
        /// An account can bid by sending the lacking amount so that total amount she sent to this contract covers the bid
        /// I any particual point of time, the user's top bid is equal to total balance she have sent to the contract
        #[ink(message, payable)]
        pub fn bid(&mut self) {
            // fail unless auction is active
            assert!(matches!(self.get_status(), AuctionStatus::OpeningPeriod | AuctionStatus::EndingPeriod));
    
            let bidder = Self::env().caller();
            let mut balance = self.env().transferred_balance();
            if let Some(old_balance) = self.bids.get(&bidder) {
                // update new balance = old_balance + transferred_balance
                balance += old_balance;
            }

            // do not accept bids lesser that current top bid
            if let Some(winner) = self.winner {
                assert!(balance > *self.bids.get(&winner).unwrap_or(&0));
            }

            // finally, accept bid
            self.bids.insert(bidder, balance);
            self.winner = Some(bidder);
        }
    }

    /// Unit tests in Rust are normally defined within such a `#[cfg(test)]`
    /// module and test functions are marked with a `#[test]` attribute.
    /// The below code is technically just normal Rust code.
    #[cfg(not(feature = "ink-experimental-engine"))]
    #[cfg(test)]
    mod tests {
        /// Imports all the definitions from the outer scope so we can use them here.
        use super::*;

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

        #[ink::test]
        #[should_panic]
        fn cannot_bid_until_started() {
            // given
            // Bob and his initial balance
            let bob = ink_env::test::default_accounts::<ink_env::DefaultEnvironment>().unwrap().bob;
            // let bob_initial_bal = ink_env::test::get_account_balance::<ink_env::DefaultEnvironment>(bob);

            // auction starts at block #5
            let mut auction = CandleAuction::new(Some(5),5,10);

            // when 
            // and Bob tries to make a bid before block #5
            auction.bid();

            // then
            // contract should just panic after this line

            // then 
            // the bid is not counted
            // assert!(auction.bids.is_empty());

            // and Bob's money are not taken
            // assert_eq!(ink_env::test::get_account_balance::<ink_env::DefaultEnvironment>(bob), bob_initial_bal);
        }

        #[ink::test]
        #[should_panic]
        fn cannot_bid_when_ended() {
            // given
            // Bob and his initial balance
            let bob = ink_env::test::default_accounts::<ink_env::DefaultEnvironment>().unwrap().bob;
            // let bob_initial_bal = ink_env::test::get_account_balance::<ink_env::DefaultEnvironment>(bob);

            // auction starts at block #1 and ended after block #15
            let mut auction = CandleAuction::new(None,5,10);

            // when 
            // Auction ended
            run_to_block::<Environment>(16);

            // and Bob tries to make a bid before block #5
            auction.bid();

            // then
            // contract should just panic after this line

            // then 
            // the bid is not counted
            // assert!(auction.bids.is_empty());

            // and Bob's money are not taken
            // assert_eq!(ink_env::test::get_account_balance::<ink_env::DefaultEnvironment>(bob), bob_initial_bal);
        }

        #[ink::test]
        fn bidding_works() {
            // given
            let alice = ink_env::test::default_accounts::<ink_env::DefaultEnvironment>().unwrap().alice;
            let mut auction = CandleAuction::new(None,5,10);
            // when
            // Push block to 1 to make auction started
            // Push the new execution context which sets Alice as caller and
            // the `mock_transferred_balance` as the value which the contract
            // will see as transferred to it.
            run_to_block::<ink_env::DefaultEnvironment>(1);
            // set_sender(alice);
            // ink_env::test::set_value_transferred::<ink_env::DefaultEnvironment>(1);

            // then 
            // first bid should set Alice balance to _tranferred_ amount 
            // which is always 500 in default test env (see https://github.com/paritytech/ink/blob/v3.0.0-rc6/crates/env/src/engine/off_chain/mod.rs#L209)
            auction.bid();
            assert_eq!(auction.bids.get(&alice),Some(&500));

            // then
            // further bids are adding up to balance
            run_to_block::<ink_env::DefaultEnvironment>(2);
            auction.bid();
            assert_eq!(auction.bids.get(&alice),Some(&1000));

        }

    }

    // wanted to use experimental fn ink_env::test::set_value_transferred()
    // but those feature goes without ink_env::test::advance_block(), what makes it unsuitable for my tests
    // leaving it here still for memo
    #[cfg(feature = "ink-experimental-engine")]
    #[cfg(test)]
    mod tests_experimental_engine {
        /// Imports all the definitions from the outer scope so we can use them here.
        use super::*;

        #[ink::test]
        fn bidding_works() {
            // given
            let alice = default_accounts().alice;
            set_balance(alice, 10);
            let auction = CandleAuction::new(None,5,10);
            // when
            // Push block to 1 to make auction started
            // Push the new execution context which sets Alice as caller and
            // the `mock_transferred_balance` as the value which the contract
            // will see as transferred to it.
            run_to_block::<ink_env::DefaultEnvironment>(1);
            set_sender(alice);
            ink_env::test::set_value_transferred::<ink_env::DefaultEnvironment>(1);

            // then 
            // first bid should set Alice balance to tranferred amount 
            auction.bid();
            assert_eq!(auction.bids.get(&alice),Some(1));

            // then
            // further bids are adding up to balance
            run_to_block::<ink_env::DefaultEnvironment>(2);
            ink_env::test::set_value_transferred::<ink_env::DefaultEnvironment>(4);
            auction.bid();
            assert_eq!(auction.bids.get(&alice),Some(5));

        }

        // Tests helper functions 
        fn set_sender(sender: AccountId) {
            ink_env::test::set_caller::<ink_env::DefaultEnvironment>(sender);
        }

        fn default_accounts(
        ) -> ink_env::test::DefaultAccounts<ink_env::DefaultEnvironment> {
            ink_env::test::default_accounts::<ink_env::DefaultEnvironment>()
        }

        fn set_balance(account_id: AccountId, balance: Balance) {
            ink_env::test::set_account_balance::<ink_env::DefaultEnvironment>(
                account_id, balance,
            )
        }

        fn get_balance(account_id: AccountId) -> Balance {
            ink_env::test::get_account_balance::<ink_env::DefaultEnvironment>(account_id)
                .expect("Cannot get account balance")
        }

    }
}
