#![cfg_attr(not(feature = "std"), no_std)]

use codec::{Decode, Encode};
/// Edit this file to define custom logic or remove it if it is not needed.
/// Learn more about FRAME and the core library of Substrate FRAME pallets:
/// https://substrate.dev/docs/en/knowledgebase/runtime/frame

use frame_support::{decl_error, decl_event, decl_module, decl_storage, dispatch, ensure};
use frame_support::traits::Get;
use frame_system::ensure_signed;
use pallet_generic_asset::AssetIdProvider;
//use sp_core::crypto::{AccountId32, Ss58Codec};
#[cfg(feature = "std")]
use serde::{Deserialize, Serialize};
use sp_arithmetic::{FixedPointNumber, FixedU128};
use sp_arithmetic::traits::{CheckedAdd, CheckedDiv, CheckedMul, CheckedSub, UniqueSaturatedFrom};
use sp_core::H256;
use sp_runtime::DispatchError;
use sp_runtime::traits::Hash;
use sp_std::collections::vec_deque::VecDeque;
use sp_std::convert::TryInto;
use sp_std::str;
use sp_std::vec::Vec;


use crate::OrderType::{AskLimit, BidLimit};

//use sp_core::H256;
#[cfg(test)]
mod mock;

#[cfg(test)]
mod tests;


/// Configure the pallet by specifying the parameters and types on which it depends.
/// pallet_generic_asset::Trait bounds this DEX pallet with pallet_generic_asset. DEX is available
/// only for runtimes that also install pallet_generic_asset.
pub trait Trait: frame_system::Trait + pallet_generic_asset::Trait {
    /// Because this pallet emits events, it depends on the runtime's definition of an event.
    type Event: From<Event<Self>> + Into<<Self as frame_system::Trait>::Event>;
    /// Amount in SpendingAssetCurrency that must reserved to register a tradingPair
    type TradingPairReservationFee: Get<<Self as pallet_generic_asset::Trait>::Balance>;
}

// Pallets use events to inform users when important changes are made.
// https://substrate.dev/docs/en/knowledgebase/runtime/events
decl_event!(
	pub enum Event<T> where Hash = <T as frame_system::Trait>::Hash,
	                        AccountId = <T as frame_system::Trait>::AccountId{
		/// New Trading pair is created [TradingPairHash]
		TradingPairCreated(Hash),
		/// New Limit Order Created [OrderId,TradingPairID,OrderType,Price,Quantity,Trader]
		NewLimitOrder(Hash,Hash,OrderType,FixedU128,FixedU128,AccountId),
		/// Market Order - Unfilled [OrderId,TradingPairID,OrderType,Price,Quantity,Trader]
		UnfilledMarketOrder(Hash,Hash,OrderType,FixedU128,FixedU128,AccountId),
		/// Market Order - Filled [OrderId,TradingPairID,OrderType,Price,Quantity,Trader]
		FilledMarketOrder(Hash,Hash,OrderType,FixedU128,FixedU128,AccountId),
		/// Limit Order Fulfilled  [OrderId,TradingPairID,OrderType,Price,Quantity,Trader]
		FulfilledLimitOrder(Hash,Hash,OrderType,FixedU128,FixedU128,AccountId),
		/// Limit Order Partial Fill  [OrderId,TradingPairID,OrderType,Price,Quantity,Trader]
		PartialFillLimitOrder(Hash,Hash,OrderType,FixedU128,FixedU128,AccountId),
	}
);

// Errors inform users that something went wrong.
decl_error! {
	pub enum Error for Module<T: Trait> {
		/// Transaction contained Same AssetID for both base and quote.
		SameAssetIdsError,
		/// TradingPair already exists in the system
		TradingPairIDExists,
		/// Insufficent Balance to Execute
		InsufficientAssetBalance,
		/// Invalid Price or Quantity for a Limit Order
		InvalidPriceOrQuantityLimit,
		/// Invalid Price for a BidMarket Order
		InvalidBidMarketPrice,
		/// Invalid Quantity for a AskMarket Order
		InvalidAskMarketQuantity,
		/// TradingPair doesn't Exist
		InvalidTradingPair,
		/// Internal Error: Failed to Convert Balance to U128
		InternalErrorU128Balance,
		/// Element not found
		NoElementFound,
		///Underflow or Overflow because of checkedMul
		MulUnderflowOrOverflow,
		///Underflow or Overflow because of checkedDiv
		DivUnderflowOrOverflow,
		///Underflow or Overflow because of checkedAdd
		AddUnderflowOrOverflow,
		///Underflow or Overflow because of checkedSub
		SubUnderflowOrOverflow,
		///Error generated during asset transfer
		ErrorWhileTransferingAsset,
		///Failed to reserve amount
		ReserveAmountFailed,
		/// Invalid Origin
		InvalidOrigin,
		/// Price doesn't match with active order's price
		CancelPriceDoesntMatch,
		/// TradingPair mismatch
		TradingPairMismatch,
		/// Invalid OrderID
		InvalidOrderID
	}
}


decl_storage! {

	trait Store for Module<T: Trait> as DEXModule {
	// Stores all the different price levels for all the trading pairs in a DoubleMap.
	PriceLevels get(fn get_pricelevels): double_map hasher(identity) T::Hash, hasher(blake2_128_concat) FixedU128 => LinkedPriceLevel<T>;
	// Stores all the different active ask and bid levels in the system as a sorted vector mapped to it's TradingPair.
	AsksLevels get(fn get_askslevels): map hasher(identity) T::Hash => Vec<FixedU128>;
	BidsLevels get(fn get_bidslevels): map hasher(identity) T::Hash => Vec<FixedU128>;
	// Stores the Orderbook struct for all available trading pairs.
	Orderbooks get(fn get_orderbooks): map hasher(identity) T::Hash => Orderbook<T>;
	// Store MarketData of TradingPairs
	// If the market data is returning None, then no trades were present for that trading in that block.
	// TODO: Currently we store market data for all the blocks
	MarketInfo get(fn get_marketdata): double_map hasher(identity) T::Hash, hasher(blake2_128_concat) T::BlockNumber => Option<MarketData>;
	Nonce: u128;
	}
}



// Dispatchable functions allows users to interact with the pallet and invoke state changes.
// These functions materialize as "extrinsics", which are often compared to transactions.
// Dispatchable functions must be annotated with a weight and must return a DispatchResult.
decl_module! {
	pub struct Module<T: Trait> for enum Call where origin: T::Origin {
		// Errors must be initialized if they are used by the pallet.
		type Error = Error<T>;

		// Events must be initialized if they are used by the pallet.
		fn deposit_event() = default;

		// TODO: Note for enabling feeless trades use dispatch::DispatchResultWithPostInfo
		// TODO: then in the Ok(()) replace it with Ok(Some(0).into()) to make it fee-less

		/// Registers a new trading pair in the system
		#[weight = 10000]
		pub fn register_new_orderbook(origin, quote_asset_id: u32, base_asset_id: u32) -> dispatch::DispatchResultWithPostInfo{
		    let trader = ensure_signed(origin)?;
		    let a =

		    ensure!(!(&quote_asset_id == &base_asset_id), <Error<T>>::SameAssetIdsError);

		    // Checks the tradingPair whether exists
		    let trading_pair_id = Self::create_trading_pair_id(&quote_asset_id,&base_asset_id);
		    ensure!(!<Orderbooks<T>>::contains_key(&trading_pair_id), <Error<T>>::TradingPairIDExists);

		    // The origin should reserve a certain amount of SpendingAssetCurrency for registering the pair
		    ensure!(Self::reserve_balance_registration(&trader), <Error<T>>::InsufficientAssetBalance);
		    Self::create_order_book(quote_asset_id.into(),base_asset_id.into(),&trading_pair_id);
		    Self::deposit_event(RawEvent::TradingPairCreated(trading_pair_id));
		    Ok(Some(0).into())
	    }

        /// Submits the given order for matching to engine.
        #[weight = 10000]
	    pub fn submit_order(origin, order_type: OrderType, trading_pair: T::Hash, price: FixedU128, quantity: FixedU128) -> dispatch::DispatchResultWithPostInfo{
	        let trader = ensure_signed(origin)?;
   //         let account: AccountId32 = AccountId32::from(trader);
	        Self::execute_order(trader, order_type, trading_pair, price, quantity)?; // TODO: It maybe an error in which case take the fees else refund
	        Ok(Some(0).into())
	    }


	    /// Cancels the order
	    #[weight = 10000]
	    pub fn cancel_order(origin, order_id: T::Hash, trading_pair: T::Hash, price: FixedU128) -> dispatch::DispatchResultWithPostInfo {
	        let trader = ensure_signed(origin)?;

	        ensure!(<Orderbooks<T>>::contains_key(&trading_pair), <Error<T>>::InvalidTradingPair);
	        Self::cancel_order_from_orderbook(trader,order_id,trading_pair,price)?;
	        Ok(Some(0).into())
	    }
    }
}

#[derive(Encode, Decode, Clone, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "std", derive(Serialize, Deserialize))]
pub enum OrderType {
    BidLimit,
    BidMarket,
    AskLimit,
    AskMarket,
}

// #[serde(crate = "alt_serde")]


// #[serde(crate = "alt_serde")]
#[derive(Encode, Decode)]
#[cfg_attr(feature = "std", derive(Serialize, Deserialize))]
pub enum OrderTypeRPC {
    BidLimit,
    BidMarket,
    AskLimit,
    AskMarket,
}

