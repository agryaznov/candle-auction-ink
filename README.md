> :bangbang: This is a **Work in Progress**.  
> Current status: [Milestone-1](https://github.com/w3f/Grants-Program/blob/master/rfps/candle-auction.md#milestone-1---basic-auction) 95% completed.  
> **Use at your own risk**. 

# 🕯️ Candle Auctions on Ink! 🎃
This is an [Ink!](https://github.com/paritytech/ink) smartcontract implementing a [candle auction](https://github.com/paritytech/ink) logic.

## Design Considerations

- Contract logic is heavily inspired by [parachain auction](https://github.com/paritytech/polkadot/blob/master/runtime/common/src/auctions.rs) implementation.
- Auction is initialized by setting Opening\Ending periods in block numbers.   
  ```rust
  // example of an auction schedule:
  //  [1][2][3][4][5][6][7][8][9][10][11][12][13]
  //     | opening  |        ending         |   
  ```
- The contract accepts payments and records participants` balances.
- Bids storage is a *HashMap* which stores only a top bid per user, therefore serving as users` balances ledger.  
- In order to make *candle* logic possible, we also store `winning_data` in featured *StorageVec* which holds bids for every *sample*.
- *Sample* is a number of consequent blocks identifying a time interval inside Ending Period.  
  In *PoC* version, sample equals to a single block. This will be enhanced later to be a configurable parameter.  
- The *winning sample* (i.e. in which candle "went out") will be selected retrospectively after Ending period ends.  
- Bids are made by transferring an amount to increment current bidder's balance which effectively equals her top bid at any point of time.  
  > E.g. Alice making calls:  
  > 1. `bid()` with `101` `<Balance>` <- Alice' top bid is 101   
  > some time later, she calls 
  > 2. `bid()` again, with `1000` `<Balance>` <- Alice' top bid now is 1101 (*not 1000*)


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
Find `candle_auction.contract` in the `target/ink` folder,  
and deploy it through the [Canvas UI](https://paritytech.github.io/canvas-ui/#/) by following [these  instructions](https://docs.substrate.io/tutorials/v3/ink-workshop/pt1/#running-a-substrate-smart-contracts-node).

### Use it!
1. Instantiate contract by setting following parameters:
+ `start_block`  
  number of block in which the auction starts.  
+ `opening_period`  
  duration of Opening Period in blocks
+ `ending_period`  
  duration of Ending Period in blocks

2. Place bids by invoking `bid()` method with an attached payment.  
   > _**!NOTE** on bids design_:      
   > bids are accepted at *incremental manner*, i.e. every bid adds up to bidder's balance which effectively compounds her top (highest) bid.
   > E.g. Alice making calls:  
   > 1. `bid()` with `101` `<Balance>` <- Alice' top bid is 101   
   > some time later, she calls 
   > 2. `bid()` again, with `1000` `<Balance>` <- Alice' top bid now is 1101 (*not 1000*)

3. Get the *(non-candle)* winner by invoking the `winner()` method. 
4. `[TDB]` Claim rewards (winner)
5. `[TDB]` Claim your funds (others)
6. `[TDB]` Retroactive `candle` winner determination.

## Check the Docs out
```
cd candle-auction-ink
cargo +nightly doc --lib --no-deps --document-private-items --open
```

### Further Learn
For newbies, it is _highly recommended_ to go through the gorgeous [Ink! Workshop](https://docs.substrate.io/tutorials/v3/ink-workshop/pt1/) on [substrate.dev](https://substrate.dev) the portal.

See [Ink! docs](https://paritytech.github.io/ink-docs/) for developer documentation.


## License

[Apache License 2.0](https://choosealicense.com/licenses/apache-2.0/) © Alexander Gryaznov