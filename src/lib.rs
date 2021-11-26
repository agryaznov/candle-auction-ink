// (c) 2021 Alexander Gryaznov (agryaznov.com)
//
//! Candle Auction implemented with Ink! smartcontract

#![cfg_attr(not(feature = "std"), no_std)]
use ink_lang as ink;

// randomness source
mod entropy;

#[ink::contract]
mod candle_auction {
    use ink_env::{
        call::{build_call, utils::ReturnType, ExecutionInput, Selector},
        transfer,
    };
    use ink_storage::collections::HashMap as StorageHashMap;
    use ink_storage::Vec as StorageVec;
    use scale::{Decode, Encode};
    // use parity_scale_codec::Decode

    #[derive(Debug, PartialEq, Eq, scale::Encode, scale::Decode)]
    #[cfg_attr(feature = "std", derive(scale_info::TypeInfo))]
    /// Error types
    pub enum Error {
        /// Returned if bidding while auction isn't in active status
        AuctionNotActive,
        /// Placed bid_new isn't outbidding current winning nid_quo
        /// (bid_new, bid_quo) returned for info
        NotOutBidding(Balance, Balance),
        /// Problems with winning_data observed
        WinningDataCorrupted,
    }

    /// Auction statuses
    /// logic inspired by
    /// [Parachain Auction](https://github.com/paritytech/polkadot/blob/master/runtime/common/src/traits.rs#L160)
    #[derive(Debug, PartialEq, Eq, scale::Encode, scale::Decode)]
    #[cfg_attr(feature = "std", derive(::scale_info::TypeInfo))]
    pub enum Status {
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

    /// Auction subject: what are we bidding for?
    #[derive(scale::Encode, scale::Decode)]
    #[cfg_attr(feature = "std", derive(scale_info::TypeInfo))]
    pub enum Subject {
        NFTs,
        Domain(Hash),
    }

    /// Event emitted when a bid is accepted.
    #[ink(event)]
    pub struct Bid {
        #[ink(topic)]
        from: AccountId,

        bid: Balance,
    }

    /// Event emitted when the auction winner is rewarded.
    #[ink(event)]
    pub struct Reward {
        #[ink(topic)]
        to: AccountId,

        contract: AccountId,
        subject: Subject,
    }

    /// Defines the storage of the contract.
    #[ink(storage)]
    pub struct CandleAuction {
        /// Contract owner
        owner: AccountId,
        /// Stores a single `bool` value on the storage.
        // value: bool,
        start_block: BlockNumber,
        /// The number of blocks of Opening period.
        /// We assume this period starts in start_block (default val is the next block after the Auction has been created)
        opening_period: BlockNumber,
        /// The number of blocks of Ending period, over which an auction may be retroactively ended.
        /// We assume this period starts right after Opening perid ends.
        ending_period: BlockNumber,
        /// Bidders balances storage.  
        /// Current user's balance = her top bid
        balances: StorageHashMap<AccountId, Balance>,
        /// *winning* <bidder> = current top bidder.  
        /// Not to be confused with *winner* = bidder who finally won.   
        winning: Option<AccountId>,
        // winner (with bid) who finally won Candle auction
        winner: Option<(AccountId, Balance)>,
        /// WinningData = storage of winners per sample (block)
        /// it's a vector of optional (AccountId, Balance) tuples representing winner in block (sample) along with her bid
        /// 0-indexed value is winner for OpeningPeriod
        /// i-indexed value is winner for sample (block) #i of EndingPeriod
        winning_data: StorageVec<Option<(AccountId, Balance)>>,
        /// ERC721 contract
        /// rewarding contract address (NFT or DNS)
        reward_contract_address: AccountId,
        /// What we are bidding for?
        /// 0 = NFT <-- default
        /// 1 = DNS
        /// 2..255 = reserved for further reward methods
        subject: u8,
        /// Domain name (in case we bid for it)
        domain: Hash,
    }

