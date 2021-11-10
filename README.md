> :bangbang: This is a **Work in Progress**.  
> Current status: [Milestone-1](https://github.com/w3f/Grants-Program/blob/master/applications/candle_auction_ink.md#milestone-1---basic-auction) :heavy_check_mark: completed.  
> **Use at your own risk**. 

# 🕯️ Candle Auctions on Ink! 🎃
This is an [Ink!](https://github.com/paritytech/ink) smartcontract implementing a [candle auction](https://github.com/paritytech/ink) logic.

With this contract, one can set up an auction for a **NFT collection** or a **domain name**!  

(Currently it's a basic auction. Candle one is WIP and will be delivered in [Milestone-2](https://github.com/w3f/Grants-Program/blob/master/rfps/candle-auction.md#milestone-2---random-close) shortly. Stay tuned!)

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
- Bidders balances as stored as a *HashMap* which effectively presents top bid per user.  
- Bids are made by transferring an amount to increment current bidder's balance which effectively equals her top bid at any point of time.  
  > E.g. Alice making calls:  
  > 1. `bid()` with `101` `<Balance>` <- Alice' top bid is 101   
  > some time later, she calls 
  > 2. `bid()` again, with `1000` `<Balance>` <- Alice' top bid now is 1101 (*not 1000*)
- *Pluggable reward logic*: auction reward method can be one of provided options and should be specified on contract initiation.
- Reward logic is executed by cross contract method invocation    
  low-level *ink_env::call::CallBuilder* is preferred over *ink-as-dependency* way, for the sake of *loosely coupling* 
- Payouts can be claimed once auction ended, on per user basic by `payout()` method invocation:  
  - winner is paid by specified reward logic  
    (e.g. a domain name transferral or an approval to became some NFT tokens operator);
  - other bidders are paid by recieving their bidded amounts back;  
  - auction owner is paid by recieving winning bid amount (winner's balance).

**Candle-fashioned**   
- In order to make *candle* logic possible, we also store `winning_data` in featured *StorageVec* which holds bids for every *sample*.
- *Sample* is a number of consequent blocks identifying a time interval inside Ending Period.  
  In *PoC* version, sample equals to a single block. This will be enhanced later to be a configurable parameter.  
- The *winning sample* (i.e. in which candle "went out") will be selected retrospectively after Ending period ends.  


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
#### External rewarding contracts
Two *pluggable reward* options are available:  
  + NFT collection: by utilizing the ERC721 contract  
    winner gets *set_approval_for_all()* tokens belonging to the auction contract  
  + Doman name ownership: by utilizing the DNS contract  
    which transfers to winner the domain name  put up for the auction    

In order to make the auction contract preferably *loosely coupled* with other contracts, this very contract doesn't use their sources *as-a-dependency*. Instead, we consciously rely just on these external contracts ABI, and *hope that their main methods selectors will stay consistent*.  
Okay, well, to guarantee this, let's use our fork of their codebase repo with explicit selectors:

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

Then deploy them through the [Canvas UI](https://paritytech.github.io/canvas-ui/#/) by following [these  instructions](https://docs.substrate.io/tutorials/v3/ink-workshop/pt1/#running-a-substrate-smart-contracts-node).

#### Candle auction contract
Find `candle_auction.contract` in the `target/ink` folder,  
and deploy it through the [Canvas UI](https://paritytech.github.io/canvas-ui/#/) by following [these  instructions](https://docs.substrate.io/tutorials/v3/ink-workshop/pt1/#running-a-substrate-smart-contracts-node).

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
  - `0` = NFTs `<-- default`
  - `1` = DNS
  - `2..255` = reserved for further reward methods
+ `domain`  
  in case of DNS subject, the domain name to bid for     
+ `reward_contract_address`  
  address of the rewarding contract: [*ERC721*](https://github.com/agryaznov/ink/blob/candle-auction/examples/erc721/lib.rs) or [*DNS*](https://github.com/agryaznov/ink/blob/candle-auction/examples/dns/lib.rs)  

2. Pass the auctioned entities ownership to the contract:  
   transfer NFT tokens / domain names to the instantiated auction contract.  

  > **_:exclamation:NOTE_** sanity checks, like: *does the auction contract really possess the entities being bidded for?* - those ones are left totally on user's discretion.    

**Action!**:  

1. Place bids by invoking `bid()` method with an attached payment.  
   > _**:exclamation:NOTE** on bids design_:      
   > bids are accepted at *incremental manner*, i.e. every bid adds up to bidder's balance which effectively compounds her top (highest) bid.
   > E.g. Alice making calls:  
   > 1. `bid()` with `101` `<Balance>` <- Alice' top bid is 101   
   > some time later, she calls 
   > 2. `bid()` again, with `1000` `<Balance>` <- Alice' top bid now is 1101 (*not 1000*)

2. Get current auction status by `get_status()` and winner `get_winner()` methods invocation.  

4a. `[TDB]` Retroactive `candle` winner determination.

**Settlement**:

5. Once auction is done, participants (and contract owner) can claim their payouts/rewards with `payout()`.  
   > **_:exclamation:NOTE_** that in NFT auction winner gets approval to transer all conrtact's ERC721 tokens with this.  
   She should then *transer* these tokens by herself by manually calling `transfer_from()` on that ERC721 contract.


## Check the Docs out
```
cargo +nightly doc --lib --no-deps --document-private-items --open
```

### Further Learn
For newbies, it is _highly recommended_ to go through the gorgeous [Ink! Workshop](https://docs.substrate.io/tutorials/v3/ink-workshop/pt1/) on [substrate.dev](https://substrate.dev) the portal.

See [Ink! docs](https://paritytech.github.io/ink-docs/) for developer documentation.


## License

[Apache License 2.0](https://choosealicense.com/licenses/apache-2.0/) © Alexander Gryaznov