#[derive(Encode, Decode, Clone, PartialEq, Eq)]
pub struct Order<T> where T: Trait {
    id: T::Hash,
    trading_pair: T::Hash,
    trader: T::AccountId,
    price: FixedU128,
    quantity: FixedU128,
    order_type: OrderType,
}

impl<T> Order<T> where T: Trait {
    pub fn convert(self) -> Order4RPC {
        Order4RPC {
            id: Self::account_to_bytes(&self.id).unwrap(),
            trading_pair: Self::account_to_bytes(&self.trading_pair).unwrap(),
            trader: Self::account_to_bytes(&self.trader).unwrap(),
            price: Self::convert_fixed_u128_to_balance(self.price).unwrap(),
            quantity: Self::convert_fixed_u128_to_balance(self.quantity).unwrap(),
            order_type: self.order_type,
        }
    }

    fn account_to_bytes<AccountId>(account: &AccountId) -> Result<[u8; 32], DispatchError>
        where AccountId: Encode,
    {
        let account_vec = account.encode();
        ensure!(account_vec.len() == 32, "AccountId must be 32 bytes.");
        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(&account_vec);
        Ok(bytes)
    }

    pub fn convert_fixed_u128_to_balance(x: FixedU128) -> Option<u128> {
        if let Some(balance_in_fixed_u128) = x.checked_div(&FixedU128::from(1000000)) {
            let balance_in_u128 = balance_in_fixed_u128.into_inner();
            Some(balance_in_u128)
        } else {
            None
        }
    }
}

// #[serde(crate = "alt_serde")]
#[derive(Encode, Decode, Eq, PartialEq)]
#[cfg_attr(feature = "std", derive(Serialize, Deserialize))]
pub struct Order4RPC {
    id: [u8; 32],
    trading_pair: [u8; 32],
    trader: [u8; 32],
    price: u128,
    quantity: u128,
    order_type: OrderType,
}

#[derive(Encode, Decode, Clone, PartialEq, Eq)]
pub struct LinkedPriceLevel<T> where T: Trait {
    next: Option<FixedU128>,
    prev: Option<FixedU128>,
    orders: VecDeque<Order<T>>,
}

impl<T> LinkedPriceLevel<T> where T: Trait {
    fn covert(self) -> LinkedPriceLevelRpc {
        LinkedPriceLevelRpc {
            next: Self::convert_fixed_u128_to_balance(self.next.unwrap()).unwrap(),
            prev: Self::convert_fixed_u128_to_balance(self.prev.unwrap()).unwrap(),
            orders: Self::cov_de_vec(self.clone().orders),
        }
    }

    fn cov_de_vec(temp: VecDeque<Order<T>>) -> Vec<Order4RPC> {
        let temp3: Vec<Order4RPC> = temp.into_iter().map(|element: Order<T>| element.convert()).collect();
        temp3
    }

    fn convert_fixed_u128_to_balance(x: FixedU128) -> Option<u128> {
        if let Some(balance_in_fixed_u128) = x.checked_div(&FixedU128::from(1000000)) {
            let balance_in_u128 = balance_in_fixed_u128.into_inner();
            Some(balance_in_u128)
        } else {
            None
        }
    }
}

// #[serde(crate = "alt_serde")]
#[derive(Encode, Decode, Eq, PartialEq)]
#[cfg_attr(feature = "std", derive(Serialize, Deserialize))]
pub struct LinkedPriceLevelRpc {
    next: u128,
    prev: u128,
    orders: Vec<Order4RPC>,
}


impl<T> Default for LinkedPriceLevel<T> where T: Trait {
    fn default() -> Self {
        LinkedPriceLevel {
            next: None,
            prev: None,
            orders: Default::default(),
        }
    }
}

#[derive(Encode, Decode, Clone, PartialEq, Eq, Debug)]
pub struct Orderbook<T> where T: Trait {
    trading_pair: T::Hash,
    base_asset_id: T::AssetId,
    quote_asset_id: T::AssetId,
    best_bid_price: FixedU128,
    best_ask_price: FixedU128,
}

impl<T> Orderbook<T> where T: Trait {
    fn convert(self) -> OrderbookRpc {
        OrderbookRpc {
            trading_pair: Self::account_to_bytes(&self.trading_pair).unwrap(),
            base_asset_id : TryInto::<u32>::try_into(self.base_asset_id).ok().unwrap(),
            quote_asset_id : TryInto::<u32>::try_into(self.quote_asset_id).ok().unwrap(),
            best_bid_price : Self::convert_fixed_u128_to_balance(self.best_bid_price).unwrap(),
            best_ask_price : Self::convert_fixed_u128_to_balance(self.best_ask_price).unwrap(),
        }
    }

    fn account_to_bytes<AccountId>(account: &AccountId) -> Result<[u8; 32], DispatchError>
        where AccountId: Encode,
    {
        let account_vec = account.encode();
        ensure!(account_vec.len() == 32, "AccountId must be 32 bytes.");
        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(&account_vec);
        Ok(bytes)
    }

    fn convert_fixed_u128_to_balance(x: FixedU128) -> Option<u128> {
        if let Some(balance_in_fixed_u128) = x.checked_div(&FixedU128::from(1000000)) {
            let balance_in_u128 = balance_in_fixed_u128.into_inner();
            Some(balance_in_u128)
        } else {
            None
        }
    }

}

impl<T> Default for Orderbook<T> where T: Trait {
    fn default() -> Self {
        Orderbook {
            trading_pair: T::Hash::default(),
            base_asset_id: 0.into(),
            quote_asset_id: 0.into(),
            best_bid_price: FixedU128::from(0),
            best_ask_price: FixedU128::from(0),
        }
    }
}

impl<T> Orderbook<T> where T: Trait {
    fn new(base_asset_id: T::AssetId, quote_asset_id: T::AssetId, trading_pair: T::Hash) -> Self {
        Orderbook {
            trading_pair,
            base_asset_id,
            quote_asset_id,
            best_bid_price: FixedU128::from(0),
            best_ask_price: FixedU128::from(0),
        }
    }
}
#[derive(Encode, Decode, Eq, PartialEq, Debug)]
#[cfg_attr(feature = "std", derive(Serialize, Deserialize))]
pub struct OrderbookRpc {
    trading_pair: [u8; 32],
    base_asset_id: u32,
    quote_asset_id: u32,
    best_bid_price: u128,
    best_ask_price: u128,
}


#[derive(Encode, Decode, Clone, PartialEq, Eq)]
pub struct MarketData {
    // Lowest price at which the trade was executed in a block.
    low: FixedU128,
    // Highest price at which the trade was executed in a block.
    high: FixedU128,
    // Total volume traded in a block.
    volume: FixedU128,
}

impl MarketData {
    fn convert (self) -> MarketDataRpc {
        MarketDataRpc {
            low: Self::convert_fixed_u128_to_balance(self.low).unwrap(),
            high: Self::convert_fixed_u128_to_balance(self.high).unwrap(),
            volume: Self::convert_fixed_u128_to_balance(self.volume).unwrap(),
        }

    }

    fn convert_fixed_u128_to_balance(x: FixedU128) -> Option<u128> {
        if let Some(balance_in_fixed_u128) = x.checked_div(&FixedU128::from(1000000)) {
            let balance_in_u128 = balance_in_fixed_u128.into_inner();
            Some(balance_in_u128)
        } else {
            None
        }
    }
}

#[derive(Encode, Decode, Eq, PartialEq, Debug)]
#[cfg_attr(feature = "std", derive(Serialize, Deserialize))]
pub struct MarketDataRpc {
    low: u128,
    high: u128,
    volume: u128,
}

impl<T: Trait> Module<T> {
    pub fn get_ask_level(trading_pair: T::Hash) -> Vec<FixedU128> {
        <AsksLevels<T>>::get(trading_pair)
    }

    pub fn get_bid_level(trading_pair: T::Hash) -> Vec<FixedU128> {
        <BidsLevels<T>>::get(trading_pair)
    }

    pub fn get_price_level(trading_pair: T::Hash) -> Vec<LinkedPriceLevelRpc> {
        let temp: Vec<LinkedPriceLevel<T>> = <PriceLevels<T>>::iter_prefix_values(&trading_pair).collect();
        let temp2: Vec<LinkedPriceLevelRpc> = temp.into_iter().map(|element| element.covert()).collect();
        temp2
    }

    pub fn get_orderbook(trading_pair: T::Hash) -> OrderbookRpc {
        let orderbook = <Orderbooks<T>>::get(trading_pair);

        orderbook.convert()
    }

    pub fn get_all_orderbook() -> Vec<OrderbookRpc> {
        let orderbook:Vec<OrderbookRpc> = <Orderbooks<T>>::iter().map(|(_key, value)| value).map(|orderbook| orderbook.convert()).collect();
        orderbook
    }

    pub fn get_market_info(trading_pair: T::Hash,blocknum: u32) -> MarketDataRpc {
        let blocknum = Self::u32_to_blocknum(blocknum);
        let temp = <MarketInfo<T>>::get(trading_pair, blocknum);
        temp.unwrap().convert()
    }

    pub fn u32_to_blocknum(input: u32) -> T::BlockNumber {
        input.into()
    }
}

impl<T: Trait> Module<T> {
    // Reserves TradingPairReservationFee (defined in configuration trait) balance of SpendingAssetCurrency
    fn reserve_balance_registration(origin: &<T as frame_system::Trait>::AccountId) -> bool {
        pallet_generic_asset::Module::<T>::reserve(
            &pallet_generic_asset::SpendingAssetIdProvider::<T>::asset_id(),
            origin, <T as Trait>::TradingPairReservationFee::get()).is_ok()
    }

