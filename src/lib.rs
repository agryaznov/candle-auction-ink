// (c) 2021 Alexader Gryaznov
//
//! Candle Auction implemented with Ink! smartcontract

#![cfg_attr(not(feature = "std"), no_std)]
use ink_lang as ink;

#[ink::contract]
/// Candle Auction module
mod candle_auction {
    use ink_env::{
        call::{build_call, utils::ReturnType, ExecutionInput, Selector},
        transfer,
    };
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
        NotOutBidding(Balance, Balance),
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

    /// Event emitted when a nft_payout happened.
    #[ink(event)]
    pub struct PayoutNFT {
        #[ink(topic)]
        to: Option<AccountId>,
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
        /// Bidders balances storage.  
        /// Current user's balance = her top bid
        balances: StorageHashMap<AccountId, Balance>,
        /// *winning* <bidder> = current top bidder.  
        /// Not to be confused with *winner* = bidder who finally won.   
        winning: Option<AccountId>,
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
        domain: Option<Hash>,
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
            subject: Option<u8>,
            domain: Option<Hash>,
            reward_contract_address: AccountId,
        ) -> Self {
            let subj = match subject {
                None => {
                    // default is NFT auction
                    0
                }
                Some(0) => 0,
                Some(1) => {
                    // if the auction is for dns,
                    // the domain name should be specified on init
                    domain.expect("Domain name put up for auction should be specified!");
                    1
                }
                _ => {
                    panic!("Only subjects [0,1] are supported so far!")
                }
            };

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
                start_block: start_in,
                opening_period,
                ending_period,
                balances: StorageHashMap::new(),
                winning: None,
                winning_data,
                reward_contract_address,
                subject: subj,
                domain,
            }
        }

        /// Auction status.
        fn status(&self, block: BlockNumber) -> AuctionStatus {
            let opening_period_last_block = self.start_block + self.opening_period - 1;
            let ending_period_last_block = opening_period_last_block + self.ending_period;

            if block >= self.start_block {
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
                AuctionStatus::OpeningPeriod => 0,
                AuctionStatus::EndingPeriod(o) => o,
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
                Ok(_) => Ok(()),
            }
        }

        /// Pay back.
        /// Winner gets her reward.
        /// Loosers get their balances back.  
        fn pay_back(&mut self, reward: fn(&Self, to: AccountId) -> (), to: AccountId) {
            // should be executed only on Ended auction
            assert_eq!(
                self.get_status(),
                AuctionStatus::Ended,
                "Auction is not Ended, no payback is possible!"
            );

            if let Some(winner) = self.get_winner() {
                // winner gets her reward
                if to == winner {
                    // remove winner balance from ledger: it's not her money anymore
                    self.balances.take(&winner);
                    // reward winner with specified reward method call
                    reward(&self, to);
                    return;
                }
            }

            // pay the looser his bidded amount back
            let bal = self.balances.take(&to).unwrap();
            // zero-balance check: bid 0 is possible, but nothing to pay back
            if bal > 0 {
                // and pay
                transfer::<Environment>(to, bal).unwrap();
            }
        }

        /// Pluggable reward logic: OPTION-1.    
        /// Reward with NFT(s) (ERC721).  
        /// Contract rewards an auction winner by giving her approval to transfer
        /// ERC721 tokens on behalf of the auction contract.  
        ///
        /// DESIGN DECISION: we call ERC721 set_approval_for_all() instead of approve() for  
        ///  1. the sake of simplicity, no need to specify TokenID  
        ///     as we need to send this token to the contract anyway,  _ater_ instantiation
        ///     but still _before_ auctions starts
        ///  2. this allows to set auction for collection of tokens instead of just for one thing
        ///
        /// Cross conract call to ERC721 set_approval_for_all() method  
        /// which is expected to have the selector: 0xFEEDBABE   
        fn give_nft(&self, to: AccountId) {
            let selector = Selector::new([0xFE, 0xED, 0xBA, 0xBE]);
            let params = build_call::<Environment>()
                .callee(self.reward_contract_address)
                .exec_input(ExecutionInput::new(selector).push_arg(to).push_arg(true))
                .returns::<ReturnType<Result<(), Error>>>();

            match params.fire() {
                Ok(v) => {
                    ink_env::debug_println!(
                        "Received return value \"{:?}\" from contract {:?}",
                        v,
                        self.reward_contract_address
                    );
                }
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
                                "Got error \"{:?}\" while trying to call {:?} with SELECTOR: {:?}",
                                e,
                                self.reward_contract_address,
                                selector.to_bytes()
                            );
                            panic!("{}", msg)
                        }
                    }
                }
            }
        }

        /// Pluggable reward logic: OPTION-2.    
        /// Reward with domain name.  
        /// Contract rewards an auction winner by transferring her auctioned
        /// domain name using the dns contract.
        ///
        /// Cross conract call to ERC721 set_approval_for_all() method,  
        /// which is expected to have the selector: 0xFEEDDEED   
        fn give_domain(&self, to: AccountId) {
            // TODO: DRY, as it is almost the same as give_nft
            let selector = Selector::new([0xFE, 0xED, 0xDE, 0xED]); // <- 1 of 2 only differences
            let params = build_call::<Environment>()
                .callee(self.reward_contract_address)
                .exec_input(
                    ExecutionInput::new(selector)
                        .push_arg(self.domain) // <- 2 of 2 only differences
                        .push_arg(to),
                )
                .returns::<ReturnType<Result<(), Error>>>();

            match params.fire() {
                Ok(v) => {
                    ink_env::debug_println!(
                        "Received return value \"{:?}\" from contract {:?}",
                        v,
                        self.reward_contract_address
                    );
                }
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
                                "Got error \"{:?}\" while trying to call {:?} with SELECTOR: {:?}",
                                e,
                                self.reward_contract_address,
                                selector.to_bytes()
                            );
                            panic!("{}", msg)
                        }
                    }
                }
            }
        }

        /// Message to get the status of the auction given the current block number.
        #[ink(message)]
        pub fn get_status(&self) -> AuctionStatus {
            let now = self.env().block_number();
            self.status(now)
        }

        #[ink(message)]
        pub fn get_winner(&self) -> Option<AccountId> {
            // temporary same as noncandle
            self.get_noncandle_winner()
        }

        /// Message to get auction winner in noncandle fashion.  
        /// To avoid ambiguity, winner is determined once the auction ended.  
        pub fn get_noncandle_winner(&self) -> Option<AccountId> {
            if self.get_status() == AuctionStatus::Ended {
                self.winning
            } else {
                None
            }
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
            // TODO: give contract owner right to withdraw winner's bid
        }
    }

    /// Tests
    #[cfg(not(feature = "ink-experimental-engine"))]
    #[cfg(test)]
    mod tests {
        use ink_lang as ink;
        use super::*;
        use ink_env::balance as contract_balance;
        use ink_env::test::get_account_balance as user_balance;

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
                None,
                None,
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
                None,
                None,
                AccountId::from(DEFAULT_CALLEE_HASH),
            );
            assert_eq!(candle_auction.start_block, 10);
            assert_eq!(candle_auction.get_status(), AuctionStatus::NotStarted);
        }

        #[ink::test]
        fn new_default_start_block_works() {
            run_to_block::<Environment>(12);
            let candle_auction = CandleAuction::new(
                None,
                5,
                10,
                None,
                None,
                AccountId::from(DEFAULT_CALLEE_HASH),
            );
            assert_eq!(candle_auction.start_block, 13);
            assert_eq!(candle_auction.get_status(), AuctionStatus::NotStarted);
        }

        #[ink::test]
        #[should_panic]
        fn cannot_init_backdated_auction() {
            run_to_block::<Environment>(27);
            CandleAuction::new(
                Some(1),
                10,
                20,
                None,
                None,
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
                None,
                None,
                AccountId::from(DEFAULT_CALLEE_HASH),
            );

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
            let mut auction = CandleAuction::new(
                Some(5),
                5,
                10,
                None,
                None,
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
                None,
                None,
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
                None,
                None,
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
                None,
                None,
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
                None,
                None,
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

        // We can't check that winner get rewarded in offchain tests,
        // as it requires cross-contract calling.
        // Hence we check here just that the winner is determined
        // and the looser can get his bidded amount back
        #[ink::test]
        fn noncandle_win_and_payout_work() {
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
                None,
                None,
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
    }
}
