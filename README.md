> :bangbang: This is a **Work in Progress**.  
> Current status: [Milestone-2](https://github.com/w3f/Grants-Program/blob/master/applications/candle_auction_ink.md#milestone-2---random-close) :heavy_check_mark: completed.  
> **Use at your own risk**. 

# 🕯️ Candle Auctions on Ink! 🎃
This is an [Ink!](https://github.com/paritytech/ink) smartcontract implementing a [candle auction](https://en.wikipedia.org/wiki/Candle_auction) logic.

With this contract, one can set up a candle auction for a **NFT collection** or a **domain name**!  

See my [blogpost](https://agryaznov.com/2021/12/06/candle-auction-ink/) with the rationale and detailed design description of the contract. 

## Design Considerations
**Basic features**   
- Contract logic is heavily inspired by the [parachain auction](https://github.com/paritytech/polkadot/blob/master/runtime/common/src/auctions.rs) implementation.
- Auction is initialized by setting Opening\Ending periods in block numbers.   
  ```rust
  // example of an auction schedule:
  //  [1][2][3][4][5][6][7][8][9][10][11][12][13]
  //     | opening  |        ending         |   
  ```
- The contract accepts payments and records participants` balances.
- Bidders balances are stored as a *HashMap* which effectively presents top bid per user.  
- Bids are made by transferring a bid amount to contract with invoking `bid()` payable method. 
- *Pluggable reward logic*: auction reward method can be one of provided options and should be specified on contract initiation.
- Reward logic is executed by cross-contract method invocation: this very contract communicates with specified `ERC721` or `DNS` contract instance, depending on which auction subject has been set up.      
  Low-level *ink_env::call::CallBuilder* is preferred over *ink-as-dependency* way, for the sake of *loosely coupling*.  
- Auction finalization, i.e. winner determination, is invoked by calling `find_winner()` method.  
  This can be done by anyone generous enough to pay for gas. However, due to specifics of secure random number generation on-chain, this is allowed to be done not earlier than `RF_DELAY` blocks after the last block of the *Ending period* has beem sealed to chain.  
- Payouts can be claimed once auction is finalized, on per user basic by `payout()` method invocation:  
  - winner is paid by the specified reward logic  
    (e.g. a domain name transferral or an approval to became some NFT tokens operator);  
    in case not the highest of her bids wins, the winner also gets the *change* paid back;
  - other bidders are paid by recieving their bidded amounts back;  
  - auction owner is paid by recieving the winning bid amount.

**Candle-fashioned**   
- In order to make *candle* logic possible, we also store `winning_data` in featured *StorageVec* which holds bids for every *sample*.
- *Sample* is a number of consequent blocks identifying a time interval inside *Ending period*.  
  In *PoC* version, sample equals to a single block. This could be enhanced later to be a configurable parameter.  
- The *winning sample* (i.e. in which candle "went out") will be selected retrospectively after *Ending period* ends.  
- Major feature what makes this auction _candle-fashioned_ is the randomness of winning sample selection.  
  The contract allows you to configure the source of this randomness (see [entropy module](src/entropy.rs)). By default, it uses `ink_env::random()` function which in turn utilizes [randomness-collective-flip](https://github.com/paritytech/substrate/blob/v3.0.0/frame/randomness-collective-flip/src/lib.rs#L113) module. The latter provides generator of low-influence random values based on the block hashes from the last `81` blocks. It means that when using this particular random function, it is required to wait at least 81 blocks after the last block of *Ending* period until invoking the function to get a random block inside that period. 


Please see [cargo docs](#check-the-docs-out) and comments in code for deeper details. 

## How to
### Install Prerequisites
Please follow installation instructions provided [here](https://docs.substrate.io/tutorials/v3/ink-workshop/pt1/#prerequisites).

### Clone this repo
```
git clone https://github.com/agryaznov/candle-auction-ink
```

### Compile + Run Tests
```
cd candle-auction-ink
cargo +nightly test
```

### Build Contract + metadata
```
cargo +nightly contract build
```

### Deploy
First we deploy __*rewarding* contracts__ which represent entities being auctioned. After that, we deploy the auction itself.

#### External rewarding contracts
Two *pluggable reward* options are available:  
  1. **NFT collection**: by utilizing the [ERC721](https://github.com/agryaznov/ink/blob/candle-auction/examples/erc721/lib.rs) contract  
    winner gets *set_approval_for_all()* tokens belonging to the auction contract  
  2. **Doman name ownership**: by utilizing the [DNS](https://github.com/agryaznov/ink/blob/candle-auction/examples/dns/lib.rs) contract  
    which transfers to winner the domain name  put up for the auction    

In order to make the auction contract preferably *loosely coupled* with other contracts, this very contract doesn't use their sources *as-a-dependency*. Instead, we rely just on these external contracts ABI, and *hope that their main methods selectors will stay consistent*.  

Okay, though, to guarantee this, let's use our fork of their codebase repo with explicit selectors:

```
git clone -b candle-auction git@github.com:agryaznov/ink.git
```

Then build the contracts:

```
cd ink/examples/erc721
cargo +nightly contract build
cd ../dns
cargo +nightly contract build
```

Then deploy them through the [*PolkadotJS apps /contracts tab*](https://polkadot.js.org/apps/?rpc=ws%3A%2F%2F127.0.0.1%3A9944#/contracts)

#### Candle auction contract
Find `candle_auction.contract` in the `target/ink` folder,  
and deploy it.

### Use it!
**Prepare/Launch**:  

1. Instantiate the contract by setting following parameters:
+ `start_block`  
  number of block in which the auction starts; by default it's the next block after contract instantiation;  
+ `opening_period`  
  duration of Opening Period in blocks
+ `ending_period`  
  duration of Ending Period in blocks
+ `subject`  
  auction subject:   
  - `0` = NFTs  
  - `1` = DNS
  - `2..255` = reserved for further reward methods
+ `domain`  
  in case of DNS subject, the domain name to bid for     
+ `reward_contract_address`  
  address of the rewarding contract: [*ERC721*](https://github.com/agryaznov/ink/blob/candle-auction/examples/erc721/lib.rs) or [*DNS*](https://github.com/agryaznov/ink/blob/candle-auction/examples/dns/lib.rs)  

2. Pass the auctioned entities ownership to the contract:  
   transfer NFT tokens / domain names to the instantiated auction contract.  

  > **_:exclamation:NOTE_** that sanity checks, like: *does the auction contract really possess the entities being bidded for?* - those ones are left totally to user's discretion.    

**Action!**:  

3. Place bids by invoking `bid()` method with an attached payment.  

4. Get current auction status by `get_status()` and current winning bid and account by `get_winning()` methods invocation.  

5. Once auction is ended, anyone can invoke `find_winner()` method to randomly detect a block during Ending period and set the auction winner to be the top bidder of that block. This effectively emulates candle blow for the auction.  
   > _**:exclamation:NOTE-1**_ that `random()` function [implementation](https://github.com/paritytech/substrate/blob/v3.0.0/frame/randomness-collective-flip/src/lib.rs#L113) used in *substrate-contract-node*
   > takes 81 block back in time to produce seed secure enough to use.
   > As follows from [the function docs](https://docs.substrate.io/rustdocs/latest/frame_support/traits/trait.Randomness.html#tymethod.random),
   > the returned seed should be used only to distinguish commitments made _after_ the first block of that 81 blocks sequence.  
   > In other words, **`find_winner()` should be called not earlier than 81 block after the auction ended**.

   > _**:exclamation:NOTE-2**_ If first bids come in block late enough, it is possible that candle "*goes out*" before that block. In such a case, __a finalized auction with `None` winner is expected outcome__. Every bidders get claim their money back.

**Settlement**:

6. Once auction is done, participants (including the contract owner) can claim their payouts/rewards with `payout()`.  
   > **_:exclamation:NOTE_** that in NFT auction winner gets approval to transer all contract's ERC721 tokens with this. 
   She should then *transer* these tokens by herself by manually calling `transfer_from()` on that ERC721 contract.


## Check the Docs out
```
cargo +nightly doc --lib --no-deps --document-private-items --open
```

### Further Learn
For newbies, it is _highly recommended_ to go through the gorgeous [Ink! Workshop](https://docs.substrate.io/tutorials/v3/ink-workshop/pt1/) on [substrate.dev](https://substrate.dev) the portal.

See [Ink! docs](https://paritytech.github.io/ink-docs/) for developer documentation.


## License

[Apache License 2.0](https://choosealicense.com/licenses/apache-2.0/) © Alexander Gryaznov ([agryaznov.com](https://agryaznov.com))