    // Initializes a new Orderbook and stores it in the Orderbooks
    fn create_order_book(quote_asset_id: T::AssetId, base_asset_id: T::AssetId, trading_pair_id: &T::Hash) {
        let orderbook = Orderbook::new(base_asset_id, quote_asset_id, trading_pair_id.clone());
        <Orderbooks<T>>::insert(trading_pair_id, orderbook);
        <AsksLevels<T>>::insert(trading_pair_id, Vec::<FixedU128>::new());
        <BidsLevels<T>>::insert(trading_pair_id, Vec::<FixedU128>::new());
    }

    // Creates a TradingPairID from both Asset IDs.
    fn create_trading_pair_id(quote_asset_id: &u32, base_asset_id: &u32) -> T::Hash {
        (quote_asset_id, base_asset_id).using_encoded(<T as frame_system::Trait>::Hashing::hash)
    }

    // Submits an order for execution
    fn execute_order(trader: T::AccountId,
                     order_type: OrderType,
                     trading_pair: T::Hash,
                     price: FixedU128,
                     quantity: FixedU128) -> Result<(), Error<T>> {
        let mut current_order = Order {
            id: T::Hash::default(), // let's do the hashing after the checks.
            trading_pair,
            trader,
            price,
            quantity,
            order_type,
        };

        match Self::basic_order_checks(&current_order) {
            Ok(mut orderbook) => {
                let nonce = Nonce::get(); // To get some kind non user controllable randomness to order id
                current_order.id = (trading_pair, current_order.trader.clone(), price, quantity, current_order.order_type.clone(), nonce)
                    .using_encoded(<T as frame_system::Trait>::Hashing::hash);
                Nonce::put(nonce + 1); // TODO: It might overflow after a long time.

                match current_order.order_type {
                    OrderType::AskMarket if orderbook.best_bid_price != FixedU128::from(0) => {
                        Self::consume_order(&mut current_order, &mut orderbook)?;
                    }

                    OrderType::BidMarket if orderbook.best_ask_price != FixedU128::from(0) => {
                        Self::consume_order(&mut current_order, &mut orderbook)?;
                    }

                    OrderType::AskLimit | OrderType::BidLimit => {
                        // Check if current can consume orders present in the system
                        if (current_order.order_type == OrderType::BidLimit &&
                            current_order.price >= orderbook.best_ask_price &&
                            orderbook.best_ask_price != FixedU128::from(0)) ||
                            (current_order.order_type == OrderType::AskLimit &&
                                current_order.price <= orderbook.best_bid_price &&
                                orderbook.best_bid_price != FixedU128::from(0)) {

                            // current_order can consume i.e. Market Taking order
                            Self::consume_order(&mut current_order, &mut orderbook)?;


                            // If current_order has quantity remaining to fulfil, insert it
                            if current_order.quantity > FixedU128::from(0) {
                                // Insert the remaining order in the order book
                                Self::insert_order(&current_order, &mut orderbook)?;
                            }
                        } else {
                            // Current Order cannot be consumed i.e. Market Making order
                            // Insert the order in the order book
                            Self::insert_order(&current_order, &mut orderbook)?;
                        }
                    }
                    _ => {}
                }
                <Orderbooks<T>>::insert(&current_order.trading_pair, orderbook);
                match current_order.order_type {
                    OrderType::BidLimit | OrderType::AskLimit if current_order.quantity > FixedU128::from(0) => {
                        Self::deposit_event(RawEvent::NewLimitOrder(current_order.id,
                                                                    current_order.trading_pair,
                                                                    current_order.order_type,
                                                                    current_order.price,
                                                                    current_order.quantity,
                                                                    current_order.trader));
                    }
                    OrderType::BidMarket if current_order.price > FixedU128::from(0) => {
                        Self::deposit_event(RawEvent::UnfilledMarketOrder(current_order.id,
                                                                          current_order.trading_pair,
                                                                          current_order.order_type,
                                                                          current_order.price,
                                                                          current_order.quantity,
                                                                          current_order.trader));
                    }
                    OrderType::AskMarket if current_order.quantity > FixedU128::from(0) => {
                        Self::deposit_event(RawEvent::UnfilledMarketOrder(current_order.id,
                                                                          current_order.trading_pair,
                                                                          current_order.order_type,
                                                                          current_order.price,
                                                                          current_order.quantity,
                                                                          current_order.trader));
                    }
                    OrderType::BidLimit | OrderType::AskLimit if current_order.quantity == FixedU128::from(0) => {
                        Self::deposit_event(RawEvent::FulfilledLimitOrder(current_order.id,
                                                                          current_order.trading_pair,
                                                                          current_order.order_type,
                                                                          current_order.price,
                                                                          current_order.quantity,
                                                                          current_order.trader));
                    }
                    OrderType::BidMarket if current_order.price == FixedU128::from(0) => {
                        Self::deposit_event(RawEvent::FilledMarketOrder(current_order.id,
                                                                        current_order.trading_pair,
                                                                        current_order.order_type,
                                                                        current_order.price,
                                                                        current_order.quantity,
                                                                        current_order.trader));
                    }
                    OrderType::AskMarket if current_order.quantity == FixedU128::from(0) => {
                        Self::deposit_event(RawEvent::FilledMarketOrder(current_order.id,
                                                                        current_order.trading_pair,
                                                                        current_order.order_type,
                                                                        current_order.price,
                                                                        current_order.quantity,
                                                                        current_order.trader));
                    }
                    _ => {
                        // This branch will not execute
                    }
                }
                Ok(())
            }
            Err(err_value) => Err(err_value),
        }
    }