    impl CandleAuction {
        /// Auction constructor.  
        /// Initializes the start_block to next block (if not set).  
        /// If start_block is set, checks it is in the future (to prevent backdating).  
        #[ink(constructor)]
        pub fn new(
            start_block: Option<BlockNumber>,
            opening_period: BlockNumber,
            ending_period: BlockNumber,
            subject: u8,
            domain: Hash,
            reward_contract_address: AccountId,
        ) -> Self {
            if subject > 1 {
                panic!("Only subjects [0,1] are supported so far!")
            }

            let now = Self::env().block_number();
            let start_in = start_block.unwrap_or(now + 1);
            // Security check versus backdating
            assert!(
                start_in > now,
                "Auction is allowed to be scheduled to future blocks only!"
            );

            let mut winning_data = StorageVec::<Option<(AccountId, Balance)>>::new();
            (0..ending_period + 1).for_each(|_| winning_data.push(None));

            Self {
                owner: Self::env().caller(),
                start_block: start_in,
                opening_period,
                ending_period,
                balances: StorageHashMap::new(),
                winning: None,
                winner: None,
                winning_data,
                reward_contract_address,
                subject,
                domain,
            }
        }

        /// Auction status.
        fn status(&self, block: BlockNumber) -> Status {
            let opening_period_last_block = self.start_block + self.opening_period - 1;
            let ending_period_last_block = opening_period_last_block + self.ending_period;

            if block >= self.start_block {
                if block > opening_period_last_block {
                    if block > ending_period_last_block {
                        Status::Ended
                    } else {
                        // number of slot = number of block inside ending period
                        Status::EndingPeriod(block - opening_period_last_block)
                    }
                } else {
                    Status::OpeningPeriod
                }
            } else {
                Status::NotStarted
            }
        }

        /// Handle bid.
        fn handle_bid(
            &mut self,
            bidder: AccountId,
            bid_increment: Balance,
            block: BlockNumber,
        ) -> Result<(), Error> {
            // fail unless auction is active
            let auction_status = self.status(block);
            let offset = match auction_status {
                Status::OpeningPeriod => 0,
                Status::EndingPeriod(o) => o,
                _ => return Err(Error::AuctionNotActive),
            };

            let mut bid = bid_increment;
            if let Some(balance) = self.balances.get(&bidder) {
                // update new_balance = old_balance + transferred_balance
                bid += balance;
            }

            // do not accept bids lesser that current top bid
            if let Some(winning) = self.winning {
                let winning_balance = *self.balances.get(&winning).unwrap_or(&0);
                if bid < winning_balance {
                    return Err(Error::NotOutBidding(bid, winning_balance));
                }
            }

            // finally, accept bid
            self.balances.insert(bidder, bid);
            self.winning = Some(bidder);
            // and update winning_data
            // for retrospective candle-fashioned winning bidder detection
            match self.winning_data.set(offset, Some((bidder, bid))) {
                Err(ink_storage::collections::vec::IndexOutOfBounds) => {
                    Err(Error::WinningDataCorrupted)
                }
                Ok(_) => {
                    self.env().emit_event(Bid {
                        from: bidder,
                        bid: bid,
                    });
                    Ok(())
                }
            }
        }

        /// Pay back.
        /// Winner gets her reward.
        /// Loosers get their balances back.
        /// Contract owner gets winner`s balance (winning bid)
        fn pay_back(&mut self, reward: fn(&Self, to: AccountId) -> (), to: AccountId) {
            // should be executed only on Ended auction
            assert_eq!(
                self.get_status(),
                Status::Ended,
                "Auction is not Ended, no payback is possible!"
            );

            if let Some(winner) = self.get_winner() {
                // winner gets her reward
                if to == winner {
                    // reward winner with specified reward method call
                    reward(&self, to);
                    return;
                } else if to == self.owner {
                    // remove winner balance from ledger:
                    let bal = self.balances.take(&winner).unwrap();
                    // zero-balance check: bid 0 is possible, but nothing to pay back
                    if bal > 0 {
                        // and pay for sold lot
                        // to auction owner
                        transfer::<Environment>(to, bal).unwrap();
                    }
                    return;
                }
            }

            // pay the loser his bid amount back
            let bal = self.balances.take(&to).unwrap();
            // zero-balance check: bid 0 is possible, but nothing to pay back
            if bal > 0 {
                // and pay
                transfer::<Environment>(to, bal).unwrap();
            }
        }

