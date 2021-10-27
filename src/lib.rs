// (c) 2021 Alexader Gryaznov
//
//! Candle Auction implemented with Ink! smartcontract

#![cfg_attr(not(feature = "std"), no_std)]
use ink_lang as ink;

#[ink::contract]
/// Candle Auction module 
mod candle_auction {
    use ink_storage::collections::HashMap as StorageHashMap;
    use ink_storage::Vec as StorageVec;

    #[derive(Debug, PartialEq, Eq, scale::Encode, scale::Decode)]
    #[cfg_attr(feature = "std", derive(scale_info::TypeInfo))]
    /// Error types
    pub enum Error {
        /// Returned if bidding whilr auction isn't in active status
        AuctionNotActive,
        /// Placed bid_new isn't outbidding current winning nid_quo
        /// (bid_new, bid_quo) returned for info
        NotOutBidding(Balance,Balance),
        /// Problems with winning_data observed
        WinningDataCorrupted,
    }
    

    /// Auction statuses
    /// logic inspired by [Parachain Auction](https://github.com/paritytech/polkadot/blob/master/runtime/common/src/traits.rs#L160)
    #[derive(Debug, PartialEq, Eq, scale::Encode, scale::Decode)]
    #[cfg_attr(feature = "std", derive(::scale_info::TypeInfo))]
    pub enum AuctionStatus {
        /// An auction has not started yet.
        NotStarted,
        /// We are in the starting period of the auction, collecting initial bids.
        OpeningPeriod,
        /// We are in the ending period of the auction, where we are taking snapshots of the winning
        /// bids. Snapshots are taken currently on per-block basis, but this logic could be later evolve 
        /// to take snapshots of on arbitrary length (in blocks)
        EndingPeriod(BlockNumber),
        Ended,
        // / We have completed the bidding process and are waiting for the VRF to return some acceptable
        // / randomness to select the winner. The number represents how many blocks we have been waiting.
        // VrfDelay(BlockNumber),
    }
   