    // Inserts the given order into orderbook
    fn insert_order(current_order: &Order<T>, orderbook: &mut Orderbook<T>) -> Result<(), Error<T>> {
        // TODO: bids_levels should be sorted in descending order  FIX-look
        // TODO: asks_levels should be sorted in ascending order FIX-look
        // TODO: The logic given below is assuming that 0th index of bids_levels is highest bid price &
        // TODO: 0th index of asks_levels is lowest ask price.
        match current_order.order_type {
            OrderType::BidLimit => {
                // bids_levels contains the sorted pricelevels
                let mut bids_levels: Vec<FixedU128> = <BidsLevels<T>>::get(&current_order.trading_pair);
                match bids_levels.binary_search(&current_order.price) {
                    Ok(_) => {
                        // current_order.price is already there in the system
                        // so we just need to insert into it's linkedpricelevel FIFO.
                        let mut linked_pricelevel: LinkedPriceLevel<T> = <PriceLevels<T>>::get(&current_order.trading_pair, &current_order.price);
                        linked_pricelevel.orders.push_back(current_order.clone());
                        // Write it back to storage
                        <PriceLevels<T>>::insert(&current_order.trading_pair, &current_order.price, linked_pricelevel)
                        // Access there is not new price level creation, there is won't be any change to orderbook's best prices
                    }
                    Err(index_at_which_we_should_insert) => {
                        bids_levels.insert(index_at_which_we_should_insert, current_order.price);
                        // Here there can be four cases,
                        // 1. when current_order is the last item in the bids_levels
                        // 2. when current_order is the first item in the bids_levels and bids_level was not empty
                        // 3. when current_order is the first item in the bids_levels and bids_level was empty
                        // 4. when current_order is inserted in between two other items in bids_levels
                        if index_at_which_we_should_insert != 0 && index_at_which_we_should_insert != bids_levels.len() - 1 {
                            // Fourth case
                            let mut index_minus1_linkedpricelevel: LinkedPriceLevel<T> = <PriceLevels<T>>::get(&current_order.trading_pair, &bids_levels.get(index_at_which_we_should_insert - 1).ok_or(Error::<T>::NoElementFound.into())?);
                            let mut index_plus1_linkedpricelevel: LinkedPriceLevel<T> = <PriceLevels<T>>::get(&current_order.trading_pair, &bids_levels.get(index_at_which_we_should_insert + 1).ok_or(Error::<T>::NoElementFound.into())?);
                            let mut current_linkedpricelevel: LinkedPriceLevel<T> = LinkedPriceLevel {
                                next: Some(*bids_levels.get(index_at_which_we_should_insert - 1).ok_or(Error::<T>::NoElementFound.into())?),
                                prev: Some(*bids_levels.get(index_at_which_we_should_insert + 1).ok_or(Error::<T>::NoElementFound.into())?),
                                orders: VecDeque::<Order<T>>::new(),
                            };
                            index_minus1_linkedpricelevel.prev = Some(current_order.price);
                            index_plus1_linkedpricelevel.next = Some(current_order.price);
                            current_linkedpricelevel.orders.push_back(current_order.clone());

                            // All the value updates are done. Write it back to storage.
                            <PriceLevels<T>>::insert(&current_order.trading_pair,
                                                     &bids_levels.get(index_at_which_we_should_insert - 1).ok_or(Error::<T>::NoElementFound.into())?,
                                                     index_minus1_linkedpricelevel);
                            <PriceLevels<T>>::insert(&current_order.trading_pair,
                                                     &bids_levels.get(index_at_which_we_should_insert + 1).ok_or(Error::<T>::NoElementFound.into())?,
                                                     index_plus1_linkedpricelevel);
                            <PriceLevels<T>>::insert(&current_order.trading_pair,
                                                     &current_order.price,
                                                     current_linkedpricelevel);
                        }

                        if index_at_which_we_should_insert == 0 && bids_levels.len() > 1 {
                            // Second Case
                            let mut index_plus1_linkedpricelevel: LinkedPriceLevel<T> = <PriceLevels<T>>::get(&current_order.trading_pair, &bids_levels.get(index_at_which_we_should_insert + 1).ok_or(Error::<T>::NoElementFound.into())?);
                            let mut current_linkedpricelevel: LinkedPriceLevel<T> = LinkedPriceLevel {
                                next: None,
                                prev: Some(*bids_levels.get(index_at_which_we_should_insert + 1).ok_or(Error::<T>::NoElementFound.into())?),
                                orders: VecDeque::<Order<T>>::new(),
                            };
                            index_plus1_linkedpricelevel.next = Some(current_order.price);
                            current_linkedpricelevel.orders.push_back(current_order.clone());
                            // All the value updates are done. Write it back to storage.
                            <PriceLevels<T>>::insert(&current_order.trading_pair,
                                                     &bids_levels.get(index_at_which_we_should_insert + 1).ok_or(Error::<T>::NoElementFound.into())?,
                                                     index_plus1_linkedpricelevel);
                            <PriceLevels<T>>::insert(&current_order.trading_pair,
                                                     &current_order.price,
                                                     current_linkedpricelevel);
                        } else if index_at_which_we_should_insert == 0 && bids_levels.len() == 1 {
                            // Third Case
                            let mut current_linkedpricelevel: LinkedPriceLevel<T> = LinkedPriceLevel {
                                next: None,
                                prev: None,
                                orders: VecDeque::<Order<T>>::new(),
                            };
                            current_linkedpricelevel.orders.push_back(current_order.clone());
                            <PriceLevels<T>>::insert(&current_order.trading_pair,
                                                     &current_order.price,
                                                     current_linkedpricelevel);

                            // As current_order has the best_bid price, we store that to best_bid_price
                            orderbook.best_bid_price = current_order.price;
                        }
                        if index_at_which_we_should_insert == bids_levels.len() - 1 && index_at_which_we_should_insert != 0 {
                            // First Case
                            let mut index_minus1_linkedpricelevel: LinkedPriceLevel<T> = <PriceLevels<T>>::get(&current_order.trading_pair, &bids_levels.get(index_at_which_we_should_insert - 1).ok_or(Error::<T>::NoElementFound.into())?);
                            let mut current_linkedpricelevel: LinkedPriceLevel<T> = LinkedPriceLevel {
                                next: Some(*bids_levels.get(index_at_which_we_should_insert - 1).ok_or(Error::<T>::NoElementFound.into())?),
                                prev: None,
                                orders: VecDeque::<Order<T>>::new(),
                            };
                            index_minus1_linkedpricelevel.prev = Some(current_order.price);

                            current_linkedpricelevel.orders.push_back(current_order.clone());
                            // All the value updates are done. Write it back to storage.
                            <PriceLevels<T>>::insert(&current_order.trading_pair,
                                                     &bids_levels.get(index_at_which_we_should_insert - 1).ok_or(Error::<T>::NoElementFound.into())?,
                                                     index_minus1_linkedpricelevel);
                            <PriceLevels<T>>::insert(&current_order.trading_pair,
                                                     &current_order.price,
                                                     current_linkedpricelevel);

                            // As current_order has the best_bid price, we store that to best_bid_price
                            orderbook.best_bid_price = current_order.price;
                        }
                    }
                }
                <BidsLevels<T>>::insert(&current_order.trading_pair, bids_levels);
            }
            OrderType::AskLimit => {
                // asks_levels contains the sorted pricelevels
                let mut asks_levels: Vec<FixedU128> = <AsksLevels<T>>::get(&current_order.trading_pair);
                match asks_levels.binary_search(&current_order.price) {
                    Ok(_) => {
                        // current_order.price is already there in the system
                        // so we just need to insert into it's linkedpricelevel FIFO.
                        let mut linked_pricelevel: LinkedPriceLevel<T> = <PriceLevels<T>>::get(&current_order.trading_pair, &current_order.price);
                        linked_pricelevel.orders.push_back(current_order.clone());
                        // Write it back to storage
                        <PriceLevels<T>>::insert(&current_order.trading_pair, &current_order.price, linked_pricelevel)
                        // Access there is not new price level creation, there is won't be any change to orderbook's best prices
                    }
                    Err(index_at_which_we_should_insert) => {
                        asks_levels.insert(index_at_which_we_should_insert, current_order.price);
                        // Here there can be four cases,
                        // 1. when current_order is the last item in the asks_levels
                        // 2. when current_order is the first item in the asks_levels
                        // 3. when current_order is the first item and the only item in asks_levels
                        // 4. when current_order is inserted in between two other items in asks_levels
                        if index_at_which_we_should_insert != 0 && index_at_which_we_should_insert != asks_levels.len() - 1 {
                            // Fourth case
                            let mut index_minus1_linkedpricelevel: LinkedPriceLevel<T> = <PriceLevels<T>>::get(&current_order.trading_pair, &asks_levels.get(index_at_which_we_should_insert - 1).ok_or(Error::<T>::NoElementFound.into())?);
                            let mut index_plus1_linkedpricelevel: LinkedPriceLevel<T> = <PriceLevels<T>>::get(&current_order.trading_pair, &asks_levels.get(index_at_which_we_should_insert + 1).ok_or(Error::<T>::NoElementFound.into())?);
                            let mut current_linkedpricelevel: LinkedPriceLevel<T> = LinkedPriceLevel {
                                next: Some(*asks_levels.get(index_at_which_we_should_insert + 1).ok_or(Error::<T>::NoElementFound.into())?),
                                prev: Some(*asks_levels.get(index_at_which_we_should_insert - 1).ok_or(Error::<T>::NoElementFound.into())?),
                                orders: VecDeque::<Order<T>>::new(),
                            };
                            index_minus1_linkedpricelevel.next = Some(current_order.price);
                            index_plus1_linkedpricelevel.prev = Some(current_order.price);
                            current_linkedpricelevel.orders.push_back(current_order.clone());

                            // All the value updates are done. Write it back to storage.
                            <PriceLevels<T>>::insert(&current_order.trading_pair,
                                                     &asks_levels.get(index_at_which_we_should_insert - 1).ok_or(Error::<T>::NoElementFound.into())?,
                                                     index_minus1_linkedpricelevel);
                            <PriceLevels<T>>::insert(&current_order.trading_pair,
                                                     &asks_levels.get(index_at_which_we_should_insert + 1).ok_or(Error::<T>::NoElementFound.into())?,
                                                     index_plus1_linkedpricelevel);
                            <PriceLevels<T>>::insert(&current_order.trading_pair,
                                                     &current_order.price,
                                                     current_linkedpricelevel);
                        }

                        if index_at_which_we_should_insert == 0 && asks_levels.len() > 1 {
                            // Second Case
                            let mut index_plus1_linkedpricelevel: LinkedPriceLevel<T> = <PriceLevels<T>>::get(&current_order.trading_pair, &asks_levels.get(index_at_which_we_should_insert + 1).ok_or(Error::<T>::NoElementFound.into())?);
                            let mut current_linkedpricelevel: LinkedPriceLevel<T> = LinkedPriceLevel {
                                next: Some(*asks_levels.get(index_at_which_we_should_insert + 1).ok_or(Error::<T>::NoElementFound.into())?),
                                prev: None,
                                orders: VecDeque::<Order<T>>::new(),
                            };
                            index_plus1_linkedpricelevel.prev = Some(current_order.price);

                            current_linkedpricelevel.orders.push_back(current_order.clone());
                            // All the value updates are done. Write it back to storage.
                            <PriceLevels<T>>::insert(&current_order.trading_pair,
                                                     &asks_levels.get(index_at_which_we_should_insert + 1).ok_or(Error::<T>::NoElementFound.into())?,
                                                     index_plus1_linkedpricelevel);
                            <PriceLevels<T>>::insert(&current_order.trading_pair,
                                                     &current_order.price,
                                                     current_linkedpricelevel);

                            // As current_order has the best_bid price, we store that to best_bid_price
                            orderbook.best_ask_price = current_order.price;
                        }
                        if index_at_which_we_should_insert == 0 && asks_levels.len() == 1 {
                            // Third Case
                            let mut current_linkedpricelevel: LinkedPriceLevel<T> = LinkedPriceLevel {
                                next: None,
                                prev: None,
                                orders: VecDeque::<Order<T>>::new(),
                            };

                            current_linkedpricelevel.orders.push_back(current_order.clone());

                            // Write it back to storage
                            <PriceLevels<T>>::insert(&current_order.trading_pair,
                                                     &current_order.price,
                                                     current_linkedpricelevel);
                            // As current_order has the best_bid price, we store that to best_bid_price
                            orderbook.best_ask_price = current_order.price;
                        }
                        if index_at_which_we_should_insert == asks_levels.len() - 1 && index_at_which_we_should_insert != 0 {
                            // First Case
                            let mut index_minus1_linkedpricelevel: LinkedPriceLevel<T> = <PriceLevels<T>>::get(&current_order.trading_pair, &asks_levels.get(index_at_which_we_should_insert - 1).ok_or(Error::<T>::NoElementFound.into())?);
                            let mut current_linkedpricelevel: LinkedPriceLevel<T> = LinkedPriceLevel {
                                next: None,
                                prev: Some(*asks_levels.get(index_at_which_we_should_insert - 1).ok_or(Error::<T>::NoElementFound.into())?),
                                orders: VecDeque::<Order<T>>::new(),
                            };
                            index_minus1_linkedpricelevel.next = Some(current_order.price);
                            current_linkedpricelevel.orders.push_back(current_order.clone());
                            // All the value updates are done. Write it back to storage.
                            <PriceLevels<T>>::insert(&current_order.trading_pair,
                                                     &asks_levels.get(index_at_which_we_should_insert - 1).ok_or(Error::<T>::NoElementFound.into())?,
                                                     index_minus1_linkedpricelevel);
                            <PriceLevels<T>>::insert(&current_order.trading_pair,
                                                     &current_order.price,
                                                     current_linkedpricelevel);
                        }
                    }
                }
                <AsksLevels<T>>::insert(&current_order.trading_pair, asks_levels);
            }
            _ => {
                // It will never execute
            }
        }
        Ok(())
    }

