> :bangbang: This is a **Work in Progress**.  
> Current status: [Milestone-1](https://github.com/w3f/Grants-Program/blob/master/rfps/candle-auction.md#milestone-1---basic-auction) 75% completed.  
> **Use at you own risk**. 

# 🕯️ Candle Auctions on Ink!
This is an [Ink!](https://github.com/paritytech/ink) smartcontract implementing a [candle auction](https://github.com/paritytech/ink) logic.

## How to
### Install Prerequisites
Please follow installation instructions provided [here](https://docs.substrate.io/tutorials/v3/ink-workshop/pt1/#prerequisites).

### Clone this repo
```
git clone https://github.com/agryaznov/candle-auction`
```

### Compile + Run Tests
```
cd candle-auction
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
   > E.g., Alice making calls:  
   > 1. `bid()` with `101` `<Balance>` <- Alice' top bid is 101   
   > some time later, she calls 
   > 2. `bid()` again, with `1000` `<Balance>` <- Alice' top bid now is 1101 (*not 1000*)

3. Get the *(non-candle)* winner by invoking the `winner()` method. 
4. `[TDB]` Claim rewards (winner)
5. `[TDB]` Claim your funds (others)
6. `[TDB]` Retroactive `candle` winner determination.

## Check the Docs out
```
cd candle-auction
cargo +nightly doc --lib --no-deps --document-private-items --open
```

### Further Learn
For newbies, it is _highly recommended_ to go through the gorgeous [Ink! Workshop](https://docs.substrate.io/tutorials/v3/ink-workshop/pt1/) on [substrate.dev](https://substrate.dev) the portal.

See [Ink! docs](https://paritytech.github.io/ink-docs/) for developer documentation.


## License

[Apache License 2.0](https://choosealicense.com/licenses/apache-2.0/) © Alexander Gryaznov