        /// Cross contract invocation method  
        /// common for both rewarding methods
        fn invoke_contract<Args>(&self, contract: AccountId, input: ExecutionInput<Args>)
        where
            Args: Encode,
        {
            let params = build_call::<Environment>()
                .callee(contract)
                .exec_input(input)
                .returns::<ReturnType<Result<(), Error>>>();

            match params.fire() {
                Ok(_v) => {}
                Err(e) => {
                    match e {
                        ink_env::Error::CodeNotFound | ink_env::Error::NotCallable => {
                            // Our recipient wasn't a smart contract, so there's nothing more for
                            // us to do
                            let msg = ink_prelude::format!(
                                "Recipient at {:#04X?} from is not a smart contract ({:?})",
                                self.reward_contract_address,
                                e
                            );
                            panic!("{}", msg)
                        }
                        _ => {
                            // We got some sort of error from the call to our recipient smart
                            // contract, and as such we must revert this call
                            let msg = ink_prelude::format!(
                                "Got error \"{:?}\" while trying to call {:?}",
                                e,
                                self.reward_contract_address,
                            );
                            panic!("{}", msg)
                        }
                    }
                }
            }
        }

        /// Pluggable reward logic: OPTION-1.    
        /// Reward with NFT(s) (ERC721).  
        /// Contract rewards an auction winner by giving her approval to transfer
        /// ERC721 tokens on behalf of the auction contract.  
        ///
        /// DESIGN DECISION: we call ERC721 set_approval_for_all() instead of approve() for  
        ///  1. the sake of simplicity, no need to specify TokenID  
        ///     as we need to send this token to the contract anyway,  _after_ instantiation
        ///     but still _before_ auctions starts
        ///  2. this allows to set auction for collection of tokens instead of just for one thing
        ///
        /// Cross conract call to ERC721 set_approval_for_all() method  
        /// which is expected to have the selector: 0xFEEDBABE   
        fn give_nft(&self, to: AccountId) {
            let selector = Selector::new([0xFE, 0xED, 0xBA, 0xBE]);
            let input = ExecutionInput::new(selector).push_arg(to).push_arg(true);

            self.invoke_contract(self.reward_contract_address, input);

            self.env().emit_event(Reward {
                to: to,
                subject: Subject::NFTs,
                contract: self.reward_contract_address,
            });
        }

        /// Pluggable reward logic: OPTION-2.    
        /// Reward with domain name.  
        /// Contract rewards an auction winner by transferring her auctioned
        /// domain name using the dns contract.
        ///
        /// Cross conract call to ERC721 set_approval_for_all() method,  
        /// which is expected to have the selector: 0xFEEDDEED   
        fn give_domain(&self, to: AccountId) {
            let selector = Selector::new([0xFE, 0xED, 0xDE, 0xED]);
            let input = ExecutionInput::new(selector)
                .push_arg(self.domain)
                .push_arg(to);

            self.invoke_contract(self.reward_contract_address, input);

            self.env().emit_event(Reward {
                to: to,
                subject: Subject::Domain(self.domain),
                contract: self.reward_contract_address,
            });
        }

        /// Message to get the auction subject.
        #[ink(message)]
        pub fn get_subject(&self) -> Subject {
            match self.subject {
                0 => Subject::NFTs,
                1 => Subject::Domain(self.domain),
                _ => panic!("Current Subject is not supported!"),
            }
        }

        /// Message to get the status of the auction given the current block number.
        #[ink(message)]
        pub fn get_status(&self) -> Status {
            let now = self.env().block_number();
            self.status(now)
        }

        #[ink(message)]
        pub fn get_winner(&self) -> Option<AccountId> {
            // temporary same as noncandle
            self.get_noncandle_winner()
        }

        /// Helper to get the auction winner in noncandle fashion.  
        /// To avoid ambiguity, winner is determined once the auction ended.  
        pub fn get_noncandle_winner(&self) -> Option<AccountId> {
            if self.get_status() == Status::Ended {
                self.winning
            } else {
                None
            }
        }