    fn consume_order(current_order: &mut Order<T>, orderbook: &mut Orderbook<T>) -> Result<(), Error<T>> {
        let mut market_data: MarketData;
        // TODO: Not sure what will be the return value of get() given below for keys that doesn't exist.
        // TODO: Currently I am assuming it will be None and not Some("default value of MarketData")
        // if <MarketInfo<T>>::contains_key(&current_order.trading_pair, <frame_system::Module<T>>::block_number()) {
        //     market_data = <MarketInfo<T>>::get(&current_order.trading_pair, <frame_system::Module<T>>::block_number())
        // } else {
        //     market_data = MarketData{
        //         low: FixedU128::from(0),
        //         high: FixedU128::from(0),
        //         volume: FixedU128::from(0)
        //     }
        // }
        let current_block_number: T::BlockNumber = <frame_system::Module<T>>::block_number();
        match <MarketInfo<T>>::get(current_order.trading_pair, current_block_number) {
            Some(_market_data) => {
                market_data = _market_data;
            }
            None => {
                market_data = MarketData {
                    low: FixedU128::from(0),
                    high: FixedU128::from(0),
                    volume: FixedU128::from(0),
                }
            }
        }
        match current_order.order_type {
            OrderType::BidLimit => {
                // The incoming order is BidLimit and it will be able to match best_ask_price
                // Hence, we load the corresponding LinkedPriceLevel of best_ask_price from storage and execute

                // we want to match the orders until the current_price is less than the ask_price
                // or the current_order is fulfilled completely
                let mut linkedpricelevel: LinkedPriceLevel<T> = <PriceLevels<T>>::take(&current_order.trading_pair, &orderbook.best_ask_price);
                while current_order.quantity > FixedU128::from(0) {
                    if let Some(mut counter_order) = linkedpricelevel.orders.pop_front() {
                        Self::do_asset_exchange(current_order,
                                                &mut counter_order,
                                                &mut market_data,
                                                orderbook.base_asset_id,
                                                orderbook.quote_asset_id)?;

                        if counter_order.quantity > FixedU128::from(0) {
                            // Emit events
                            Self::emit_partial_fill(&counter_order, current_order.quantity);
                            // counter_order was not completely used so we store it back in the FIFO
                            linkedpricelevel.orders.push_front(counter_order);
                        } else {
                            // Emit events
                            Self::emit_complete_fill(&counter_order, current_order.quantity);
                        }
                    } else {
                        // TODO: Check: Not sure if "no orders remaining" is the only case that will trigger this branch
                        // As no more orders are available in the linkedpricelevel.
                        // we check if we can match with the next available level

                        // As we consumed the linkedpricelevel completely remove that from asks_levels
                        let mut asks_levels: Vec<FixedU128> = <AsksLevels<T>>::get(&current_order.trading_pair);
                        // asks_levels is already sorted and the best_ask_price should be the first item
                        // so we don't need to sort it after we remove and simply remove the 0th index.
                        // NOTE: In asks_levels & bids_levels all items are unique.
                        asks_levels.remove(0);
                        // Write it back to storage.
                        <AsksLevels<T>>::insert(&current_order.trading_pair, asks_levels);

                        if linkedpricelevel.next.is_none() {
                            // No more price levels available
                            break;
                        }

                        if current_order.price >= linkedpricelevel.next.ok_or(Error::<T>::NoElementFound.into())? {
                            // In this case current_order.quantity is remaining and
                            // it can match with next price level in orderbook.

                            // Last best_ask_price is consumed and doesn't exist anymore hence
                            // we set new best_ask_price in orderbook.
                            orderbook.best_ask_price = linkedpricelevel.next.ok_or(Error::<T>::NoElementFound.into())?;
                            linkedpricelevel = <PriceLevels<T>>::take(&current_order.trading_pair, linkedpricelevel.next.ok_or(Error::<T>::NoElementFound.into())?);
                        } else {
                            // In this case, the current_order cannot match with the best_ask_price available
                            // so let's break the while loop and return the current_order and orderbook
                            break;
                        }
                    }
                }

                if !linkedpricelevel.orders.is_empty() {
                    // Save Pricelevel back to storage
                    <PriceLevels<T>>::insert(&current_order.trading_pair, &orderbook.best_ask_price, linkedpricelevel);
                } else {
                    // As we consumed the linkedpricelevel completely remove that from asks_levels
                    let mut asks_levels: Vec<FixedU128> = <AsksLevels<T>>::get(&current_order.trading_pair);
                    // asks_levels is already sorted and the best_ask_price should be the first item
                    // so we don't need to sort it after we remove and simply remove it
                    asks_levels.remove(0);
                    // Update the Orderbook
                    if asks_levels.len() == 0 {
                        orderbook.best_ask_price = FixedU128::from(0);
                    } else {
                        match asks_levels.get(0) {
                            Some(best_price) => {
                                orderbook.best_ask_price = *best_price;
                            }
                            None => {
                                orderbook.best_ask_price = FixedU128::from(0);
                            }
                        }
                    }
                    // Write it back to storage.
                    <AsksLevels<T>>::insert(&current_order.trading_pair, asks_levels);
                }
            }

            OrderType::BidMarket => {
                // Incoming order is a Market buy order so it, trader whats to buy the quote_asset for
                // current_order.price at Market price.


                // We load the best_ask_price level and start to fill the order
                let mut linkedpricelevel: LinkedPriceLevel<T> = <PriceLevels<T>>::take(&current_order.trading_pair, &orderbook.best_ask_price);
                // We iterate until current_order is fulfilled or exhausts the Ask orders in the system.
                while current_order.price > FixedU128::from(0) {
                    if let Some(mut counter_order) = linkedpricelevel.orders.pop_front() {
                        Self::do_asset_exchange_market(current_order,
                                                       &mut counter_order,
                                                       &mut market_data,
                                                       orderbook.base_asset_id,
                                                       orderbook.quote_asset_id)?;


                        if counter_order.quantity > FixedU128::from(0) {
                            // Emit events
                            Self::emit_partial_fill(&counter_order, current_order.quantity);
                            // counter_order was not completely used so we store it back in the FIFO
                            linkedpricelevel.orders.push_front(counter_order);
                        } else {
                            // Emit events
                            Self::emit_complete_fill(&counter_order, current_order.quantity);
                        }
                    } else {
                        // TODO: Check: Not sure if "no orders remaining" is the only case that will trigger this branch
                        // As no more orders are available in the linkedpricelevel.
                        // we check if we can match with the next available level

                        // As we consumed the linkedpricelevel completely remove that from asks_levels
                        let mut asks_levels: Vec<FixedU128> = <AsksLevels<T>>::get(&current_order.trading_pair);
                        // asks_levels is already sorted and the best_ask_price should be the first item
                        // so we don't need to sort it after we remove and simply remove it
                        // NOTE: In asks_levels & bids_levels all items are unique.
                        asks_levels.remove(0);
                        // Write it back to storage.
                        <AsksLevels<T>>::insert(&current_order.trading_pair, asks_levels);

                        if linkedpricelevel.next.is_none() {
                            // No more price levels available
                            break;
                        }

                        orderbook.best_ask_price = linkedpricelevel.next.ok_or(Error::<T>::NoElementFound.into())?;
                        linkedpricelevel = <PriceLevels<T>>::take(&current_order.trading_pair, linkedpricelevel.next.ok_or(Error::<T>::NoElementFound.into())?);
                    }
                }

                if !linkedpricelevel.orders.is_empty() {
                    // Save Pricelevel back to storage
                    <PriceLevels<T>>::insert(&current_order.trading_pair, &orderbook.best_ask_price, linkedpricelevel);
                } else {
                    // As we consumed the linkedpricelevel completely remove that from asks_levels
                    let mut asks_levels: Vec<FixedU128> = <AsksLevels<T>>::get(&current_order.trading_pair);
                    // asks_levels is already sorted and the best_ask_price should be the first item
                    // so we don't need to sort it after we remove and simply remove it
                    asks_levels.remove(0);
                    // Update the Orderbook
                    if asks_levels.len() == 0 {
                        orderbook.best_ask_price = FixedU128::from(0);
                    } else {
                        match asks_levels.get(0) {
                            Some(best_price) => {
                                orderbook.best_ask_price = *best_price;
                            }
                            None => {
                                orderbook.best_ask_price = FixedU128::from(0);
                            }
                        }
                    }
                    // Write it back to storage.
                    <AsksLevels<T>>::insert(&current_order.trading_pair, asks_levels);
                }
            }

            OrderType::AskLimit => {
                // The incoming order is AskLimit and it will be able to match best_bid_price
                // Hence, we load the corresponding LinkedPriceLevel of best_bid_price from storage and execute

                // we want to match the orders until the current_price is greater than the bid_price
                // or the current_order is fulfilled completely
                let mut linkedpricelevel: LinkedPriceLevel<T> = <PriceLevels<T>>::take(&current_order.trading_pair, &orderbook.best_bid_price);
                while current_order.quantity > FixedU128::from(0) {
                    if let Some(mut counter_order) = linkedpricelevel.orders.pop_front() {
                        Self::do_asset_exchange(current_order,
                                                &mut counter_order,
                                                &mut market_data,
                                                orderbook.base_asset_id,
                                                orderbook.quote_asset_id)?;

                        if counter_order.quantity > FixedU128::from(0) {
                            // Emit events
                            Self::emit_partial_fill(&counter_order, current_order.quantity);
                            // counter_order was not completely used so we store it back in the FIFO
                            linkedpricelevel.orders.push_front(counter_order);
                        } else {
                            // Emit events
                            Self::emit_complete_fill(&counter_order, current_order.quantity);
                        }
                    } else {
                        // TODO: Check: Not sure if "no orders remaining" is the only case that will trigger this branch
                        // As no more orders are available in the linkedpricelevel.
                        // we check if we can match with the next available level

                        // As we consumed the linkedpricelevel completely remove that from bids_levels
                        let mut bids_levels: Vec<FixedU128> = <BidsLevels<T>>::get(&current_order.trading_pair);
                        // bids_levels is already sorted and the best_bid_price should be the first item
                        // so we don't need to sort it after we remove and simply remove it
                        // NOTE: In asks_levels & bids_levels all items are unique.
                        bids_levels.remove(bids_levels.len() - 1);
                        // Write it back to storage.
                        <BidsLevels<T>>::insert(&current_order.trading_pair, bids_levels);

                        if linkedpricelevel.prev.is_none() {
                            // No more price levels available
                            break;
                        }
                        if current_order.price <= linkedpricelevel.prev.ok_or(Error::<T>::NoElementFound.into())? {
                            // In this case current_order.quantity is remaining and
                            // it can match with next price level in orderbook.

                            // Last best_bid_price is consumed and doesn't exist anymore hence
                            // we set new best_bid_price in orderbook.
                            orderbook.best_bid_price = linkedpricelevel.prev.ok_or(Error::<T>::NoElementFound.into())?;
                            linkedpricelevel = <PriceLevels<T>>::take(&current_order.trading_pair, linkedpricelevel.prev.ok_or(Error::<T>::NoElementFound.into())?);
                        } else {
                            // In this case, the current_order cannot match with the best_bid_price available
                            // so let's break the while loop and return the current_order and orderbook
                            break;
                        }
                    }
                }

                if !linkedpricelevel.orders.is_empty() {
                    // Save Pricelevel back to storage
                    <PriceLevels<T>>::insert(&current_order.trading_pair, &orderbook.best_bid_price, linkedpricelevel);
                } else {
                    // As we consumed the linkedpricelevel completely remove that from bids_levels
                    let mut bids_levels: Vec<FixedU128> = <BidsLevels<T>>::get(&current_order.trading_pair);
                    // bids_levels is already sorted and the best_bid_price should be the last item
                    // so we don't need to sort it after we remove and simply remove it
                    if bids_levels.len() != 0 {
                        bids_levels.remove(bids_levels.len() - 1);
                    }
                    // Update the Orderbook
                    if bids_levels.len() == 0 {
                        orderbook.best_bid_price = FixedU128::from(0);
                    } else {
                        match bids_levels.get(bids_levels.len() - 1) {
                            Some(best_price) => {
                                orderbook.best_bid_price = *best_price;
                            }
                            None => {
                                orderbook.best_bid_price = FixedU128::from(0);
                            }
                        }
                    }
                    // Write it back to storage.
                    <BidsLevels<T>>::insert(&current_order.trading_pair, bids_levels);
                }
            }

            OrderType::AskMarket => {
                // Incoming Order is a Market Sell, so trader wants to sell current_order.quantity
                // at best possible price.
                // We load the best_bid_price level and start to fill the order
                let mut linkedpricelevel: LinkedPriceLevel<T> = <PriceLevels<T>>::take(&current_order.trading_pair, &orderbook.best_bid_price);
                while current_order.quantity > FixedU128::from(0) {
                    if let Some(mut counter_order) = linkedpricelevel.orders.pop_front() {
                        Self::do_asset_exchange_market(current_order,
                                                       &mut counter_order,
                                                       &mut market_data,
                                                       orderbook.base_asset_id,
                                                       orderbook.quote_asset_id)?;

                        if counter_order.quantity > FixedU128::from(0) {
                            // Emit events
                            Self::emit_partial_fill(&counter_order, current_order.quantity);
                            // counter_order was not completely used so we store it back in the FIFO
                            linkedpricelevel.orders.push_front(counter_order);
                        } else {
                            // Emit events
                            Self::emit_complete_fill(&counter_order, current_order.quantity);
                        }
                    } else {
                        // As no more orders are available in the linkedpricelevel.
                        // we check if we can match with the next available level

                        // As we consumed the linkedpricelevel completely remove that from bids_levels
                        let mut bids_levels: Vec<FixedU128> = <BidsLevels<T>>::get(&current_order.trading_pair);
                        // bids_levels is already sorted and the best_bid_price should be the first item
                        // so we don't need to sort it after we remove and simply remove it
                        // NOTE: In asks_levels & bids_levels all items are unique.
                        bids_levels.remove(bids_levels.len() - 1);
                        // Write it back to storage.
                        <BidsLevels<T>>::insert(&current_order.trading_pair, bids_levels);

                        if linkedpricelevel.prev.is_none() {
                            // No more price levels available
                            break;
                        }

                        orderbook.best_bid_price = linkedpricelevel.prev.ok_or(Error::<T>::NoElementFound.into())?;
                        linkedpricelevel = <PriceLevels<T>>::take(&current_order.trading_pair, linkedpricelevel.prev.ok_or(Error::<T>::NoElementFound.into())?);
                    }
                }

                if !linkedpricelevel.orders.is_empty() {
                    // Save Pricelevel back to storage
                    <PriceLevels<T>>::insert(&current_order.trading_pair, &orderbook.best_bid_price, linkedpricelevel);
                } else {
                    // As we consumed the linkedpricelevel completely remove that from bids_levels
                    let mut bids_levels: Vec<FixedU128> = <BidsLevels<T>>::get(&current_order.trading_pair);
                    // bids_levels is already sorted and the best_bid_price should be the first item
                    // so we don't need to sort it after we remove and simply remove it
                    if bids_levels.len() != 0 {
                        bids_levels.remove(bids_levels.len() - 1);
                    }
                    // Update the Orderbook
                    if bids_levels.len() == 0 {
                        orderbook.best_bid_price = FixedU128::from(0);
                    } else {
                        match bids_levels.get(bids_levels.len() - 1) {
                            Some(best_price) => {
                                orderbook.best_bid_price = *best_price;
                            }
                            None => {
                                orderbook.best_bid_price = FixedU128::from(0);
                            }
                        }
                    }
                    // Write it back to storage.
                    <BidsLevels<T>>::insert(&current_order.trading_pair, bids_levels);
                }
            }
        }
        // Write the market data back to storage
        <MarketInfo<T>>::insert(&current_order.trading_pair, current_block_number, market_data);
        Ok(())
    }