    /// Defines the storage of the contract.
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
        /// Bids storage: 
        // / for each block during ending_period (aka sample) (outer hashmap), 
        // / we store only one last (top) bid per user as a hashmap (account => amount) (inner hashmap)
        // / (therefore it is also serves as users balances ledger)
        // bids: StorageHashMap<BlockNumber, StorageHashMap<AccountId,Balance>>,
        bids: StorageHashMap<AccountId,Balance>,
        /// winner = current top bidder
        winner: Option<AccountId>,
        /// WinningData = storage of winners per sample (block)
        /// it's a vector of optional (AccountId, Balance) tuples representing winner in block (sample) along with her bid
        /// 0-indexed value is winner for OpeningPeriod
        /// i-indexed value is winner for sample (block) #i of EndingPeriod
        winning_data: StorageVec<Option<(AccountId,Balance)>>,
    }

    impl CandleAuction {
        /// Auction constructor.
        /// Initializes the start_block to next block (if not set).
        #[ink(constructor)]
        pub fn new(start_block: Option<BlockNumber>, opening_period: BlockNumber, ending_period: BlockNumber) -> Self {
            let mut winning_data = StorageVec::<Option<(AccountId, Balance)>>::new();
            (0..ending_period+1).for_each(|_| winning_data.push(None));

            Self { 
                start_block: start_block.unwrap_or(Self::env().block_number() + 1),
                opening_period,
                ending_period, 
                bids: StorageHashMap::new(),
                winner: None,
                winning_data
             }
        }

        /// Helper for getting auction status
        fn status(&self, block: BlockNumber) -> AuctionStatus {
            let opening_period_last_block = self.start_block + self.opening_period - 1;
            let ending_period_last_block = opening_period_last_block + self.ending_period;

            if block >= self.start_block  {
                if block > opening_period_last_block {
                    if block > ending_period_last_block {
                        AuctionStatus::Ended
                    } else {
                        // number of slot = number of block inside ending period
                        AuctionStatus::EndingPeriod(block - opening_period_last_block)
                    }
                } else {
                        AuctionStatus::OpeningPeriod

                } 
            } else {
                AuctionStatus::NotStarted
            }
        }

        /// Helper for handling bid
        fn handle_bid(&mut self, bidder: AccountId, bid_increment: Balance, block: BlockNumber) -> Result<(), Error> {
            // fail unless auction is active
            let auction_status = self.status(block);
            let offset = match auction_status {
                AuctionStatus::OpeningPeriod => 0,
                AuctionStatus::EndingPeriod(o) => o,
                _ => return Err(Error::AuctionNotActive)
            };

            let mut bid = bid_increment;
            if let Some(balance) = self.bids.get(&bidder) {
                // update new_balance = old_balance + transferred_balance
                bid += balance;
            }

            // do not accept bids lesser that current top bid
            if let Some(winner) = self.winner {
                let winners_balance = *self.bids.get(&winner).unwrap_or(&0);
                if bid < winners_balance {
                    return Err(Error::NotOutBidding(bid,winners_balance))
                }
                // assert!(bid > winners_balance, "You aren't outbidding {} with {}", bid, winners_balance);
            }

            // finally, accept bid
            self.bids.insert(bidder, bid);
            self.winner = Some(bidder);
            // and update winning_data
            // for retrospective candle-fashioned winner detection
            match self.winning_data.set(offset, Some((bidder,bid))) {
                Err(ink_storage::collections::vec::IndexOutOfBounds) => Err(Error::WinningDataCorrupted),
                Ok(_) => Ok(())
            }     
        }

        /// Message to get the status of the auction given the current block number.
        #[ink(message)]
    	pub fn get_status(&self) -> AuctionStatus {
            let now = self.env().block_number();
            self.status(now)
        }

        /// Message to place a bid
        /// An account can bid by sending the lacking amount so that total amount she sent to this contract covers the bid
        /// I any particual point of time, the user's top bid is equal to total balance she have sent to the contract
        #[ink(message, payable)]
        pub fn bid(&mut self) {
            let now = self.env().block_number();
            let bidder = Self::env().caller();
            let bid_increment = self.env().transferred_balance();
            match self.handle_bid(bidder, bid_increment, now) {
                Err(Error::AuctionNotActive) => {
                    panic!("Auction isn't active!")
                },
                Err(Error::NotOutBidding(bid_new,bid_quo)) => {
                    panic!("You can't outbid {} with {}", bid_new, bid_quo)
                },
                Err(Error::WinningDataCorrupted) => {
                    panic!("Auction's winning data corrupted!")
                },
                Ok(()) => {}
            }
        }
    }

    /// Tests
    #[cfg(not(feature = "ink-experimental-engine"))]
    #[cfg(test)]
    mod tests {
        /// Imports all the definitions from the outer scope so we can use them here.
        use super::*;
        /// Imports `ink_lang` so we can use `#[ink::test]`.
        use ink_lang as ink;

        fn run_to_block<T>(n: T::BlockNumber)
        where 
            T: ink_env::Environment,
        {
            let mut block = ink_env::block_number::<T>();
            while block < n {
                match ink_env::test::advance_block::<T>() {
                    Err(_) => {panic!("Cannot add blocks to test chain!")},
                    Ok(_) => {block = ink_env::block_number::<T>()}

                }
            }
        }

        fn set_sender<T>(sender: AccountId, amount: T::Balance)
        where 
            T: ink_env::Environment<Balance = u128>,
        {
            const WALLET: [u8; 32] = [7; 32];
            ink_env::test::push_execution_context::<Environment>(
                sender,
                WALLET.into(),
                1000000,
                amount, 
                ink_env::test::CallData::new(ink_env::call::Selector::new([0x00; 4])), /* dummy */
            );
        }

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
            // an auction with the following picture:
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
            assert_eq!(candle_auction.get_status(), AuctionStatus::EndingPeriod(1));
            run_to_block::<Environment>(12);
            assert_eq!(candle_auction.get_status(), AuctionStatus::EndingPeriod(7));
            run_to_block::<Environment>(13);
            assert_eq!(candle_auction.get_status(), AuctionStatus::Ended);
        }

        #[ink::test]
        #[should_panic]
        fn cannot_bid_until_started() {
            // given
            // default account (Alice)

            // when 
            // auction starts at block #5
            let mut auction = CandleAuction::new(Some(5),5,10);

            // and Alice tries to make a bid before block #5
            auction.bid();

            // then
            // contract should just panic after this line
        }

        #[ink::test]
        #[should_panic]
        fn cannot_bid_when_ended() {
            // given
            // default account (Alice)
            // and auction starts at block #1 and ended after block #15
            let mut auction = CandleAuction::new(None,5,10);

            // when 
            // Auction is ended
            run_to_block::<Environment>(16);

            // and Alice tries to make a bid before block #5
            auction.bid();

            // then
            // contract should just panic after this line
        }

        #[ink::test]
        fn bidding_works() {
            // given
            let alice = ink_env::test::default_accounts::<Environment>().unwrap().alice;
            let mut auction = CandleAuction::new(None,5,10);
            // when
            // Push block to 1 to make auction started
            run_to_block::<Environment>(1);

            // Alice bids 100
            set_sender::<Environment>(alice,100);            
            auction.bid();

            // then
            // bid is accepted
            assert_eq!(auction.bids.get(&alice),Some(&100));
            // and Alice is current winner 
            assert_eq!(auction.winner, Some(alice));

            // and
            // further Alice' bids are adding up to her balance
            run_to_block::<Environment>(2);
            set_sender::<Environment>(alice,25);            
            auction.bid();
            assert_eq!(auction.bids.get(&alice),Some(&125));
            // and Alice is still the winner 
            assert_eq!(auction.winner, Some(alice));
        }

        #[ink::test]
        fn noncandle_winner_determined() {
            // given
            // Alice and Bob 
            let alice = ink_env::test::default_accounts::<ink_env::DefaultEnvironment>().unwrap().alice;
            let bob = ink_env::test::default_accounts::<ink_env::DefaultEnvironment>().unwrap().bob;
            // and an auction
            let mut auction = CandleAuction::new(None,5,10);
            // when
            // auction starts
            run_to_block::<Environment>(1);
            // Alice bids 100
            set_sender::<Environment>(alice, 100);
            auction.bid();

            // Bob bids 101
            set_sender::<Environment>(bob, 101);
            auction.bid();

            // then 
            // Bob wins
            assert_eq!(auction.winner, Some(bob));
        }

        #[ink::test]
        fn winning_data_constructed_correctly() {
            // given
            // an auction with the following structure:
            //  [1][2][3][4][5][6][7][8][9][10][11][12][13]
            //     | opening  |        ending         |     
            let mut auction = CandleAuction::new(Some(2),4,7);

            // Alice and Bob 
            let alice = ink_env::test::default_accounts::<ink_env::DefaultEnvironment>().unwrap().alice;
            let bob = ink_env::test::default_accounts::<ink_env::DefaultEnvironment>().unwrap().bob;

            // when
            // there is no bids
            // then
            // winning_data initialized with Nones
            assert_eq!(auction.winning_data, [None; 8].iter().map(|o| *o).collect());

            // when 
            // there are bids in opening period
            run_to_block::<Environment>(3);
            // Alice bids 100
            set_sender::<Environment>(alice,100);            
            auction.bid();

            run_to_block::<Environment>(5);
            // Bob bids 100
            set_sender::<Environment>(bob,101);            
            auction.bid();
            
            // then 
            // the top of these bids goes to index 0
            assert_eq!(
                auction.winning_data, 
                [Some((bob,101)), None, None, None, None, None, None, None].iter().map(|o| *o).collect()
            );

            // when 
            // bids added in Ending Period
            run_to_block::<Environment>(7);
            // Alice bids 102
            set_sender::<Environment>(alice,2);            
            auction.bid();

            run_to_block::<Environment>(9);
            // Bob bids 103
            set_sender::<Environment>(bob,2);            
            auction.bid();

            run_to_block::<Environment>(11);
            // Alice bids 104
            set_sender::<Environment>(alice,2);            
            auction.bid();

            // then
            // bids are accounted for correclty 
            assert_eq!(
                auction.winning_data, 
                [Some((bob,101)), None, Some((alice,102)), None, Some((bob,103)), None, Some((alice,104)), None]
                    .iter().map(|o| *o).collect()
            );
        }
    }
}