        /// Retrospective RANDOM `candle blowing`:  
        ///  `seed` buffer is used for additional hash randomization.  
        /// Returns a record from `winning_data` determined randomly by imitated `candle blow`
        fn blow_candle(&self, seed: &[u8]) -> Option<(AccountId, Balance)> {
            let opening_period_last_block = self.start_block + self.opening_period - 1;
            let ending_period_last_block = opening_period_last_block + self.ending_period;

            // Here is where we use Random func
            let (raw_offset, known_since): (Hash, BlockNumber) =
                crate::entropy::random::<Environment>(seed);
            if ending_period_last_block <= known_since {
                // (Inspired by:
                //   https://github.com/paritytech/polkadot/blob/v0.9.13-rc1/runtime/common/src/auctions.rs#L526)
                // Our random seed was known only after the auction ended. Good to use.
                let raw_offset_block_number = <BlockNumber>::decode(&mut raw_offset.as_ref())
                    .expect("secure hashes should always be bigger than the block number; qed");

                // detect the block when 'the candle went out' in Ending Period
                let offset = raw_offset_block_number % self.ending_period + 1;
                // TODO: emit event WinningOffset
                // self.env().emit_event(Bid {
                //     from: bidder,
                //     bid: bid,
                // });

                // Detect winning slot.
                // Starting from the `candle-determined` block,
                // iterate backwards until a block with some bids found
                let mut win_data: Option<(AccountId, Balance)> = None;
                for i in (1..offset + 1).rev() {
                    if let Some((w, b)) = self.winning_data.get(i).unwrap() {
                        win_data = Some((*w, *b));
                        break;
                    }
                }
                return win_data;
            }
            None
        }
        /// Helper to get the Candle auction winner:
        ///  Get random block in Ending period,  
        ///  then get the highest bidder in that block
        /// 1. [done] Easy lvl: use ink_env::random
        /// TODO: 2. Intermediate lvl: use chain extension like in ink rand-extension example
        ///          -> impl an Entropy Trait in separate crate Randomness
        ///          in it, impl fn random which calls ink_env::random, but could be any other (e.g. random_ext)
        /// TODO: this sould be invoked automatically? or not? maybe not, but once
        ///    making whis call by account (non auto) brings some more entropy in sense which
        ///    on what exact block this wouls be called (results of random func will be different)
        pub fn get_candle_winner(&mut self) -> Option<(AccountId, Balance)> {
            // To get winner by candle:
            //   1. Auction should be Ended;
            //   2. [optimisation] There should be (at least one) winning candidate
            if (self.get_status() == Status::Ended) && (self.winning.is_some()) {
                // if winner already defined => just return her
                if let Some(winner) = self.winner {
                    return Some(winner);
                }

                // Determine winner by random candle blowing
                // additional random source = caller address used as seed
                self.winner = self.blow_candle(Self::env().caller().as_ref());
                return self.winner;
            }
            None
        }

        /// Message to place a bid.  
        /// An account can bid by sending the lacking amount so that total amount she sent to this contract covers the bid.  
        /// In any particual point of time, the user's top bid is equal to total balance she have sent to the contract.
        #[ink(message, payable)]
        pub fn bid(&mut self) {
            let now = self.env().block_number();
            let bidder = Self::env().caller();
            let bid_increment = self.env().transferred_balance();
            match self.handle_bid(bidder, bid_increment, now) {
                Err(Error::AuctionNotActive) => {
                    panic!("Auction isn't active!")
                }
                Err(Error::NotOutBidding(bid_new, bid_quo)) => {
                    panic!("You can't outbid {} with {}", bid_quo, bid_new)
                }
                Err(Error::WinningDataCorrupted) => {
                    panic!("Auction's winning data corrupted!")
                }
                Ok(()) => {}
            }
        }

        /// Message to claim the payout.  
        #[ink(message)]
        pub fn payout(&mut self) {
            const REWARD_METHODS: [fn(&CandleAuction, to: AccountId); 2] =
                [CandleAuction::give_nft, CandleAuction::give_domain];
            let caller = self.env().caller();
            // invoke reward method
            self.pay_back(REWARD_METHODS[usize::from(self.subject)], caller);
        }
    }

    /// Tests
    #[cfg(not(feature = "ink-experimental-engine"))]
    #[cfg(test)]
    mod tests {
        use super::*;
        use ink_env::balance as contract_balance;
        use ink_env::test::get_account_balance as user_balance;
        use ink_env::Clear;
        use ink_lang as ink;