    fn do_asset_exchange_market(current_order: &mut Order<T>, counter_order: &mut Order<T>, market_data: &mut MarketData, base_assetid: T::AssetId, quote_assetid: T::AssetId) -> Result<(), Error<T>> {
        if market_data.low == FixedU128::from(0) {
            market_data.low = counter_order.price
        }
        if market_data.high == FixedU128::from(0) {
            market_data.high = counter_order.price
        }
        if market_data.high < counter_order.price {
            market_data.high = counter_order.price
        }
        if market_data.low > counter_order.price {
            market_data.low = counter_order.price
        }
        match current_order.order_type {
            OrderType::BidMarket => {
                let current_order_quantity = current_order.price.checked_div(&counter_order.price).ok_or(Error::<T>::DivUnderflowOrOverflow.into())?;
                // 100/5 = 20.
                if current_order_quantity <= counter_order.quantity {
                    // We have enough quantity in the counter_order to fulfill current_order completely
                    // Transfer the base asset
                    Self::transfer_asset_market(base_assetid, current_order.price, &current_order.trader, &counter_order.trader)?;
                    // Transfer the quote asset
                    Self::transfer_asset(quote_assetid, current_order_quantity, &counter_order.trader, &current_order.trader)?;
                    // Add the volume executed
                    market_data.volume = market_data.volume.checked_add(&current_order.price).ok_or(Error::<T>::AddUnderflowOrOverflow.into())?;
                    //Set current_order quantity to 0 and counter_order is reduced by fulfilled amount
                    counter_order.quantity = counter_order.quantity.checked_sub(&current_order_quantity).ok_or(Error::<T>::SubUnderflowOrOverflow.into())?;
                    current_order.price = FixedU128::from(0);
                } else {
                    let trade_amount = counter_order.price.checked_mul(&counter_order.quantity).ok_or(Error::<T>::MulUnderflowOrOverflow.into())?;
                    // Transfer the base asset
                    Self::transfer_asset_market(base_assetid, trade_amount, &current_order.trader, &counter_order.trader)?;
                    // Transfer the quote asset
                    Self::transfer_asset(quote_assetid, counter_order.quantity, &counter_order.trader, &current_order.trader)?;
                    // Add the volume executed
                    market_data.volume = market_data.volume.checked_add(&trade_amount).ok_or(Error::<T>::AddUnderflowOrOverflow.into())?;
                    // counter_order is set to 0 and current_order.price is reduced by fulfilled amount
                    counter_order.quantity = FixedU128::from(0);
                    current_order.price = current_order.price.checked_sub(&trade_amount).ok_or(Error::<T>::SubUnderflowOrOverflow.into())?;
                }
            }
            OrderType::AskMarket => {
                if current_order.quantity <= counter_order.quantity {
                    // We have enough quantity in the counter_order to fulfill current_order completely
                    let trade_amount = counter_order.price.checked_mul(&current_order.quantity).ok_or(Error::<T>::MulUnderflowOrOverflow.into())?;
                    // Transfer the base asset
                    Self::transfer_asset(base_assetid, trade_amount, &counter_order.trader, &current_order.trader)?;
                    // Transfer the quote asset
                    Self::transfer_asset_market(quote_assetid, current_order.quantity, &current_order.trader, &counter_order.trader)?;
                    // Add the volume executed
                    market_data.volume = market_data.volume.checked_add(&trade_amount).ok_or(Error::<T>::AddUnderflowOrOverflow.into())?;
                    // current_order is set to 0 and counter_order is reduced by fulfilled amount
                    counter_order.quantity = counter_order.quantity.checked_sub(&current_order.quantity).ok_or(Error::<T>::SubUnderflowOrOverflow.into())?;
                    current_order.quantity = FixedU128::from(0);
                } else {
                    // We have enough quantity in the counter_order to fulfill current_order completely
                    let trade_amount = counter_order.price.checked_mul(&counter_order.quantity).ok_or(Error::<T>::MulUnderflowOrOverflow.into())?;
                    // Transfer the base asset
                    Self::transfer_asset(base_assetid, trade_amount, &counter_order.trader, &current_order.trader)?;
                    // Transfer the quote asset
                    Self::transfer_asset_market(quote_assetid, counter_order.quantity, &current_order.trader, &counter_order.trader)?;
                    // Add the volume executed
                    market_data.volume = market_data.volume.checked_add(&trade_amount).ok_or(Error::<T>::AddUnderflowOrOverflow.into())?;
                    // counter_order is set to 0 and current_order is reduced by fulfilled amount
                    current_order.quantity = current_order.quantity.checked_sub(&counter_order.quantity).ok_or(Error::<T>::SubUnderflowOrOverflow.into())?;
                    counter_order.quantity = FixedU128::from(0);
                }
            }
            _ => {
                // It won't execute.
            }
        }
        Ok(())
    }