        const DEFAULT_CALLEE_HASH: [u8; 32] = [0x06; 32];

        fn run_to_block<T>(n: T::BlockNumber)
        where
            T: ink_env::Environment,
        {
            let mut block = ink_env::block_number::<T>();
            while block < n {
                match ink_env::test::advance_block::<T>() {
                    Err(_) => {
                        panic!("Cannot add blocks to test chain!")
                    }
                    Ok(_) => block = ink_env::block_number::<T>(),
                }
            }
        }

        fn set_sender<T>(sender: AccountId, amount: T::Balance)
        where
            T: ink_env::Environment<Balance = u128>,
        {
            ink_env::test::push_execution_context::<Environment>(
                sender,
                ink_env::account_id::<Environment>(),
                1000000,
                amount,
                ink_env::test::CallData::new(ink_env::call::Selector::new([0x00; 4])), /* dummy */
            );
        }

        #[ink::test]
        #[should_panic]
        fn not_ended_no_payout() {
            // given
            // Alice and Bob
            let alice = ink_env::test::default_accounts::<Environment>()
                .unwrap()
                .alice;
            let bob = ink_env::test::default_accounts::<Environment>()
                .unwrap()
                .bob;

            // and an auction
            let mut auction = CandleAuction::new(
                Some(1),
                10,
                20,
                0,
                Hash::clear(),
                AccountId::from(DEFAULT_CALLEE_HASH),
            );
            run_to_block::<Environment>(27);

            // Alice bids
            set_sender::<Environment>(alice, 100);
            auction.bid();

            // then
            // as auction is still not ended
            // there is no winner
            // and hence payout is not possible
            // Bob calls for payout
            set_sender::<Environment>(bob, 100);
            auction.payout();

            // contract panics here
        }

        #[ink::test]
        fn new_works() {
            let candle_auction = CandleAuction::new(
                Some(10),
                5,
                10,
                0,
                Hash::clear(),
                AccountId::from(DEFAULT_CALLEE_HASH),
            );
            assert_eq!(candle_auction.start_block, 10);
            assert_eq!(candle_auction.get_status(), Status::NotStarted);
        }

        #[ink::test]
        fn new_default_start_block_works() {
            run_to_block::<Environment>(12);
            let candle_auction = CandleAuction::new(
                None,
                5,
                10,
                0,
                Hash::clear(),
                AccountId::from(DEFAULT_CALLEE_HASH),
            );
            assert_eq!(candle_auction.start_block, 13);
            assert_eq!(candle_auction.get_status(), Status::NotStarted);
        }

        #[ink::test]
        #[should_panic]
        fn cannot_init_backdated_auction() {
            run_to_block::<Environment>(27);
            CandleAuction::new(
                Some(1),
                10,
                20,
                0,
                Hash::clear(),
                AccountId::from(DEFAULT_CALLEE_HASH),
            );
        }

        #[ink::test]
        fn auction_statuses_returned_correctly() {
            // an auction with the following picture:
            //  [1][2][3][4][5][6][7][8][9][10][11][12][13]
            //     | opening  |             ending    |
            let candle_auction = CandleAuction::new(
                Some(2),
                4,
                7,
                0,
                Hash::clear(),
                AccountId::from(DEFAULT_CALLEE_HASH),
            );

            assert_eq!(candle_auction.get_status(), Status::NotStarted);
            run_to_block::<Environment>(1);
            assert_eq!(candle_auction.get_status(), Status::NotStarted);
            run_to_block::<Environment>(2);
            assert_eq!(candle_auction.get_status(), Status::OpeningPeriod);
            run_to_block::<Environment>(5);
            assert_eq!(candle_auction.get_status(), Status::OpeningPeriod);
            run_to_block::<Environment>(6);
            assert_eq!(candle_auction.get_status(), Status::EndingPeriod(1));
            run_to_block::<Environment>(12);
            assert_eq!(candle_auction.get_status(), Status::EndingPeriod(7));
            run_to_block::<Environment>(13);
            assert_eq!(candle_auction.get_status(), Status::Ended);
        }

        #[ink::test]
        #[should_panic]
        fn cannot_bid_until_started() {
            // given
            // default account (Alice)

            // when
            // auction starts at block #5
            let mut auction = CandleAuction::new(
                Some(5),
                5,
                10,
                0,
                Hash::clear(),
                AccountId::from(DEFAULT_CALLEE_HASH),
            );

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
            let mut auction = CandleAuction::new(
                None,
                5,
                10,
                0,
                Hash::clear(),
                AccountId::from(DEFAULT_CALLEE_HASH),
            );

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
            let alice = ink_env::test::default_accounts::<Environment>()
                .unwrap()
                .alice;
            let mut auction = CandleAuction::new(
                None,
                5,
                10,
                0,
                Hash::clear(),
                AccountId::from(DEFAULT_CALLEE_HASH),
            );
            // when
            // Push block to 1 to make auction started
            run_to_block::<Environment>(1);

            // Alice bids 100
            set_sender::<Environment>(alice, 100);
            auction.bid();

            // then
            // bid is accepted
            assert_eq!(auction.balances.get(&alice), Some(&100));
            // and Alice is currently winning
            assert_eq!(auction.winning, Some(alice));

            // and
            // further Alice' bids are adding up to her balance
            run_to_block::<Environment>(2);
            set_sender::<Environment>(alice, 25);
            auction.bid();
            assert_eq!(auction.balances.get(&alice), Some(&125));
            // and Alice is still winning
            assert_eq!(auction.winning, Some(alice));
        }

        #[ink::test]
        fn winning_data_constructed_correctly() {
            // given
            // an auction with the following structure:
            //  [1][2][3][4][5][6][7][8][9][10][11][12][13]
            //     | opening  |        ending         |
            let mut auction = CandleAuction::new(
                Some(2),
                4,
                7,
                0,
                Hash::clear(),
                AccountId::from(DEFAULT_CALLEE_HASH),
            );

            // Alice and Bob
            let alice = ink_env::test::default_accounts::<ink_env::DefaultEnvironment>()
                .unwrap()
                .alice;
            let bob = ink_env::test::default_accounts::<ink_env::DefaultEnvironment>()
                .unwrap()
                .bob;

            // when
            // there is no bids
            // then
            // winning_data initialized with Nones
            assert_eq!(auction.winning_data, [None; 8].iter().map(|o| *o).collect());

            // when
            // there are bids in opening period
            run_to_block::<Environment>(3);
            // Alice bids 100
            set_sender::<Environment>(alice, 100);
            auction.bid();

            run_to_block::<Environment>(5);
            // Bob bids 100
            set_sender::<Environment>(bob, 101);
            auction.bid();

            // then
            // the top of these bids goes to index 0
            assert_eq!(
                auction.winning_data,
                [Some((bob, 101)), None, None, None, None, None, None, None]
                    .iter()
                    .map(|o| *o)
                    .collect()
            );

            // when
            // bids added in Ending Period
            run_to_block::<Environment>(7);
            // Alice bids 102
            set_sender::<Environment>(alice, 2);
            auction.bid();

            run_to_block::<Environment>(9);
            // Bob bids 103
            set_sender::<Environment>(bob, 2);
            auction.bid();

            run_to_block::<Environment>(11);
            // Alice bids 104
            set_sender::<Environment>(alice, 2);
            auction.bid();

            // then
            // bids are accounted for correclty
            assert_eq!(
                auction.winning_data,
                [
                    Some((bob, 101)),
                    None,
                    Some((alice, 102)),
                    None,
                    Some((bob, 103)),
                    None,
                    Some((alice, 104)),
                    None
                ]
                .iter()
                .map(|o| *o)
                .collect()
            );
        }

        #[ink::test]
        fn no_winner_until_ended() {
            // given
            // Alice and Bob
            let alice = ink_env::test::default_accounts::<ink_env::DefaultEnvironment>()
                .unwrap()
                .alice;
            let bob = ink_env::test::default_accounts::<ink_env::DefaultEnvironment>()
                .unwrap()
                .bob;
            // and an auction
            let mut auction = CandleAuction::new(
                None,
                5,
                10,
                0,
                Hash::clear(),
                AccountId::from(DEFAULT_CALLEE_HASH),
            );
            // when
            // auction starts
            run_to_block::<Environment>(1);
            // Alice bids 100
            set_sender::<Environment>(alice, 100);
            auction.bid();

            run_to_block::<Environment>(15);
            // Bob bids 101
            set_sender::<Environment>(bob, 101);
            auction.bid();

            // then
            // no winner yet determined
            assert_eq!(auction.get_candle_winner(), None);
        }