    // It checks the if the counter_order.quantity has enough to fulfill current_order then exchanges
    // it after collecting fees and else if counter_order.quantity is less than current_order.quantity
    // then exchange counter_order.quantity completely and return.
    // current_order.quantity is modified to new value.
    fn do_asset_exchange(current_order: &mut Order<T>, counter_order: &mut Order<T>, market_data: &mut MarketData, base_assetid: T::AssetId, quote_assetid: T::AssetId) -> Result<(), Error<T>> {
        if market_data.low == FixedU128::from(0) {
            market_data.low = counter_order.price
        }
        if market_data.high == FixedU128::from(0) {
            market_data.high = counter_order.price
        }
        if market_data.high < counter_order.price {
            market_data.high = counter_order.price
        }
        if market_data.low > counter_order.price {
            market_data.low = counter_order.price
        }
        match current_order.order_type {
            OrderType::BidLimit => {
                // BTC/USDT - quote/base
                // The current order is trying to buy the quote_asset
                if current_order.quantity <= counter_order.quantity {
                    // We have enough quantity in the counter_order to fulfill current_order completely
                    // Calculate the total cost in base asset for buying required amount
                    let trade_amount = current_order.price.checked_mul(&current_order.quantity).ok_or(<Error<T>>::MulUnderflowOrOverflow.into())?;
                    // Transfer the base asset
                    // AssetId, amount to send, from, to
                    Self::transfer_asset(base_assetid, trade_amount, &current_order.trader, &counter_order.trader)?;
                    // Transfer the quote asset
                    Self::transfer_asset(quote_assetid, current_order.quantity, &counter_order.trader, &current_order.trader)?;
                    // Add the executed volume
                    market_data.volume = market_data.volume.checked_add(&trade_amount).ok_or(<Error<T>>::AddUnderflowOrOverflow.into())?;
                    //Set Current order quantity to 0 and counter_order is subtracted.
                    counter_order.quantity = counter_order.quantity.checked_sub(&current_order.quantity).ok_or(<Error<T>>::SubUnderflowOrOverflow.into())?;
                    current_order.quantity = FixedU128::from(0);
                } else {
                    // current_order is partially filled and counter_order is completely filled.
                    // Calculate the total cost in base asset for buying required amount
                    let trade_amount = current_order.price.checked_mul(&counter_order.quantity).ok_or(<Error<T>>::MulUnderflowOrOverflow.into())?;
                    // Transfer the base asset
                    // AssetId, amount to send, from, to
                    Self::transfer_asset(base_assetid, trade_amount, &current_order.trader, &counter_order.trader)?;
                    // Transfer the quote asset from counter_order to current_order's trader.
                    Self::transfer_asset(quote_assetid, counter_order.quantity, &counter_order.trader, &current_order.trader)?;
                    // Add the volume executed
                    market_data.volume = market_data.volume.checked_add(&trade_amount).ok_or(<Error<T>>::MulUnderflowOrOverflow.into())?;
                    //Set counter_order quantity to 0 and current_order is subtracted.
                    current_order.quantity = current_order.quantity.checked_sub(&counter_order.quantity).ok_or(<Error<T>>::SubUnderflowOrOverflow.into())?;
                    counter_order.quantity = FixedU128::from(0);
                }
            }
            OrderType::AskLimit => {
                // The current order is trying to sell the quote_asset
                if current_order.quantity <= counter_order.quantity {
                    // We have enough quantity in the counter_order to fulfill current_order completely
                    // Calculate the total cost in base asset for selling the required amount
                    let trade_amount = counter_order.price.checked_mul(&current_order.quantity).ok_or(<Error<T>>::MulUnderflowOrOverflow.into())?;
                    // Transfer the base asset
                    // AssetId, amount to send, from, to
                    Self::transfer_asset(base_assetid, trade_amount, &counter_order.trader, &current_order.trader)?;
                    // Transfer the quote asset
                    Self::transfer_asset(quote_assetid, current_order.quantity, &current_order.trader, &counter_order.trader)?;
                    // Add the volume executed
                    market_data.volume = market_data.volume.checked_add(&trade_amount).ok_or(<Error<T>>::AddUnderflowOrOverflow.into())?;
                    //Set Current order quantity to 0 and counter_order is subtracted.
                    counter_order.quantity = counter_order.quantity.checked_sub(&current_order.quantity).ok_or(<Error<T>>::SubUnderflowOrOverflow.into())?;
                    current_order.quantity = FixedU128::from(0);
                } else {
                    // current_order is partially filled and counter_order is completely filled.
                    // Calculate the total cost in base asset for selling the required amount
                    let trade_amount = counter_order.price.checked_mul(&counter_order.quantity).ok_or(<Error<T>>::MulUnderflowOrOverflow.into())?;
                    // Transfer the base asset
                    // AssetId, amount to send, from, to
                    Self::transfer_asset(base_assetid, trade_amount, &counter_order.trader, &current_order.trader)?;
                    // Transfer the quote asset from counter_order to current_order's trader.
                    Self::transfer_asset(quote_assetid, counter_order.quantity, &current_order.trader, &counter_order.trader)?;
                    // Add the volume executed
                    market_data.volume = market_data.volume.checked_add(&trade_amount).ok_or(<Error<T>>::AddUnderflowOrOverflow.into())?;
                    //Set counter_order quantity to 0 and current_order is subtracted.
                    current_order.quantity = current_order.quantity.checked_sub(&counter_order.quantity).ok_or(<Error<T>>::SubUnderflowOrOverflow.into())?;
                    counter_order.quantity = FixedU128::from(0);
                }
            }

            _ => {}
        }
        Ok(())
    }

    // Transfers the balance of traders
    fn transfer_asset(asset_id: T::AssetId, amount: FixedU128, from: &T::AccountId, to: &T::AccountId) -> Result<(), Error<T>> {
        let amount_balance = Self::convert_fixed_u128_to_balance(amount).ok_or(<Error<T>>::SubUnderflowOrOverflow.into())?;
        // Initially the balance was reserved so now it is unreserved and then transfer is made
        pallet_generic_asset::Module::<T>::unreserve(&asset_id, from, amount_balance);
        match pallet_generic_asset::Module::<T>::make_transfer(&asset_id, from, to,
                                                               amount_balance) {
            Ok(_) => Ok(()),
            _ => Err(<Error<T>>::ErrorWhileTransferingAsset.into()),
        }
    }

    // Transfers the balance of traders
    fn transfer_asset_market(asset_id: T::AssetId, amount: FixedU128, from: &T::AccountId, to: &T::AccountId) -> Result<(), Error<T>> {
        let amount_balance = Self::convert_fixed_u128_to_balance(amount).ok_or(<Error<T>>::SubUnderflowOrOverflow.into())?;
        match pallet_generic_asset::Module::<T>::make_transfer(&asset_id, from, to,
                                                               amount_balance) {
            Ok(_) => Ok(()),
            _ => Err(<Error<T>>::ErrorWhileTransferingAsset.into()),
        }
    }