        #[ink::test]
        fn winner_is_random_and_no_override() {
            // given
            // an auction with the following structure:
            //  [1][2][3][4][5][6][7][8][9][10][11][12][13]
            //     | opening  |        ending         |
            let mut auction = CandleAuction::new(
                Some(2),
                4,
                7,
                0,
                Hash::clear(),
                AccountId::from(DEFAULT_CALLEE_HASH),
            );

            // Alice and Bob
            let alice = ink_env::test::default_accounts::<ink_env::DefaultEnvironment>()
                .unwrap()
                .alice;
            let bob = ink_env::test::default_accounts::<ink_env::DefaultEnvironment>()
                .unwrap()
                .bob;

            // when
            // there are bids in opening period
            run_to_block::<Environment>(3);
            // Alice bids 100
            set_sender::<Environment>(alice, 100);
            auction.bid();

            run_to_block::<Environment>(5);
            // Bob bids 100
            set_sender::<Environment>(bob, 101);
            auction.bid();

            // when
            // bids added in Ending Period
            run_to_block::<Environment>(7);
            // Alice bids 102
            set_sender::<Environment>(alice, 2);
            auction.bid();

            run_to_block::<Environment>(9);
            // Bob bids 103
            set_sender::<Environment>(bob, 2);
            auction.bid();

            run_to_block::<Environment>(11);
            // Alice bids 104
            set_sender::<Environment>(alice, 2);
            auction.bid();

            // auction ends
            run_to_block::<Environment>(13);

            // auction.winning_data:
            //     [
            //         Some((bob, 101)),
            //         None,
            //         Some((alice, 102)),
            //         None,
            //         Some((bob, 103)),
            //         None,
            //         Some((alice, 104)),
            //         None
            //     ]

            // then
            // candle winner is detected
            let w1 = auction.get_candle_winner().unwrap();
            auction.winner.expect("Candle winner SHOULD be detected!");

            // and
            // winner detection is likely to be randomized:
            //   should be 4^-10 ~ less than _one in a million_ chance
            //   that candle selects the same 1 out of 4 bids
            //   all 10 times in a row
            let mut candles = Vec::<(AccountId, Balance)>::new();
            candles.push(w1);
            for i in 1..10 {
                run_to_block::<Environment>(13 + i);
                // this one fails in 50% test runs because of not enough known_since block randomization
                candles.push(auction.blow_candle(&b"blablabla"[..]).unwrap());
                // and
                // winner cannot be overriden
                assert_eq!(
                    auction.winner.unwrap(),
                    auction.get_candle_winner().unwrap()
                );
            }
            // this one can fail once in 1048576 times:
            assert_ne!(
                candles,
                [w1; 10]
                    .iter()
                    .map(|o| *o)
                    .collect::<Vec::<(AccountId, Balance)>>(),
                "candle should be random!"
            );
        }
        // Candle test cases:
        // cannot:
        //  1. get winner until ended (V)
        //  2. override the winner (V)

        // should:
        //  3. if all but 1 None -> winner is 1
        //  4. (very likely) (V)
        // also:
        //  5. reward should payback difference between won bid and balance, to winner
        // #[ink::test]
        // fn

        #[ink::test]
        fn dns_auction_new_works() {
            let auction_with_domain = CandleAuction::new(
                Some(10),
                5,
                10,
                1,
                Hash::from([0x99; 32]),
                AccountId::from(DEFAULT_CALLEE_HASH),
            );
            assert_eq!(auction_with_domain.start_block, 10);
            assert_eq!(auction_with_domain.domain, Hash::from([0x99; 32]));
            assert_eq!(auction_with_domain.get_status(), Status::NotStarted);

            let auction_no_domain = CandleAuction::new(
                Some(10),
                5,
                10,
                1,
                Hash::clear(),
                AccountId::from(DEFAULT_CALLEE_HASH),
            );

            assert_eq!(auction_no_domain.domain, Hash::clear());
        }

        // We can't check that winner get rewarded in offchain tests,
        // as it requires cross-contract calling.
        // Hence we check here just that the winner is determined
        // and the looser can get his bidded amount back
        #[ink::test]
        #[ignore = "obsolete non-candle test"]
        fn noncandle_win_and_payout_work() {
            // given
            // Charlie is auction owner
            let charlie = ink_env::test::default_accounts::<ink_env::DefaultEnvironment>()
                .unwrap()
                .charlie;
            // Alice and Bob are bidders
            let alice = ink_env::test::default_accounts::<ink_env::DefaultEnvironment>()
                .unwrap()
                .alice;
            let bob = ink_env::test::default_accounts::<ink_env::DefaultEnvironment>()
                .unwrap()
                .bob;

            // Charlie sets up an auction
            set_sender::<Environment>(charlie, 1000);
            let mut auction = CandleAuction::new(
                None,
                5,
                10,
                0,
                Hash::clear(),
                AccountId::from(DEFAULT_CALLEE_HASH),
            );

            // when
            // auction starts
            run_to_block::<Environment>(1);

            // Alice bids 100
            set_sender::<Environment>(alice, 100);
            auction.bid();

            run_to_block::<Environment>(15);
            // Bob bids 101
            set_sender::<Environment>(bob, 101);
            auction.bid();

            // Auction ends
            run_to_block::<Environment>(17);

            // then
            // Bob wins (with bid 101)
            assert_eq!(auction.get_noncandle_winner(), Some(bob));

            // dirty hack
            // TODO: report problem: contract balance isn't changed with called payables
            ink_env::test::set_account_balance::<Environment>(
                ink_env::account_id::<Environment>(),
                100000000,
            )
            .unwrap();

            // balances: [alice's, bob's, contract's]
            let balances_before = [
                user_balance::<Environment>(alice).unwrap(),
                user_balance::<Environment>(bob).unwrap(),
                contract_balance::<Environment>(),
            ];
            ink_env::debug_println!("balances_before: {:?}", balances_before);

            // we don't check payout claimed by winner Bob
            // offchain env does not support cross-contract calling
            // auction.payout();

            // payout claimed by looser Alice
            set_sender::<Environment>(alice, 0);
            auction.payout();

            let balances_after = [
                user_balance::<Environment>(alice).unwrap(),
                user_balance::<Environment>(bob).unwrap(),
                contract_balance::<Environment>(),
            ];
            ink_env::debug_println!("balances_after: {:?}", balances_after);

            let mut balances_diff = [0; 3];
            for i in 0..3 {
                balances_diff[i] = balances_after[i].wrapping_sub(balances_before[i]);
            }

            // then
            // Alice gets back her bidded amount => diff = +100
            // Bob as winner gets no money back => diff = 0
            // Contract pays that amount to Alice => diff = -100
            // balances_diff == [100,0,-100]
            assert_eq!(balances_diff, [100, 0, 0u128.wrapping_sub(100)]);

            // and
            // Contract ledger cleared
            // Again, except winner's balance,
            // which will be cleared once he claims the reward,
            // which cannot be tested in offchain env
            assert_eq!(auction.balances.len(), 1);
        }

        #[ink::test]
        #[ignore = "obsolete non-candle test"]
        fn no_noncandle_winner_until_ended() {
            // given
            // Alice and Bob
            let alice = ink_env::test::default_accounts::<ink_env::DefaultEnvironment>()
                .unwrap()
                .alice;
            let bob = ink_env::test::default_accounts::<ink_env::DefaultEnvironment>()
                .unwrap()
                .bob;
            // and an auction
            let mut auction = CandleAuction::new(
                None,
                5,
                10,
                0,
                Hash::clear(),
                AccountId::from(DEFAULT_CALLEE_HASH),
            );
            // when
            // auction starts
            run_to_block::<Environment>(1);
            // Alice bids 100
            set_sender::<Environment>(alice, 100);
            auction.bid();

            run_to_block::<Environment>(15);
            // Bob bids 101
            set_sender::<Environment>(bob, 101);
            auction.bid();

            // then
            // no winner yet determined
            assert_eq!(auction.get_noncandle_winner(), None);
        }
    }
}