    // Checks all the basic checks
    fn basic_order_checks(order: &Order<T>) -> Result<Orderbook<T>, Error<T>> {
        match order.order_type {
            OrderType::BidLimit | OrderType::AskLimit if order.price <= FixedU128::from(0) || order.quantity <= FixedU128::from(0) => Err(<Error<T>>::InvalidPriceOrQuantityLimit.into()),
            OrderType::BidMarket if order.price <= FixedU128::from(0) => Err(<Error<T>>::InvalidBidMarketPrice.into()),
            OrderType::BidMarket | OrderType::BidLimit => Self::check_order(order),
            OrderType::AskMarket if order.quantity <= FixedU128::from(0) => Err(<Error<T>>::InvalidAskMarketQuantity.into()),
            OrderType::AskMarket | OrderType::AskLimit => Self::check_order(order),
        }
    }
    fn check_order(order: &Order<T>) -> Result<Orderbook<T>, Error<T>> {
        let orderbook: Orderbook<T> = <Orderbooks<T>>::get(&order.trading_pair);
        let balance: <T>::Balance = match order.order_type {
            OrderType::BidLimit | OrderType::BidMarket => pallet_generic_asset::Module::<T>::free_balance(&orderbook.base_asset_id, &order.trader),
            OrderType::AskMarket | OrderType::AskLimit => pallet_generic_asset::Module::<T>::free_balance(&orderbook.quote_asset_id, &order.trader),
        };

        match Self::convert_balance_to_fixed_u128(balance) {
            Some(converted_balance) if order.order_type == OrderType::BidLimit => Self::compare_balance(converted_balance, order, orderbook),
            Some(converted_balance) if order.order_type == OrderType::BidMarket && converted_balance < order.price => Err(<Error<T>>::InsufficientAssetBalance.into()),
            Some(converted_balance) if (order.order_type == OrderType::AskLimit || order.order_type == OrderType::AskMarket) && converted_balance < order.quantity => Err(<Error<T>>::InsufficientAssetBalance.into()),
            Some(_) if order.order_type == OrderType::AskLimit => Self::reserve_user_balance(orderbook, order, order.quantity),
            Some(_) if order.order_type == OrderType::AskMarket => Ok(orderbook),
            Some(_) if order.order_type == OrderType::BidMarket => Ok(orderbook),
            _ => Err(<Error<T>>::InternalErrorU128Balance.into()),
        }
    }

    fn compare_balance(converted_balance: FixedU128, order: &Order<T>, orderbook: Orderbook<T>) -> Result<Orderbook<T>, Error<T>> {
        match order.price.checked_mul(&order.quantity) {
            Some(trade_amount) if converted_balance < trade_amount => Err(<Error<T>>::InsufficientAssetBalance.into()),
            Some(trade_amount) if converted_balance >= trade_amount => Self::reserve_user_balance(orderbook, order, trade_amount),
            _ => Err(<Error<T>>::InternalErrorU128Balance.into()),
        }
    }

    fn reserve_user_balance(orderbook: Orderbook<T>, order: &Order<T>, amount: FixedU128) -> Result<Orderbook<T>, Error<T>> {
        // TODO: Based on BidLimit or AskLimit we need to change between orderbook.base_asset_id & orderbook.quote_asset_id respectively
        let asset_id = if order.order_type == OrderType::AskLimit { &orderbook.quote_asset_id } else { &orderbook.base_asset_id };

        match Self::convert_fixed_u128_to_balance(amount) {
            Some(balance) => {
                match pallet_generic_asset::Module::<T>::reserve(
                    asset_id, &order.trader,
                    balance) {
                    Ok(_) => Ok(orderbook),
                    _ => Err(<Error<T>>::ReserveAmountFailed.into()),
                }
            }

            None => Err(<Error<T>>::InternalErrorU128Balance.into()),
        }
    }

    // Converts Balance to FixedU128 representation
    pub fn convert_balance_to_fixed_u128(x: T::Balance) -> Option<FixedU128> {
        if let Some(y) = TryInto::<u128>::try_into(x).ok() {
            FixedU128::from(y).checked_div(&FixedU128::from(1_000_000_000_000))
        } else {
            None
        }
    }

    // Converts FixedU128 to Balance representation
    pub fn convert_fixed_u128_to_balance(x: FixedU128) -> Option<T::Balance> {
        if let Some(balance_in_fixed_u128) = x.checked_div(&FixedU128::from(1000000)) {
            let balance_in_u128 = balance_in_fixed_u128.into_inner();
            Some(UniqueSaturatedFrom::<u128>::unique_saturated_from(balance_in_u128))
        } else {
            None
        }
    }

    pub fn emit_partial_fill(order: &Order<T>, filled_amount: FixedU128) {
        Self::deposit_event(RawEvent::PartialFillLimitOrder(order.id,
                                                            order.trading_pair,
                                                            order.order_type.clone(),
                                                            order.price,
                                                            filled_amount,
                                                            order.trader.clone()));
    }

    pub fn emit_complete_fill(order: &Order<T>, filled_amount: FixedU128) {
        Self::deposit_event(RawEvent::FulfilledLimitOrder(order.id,
                                                          order.trading_pair,
                                                          order.order_type.clone(),
                                                          order.price,
                                                          filled_amount,
                                                          order.trader.clone()));
    }

    // Cancels an existing active order
    pub fn cancel_order_from_orderbook(trader: T::AccountId, order_id: T::Hash, trading_pair: T::Hash, price: FixedU128) -> Result<(), Error<T>> {
        // There are two situations we get the LinkedPriceLevel delete the order from that FIFO
        // FIFO can be empty after this operation so we delete the LinkedPriceLevel and modify the
        // next and prev of LinkedPriceLevels previous and next to this one.
        // Also delete the price from AsksLevels or BidsLevels as per the current_order.
        let mut current_linkedpricelevel: LinkedPriceLevel<T> = <PriceLevels<T>>::take(trading_pair, price);
        let mut index = 0;
        let mut match_flag = false;
        // TODO: Can we define removed_order like in the comments?
        // let removed_order: Order<T>;
        let mut removed_order: Order<T> = Order {
            id: Default::default(),
            trading_pair: Default::default(),
            trader: Default::default(),
            price: Default::default(),
            quantity: Default::default(),
            order_type: OrderType::BidLimit,
        };
        // TODO: Can we optimize this iteration? or even completely remove it?
        for order in current_linkedpricelevel.orders.iter() {
            if order.id == order_id {
                removed_order = current_linkedpricelevel.orders.remove(index).unwrap();
                match_flag = true;
                break;
            }
            index = index + 1;
        }
        ensure!(match_flag, <Error<T>>::InvalidOrderID);
        ensure!(removed_order.trader == trader,<Error<T>>::InvalidOrigin);
        ensure!(removed_order.trading_pair == trading_pair,<Error<T>>::TradingPairMismatch);
        ensure!(removed_order.price == price,<Error<T>>::CancelPriceDoesntMatch);


        if !current_linkedpricelevel.orders.is_empty() {
            // Current LinkedPriceLevel contains other orders so write it back to storage and exit
            <PriceLevels<T>>::insert(trading_pair, price, current_linkedpricelevel);
            return Ok(());
        }
        // There are no more orders in the current linkedPricelevel struct so we need to remove it also
        // make sure the linkedlist is not broken when this linked item was removed so modify the next and prev members accordingly.
        // Also check if the it is the best_bid_price or best_ask_price if so modify that too.
        if current_linkedpricelevel.prev.is_some() && current_linkedpricelevel.next.is_some() {
            let mut prev_linkedpricelevel: LinkedPriceLevel<T> = <PriceLevels<T>>::get(trading_pair, current_linkedpricelevel.prev.unwrap());
            let mut next_linkedpricelevel: LinkedPriceLevel<T> = <PriceLevels<T>>::get(trading_pair, current_linkedpricelevel.next.unwrap());

            // Fix the broken linkedlist
            prev_linkedpricelevel.next = current_linkedpricelevel.next;
            next_linkedpricelevel.prev = current_linkedpricelevel.prev;

            // Write it back
            <PriceLevels<T>>::insert(trading_pair, current_linkedpricelevel.prev.unwrap(), prev_linkedpricelevel);
            <PriceLevels<T>>::insert(trading_pair, current_linkedpricelevel.next.unwrap(), next_linkedpricelevel);
        }

        if current_linkedpricelevel.prev.is_some() && current_linkedpricelevel.next.is_none() {
            let mut prev_linkedpricelevel: LinkedPriceLevel<T> = <PriceLevels<T>>::get(trading_pair, current_linkedpricelevel.prev.unwrap());

            // Fix the broken linkedlist
            prev_linkedpricelevel.next = None;

            // Write it back
            <PriceLevels<T>>::insert(trading_pair, current_linkedpricelevel.prev.unwrap(), prev_linkedpricelevel);
        }
        if current_linkedpricelevel.prev.is_none() && current_linkedpricelevel.next.is_some() {
            let mut next_linkedpricelevel: LinkedPriceLevel<T> = <PriceLevels<T>>::get(trading_pair, current_linkedpricelevel.next.unwrap());

            // Fix the broken linkedlist
            next_linkedpricelevel.prev = None;

            // Write it back
            <PriceLevels<T>>::insert(trading_pair, current_linkedpricelevel.next.unwrap(), next_linkedpricelevel);

            // Update the orderbook
            let mut orderbook: Orderbook<T> = <Orderbooks<T>>::get(trading_pair);
            // Update the best_bid_price if applicable
            if removed_order.order_type == OrderType::BidLimit && price == orderbook.best_bid_price {
                orderbook.best_bid_price = current_linkedpricelevel.next.unwrap();
            }
            // Update the best_ask_price if applicable
            if removed_order.order_type == OrderType::AskLimit && price == orderbook.best_ask_price {
                orderbook.best_ask_price = current_linkedpricelevel.next.unwrap();
            }
            // Write orderbook back to storage
            <Orderbooks<T>>::insert(trading_pair, orderbook);
        }
        Ok(())
    }


    // Helper Functions
    #[allow(dead_code)]
    fn u32_to_asset_id(input: u32) -> T::AssetId {
        input.into()
    }
}