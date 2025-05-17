use anchor_lang::prelude::*;
use anchor_spl::token::{self, Token, TokenAccount, Mint, Transfer};
use pyth_solana_receiver_sdk::price_update::PriceUpdateV2;

declare_id!("573mPaFytnEp1y9oKtHd1aNfwcxRc4ExYY1LthCVR4sX");

// Core data structures with proper implementation
#[account]
pub struct Market {
    pub authority: Pubkey,
    pub base_mint: Pubkey,
    pub quote_mint: Pubkey,
    pub base_vault: Pubkey,
    pub quote_vault: Pubkey,
    pub vault_signer_bump: u8,
    pub registry: Pubkey,
    
    // Basic market parameters
    pub min_base_order_size: u64,
    pub tick_size: u64,
    pub taker_fee_bps: u16,
    pub maker_rebate_bps: u16,
    
    // Order tracking
    pub next_order_id: u64,
    pub next_client_id: u64,
    pub status: MarketStatus,
    
    // Market metadata
    pub market_name: String,
    pub market_symbol: String,
    pub asset_id: String,
    pub is_perpetual: bool,
    pub settle_with_usdc: bool,
    
    // Perpetual market data
    pub last_funding_timestamp: u64,
    pub last_oracle_price: u64,
    pub oracle_price_offset: i64,
    pub mark_price_twap: u64,
    pub open_interest_long: u64,
    pub open_interest_short: u64,
    pub cumulative_funding_long: i64,
    pub cumulative_funding_short: i64,
    pub funding_interval: u64,
    pub max_leverage: u16,
    
    // User position data (for perpetuals)
    pub user_positions: Vec<(Pubkey, Position)>,
    
    // Oracle feed ID for Pyth integration
    pub oracle_feed_id: [u8; 32],
    pub max_oracle_age: u64,
}

impl Market {
    pub const SIZE: usize = 32 + 32 + 32 + 32 + 32 + 1 + 32 + 
                           8 + 8 + 2 + 2 + 
                           8 + 8 + 1 + 
                           64 + 32 + 32 + 1 + 1 + 
                           8 + 8 + 8 + 8 + 8 + 8 + 8 + 8 + 8 + 2 +
                           4 + (50 * (32 + 8 + 8 + 8 + 2 + 8 + 8)) +
                           32 + 8; // Added oracle_feed_id and max_oracle_age

    pub fn get_position(&self, user: &Pubkey) -> Option<(usize, &Position)> {
        self.user_positions
            .iter()
            .enumerate()
            .find(|(_, (owner, _))| owner == user)
            .map(|(idx, (_, position))| (idx, position))
    }

    pub fn get_position_mut(&mut self, user: &Pubkey) -> Option<(usize, &mut Position)> {
        self.user_positions
            .iter_mut()
            .enumerate()
            .find(|(_, (owner, _))| owner == user)
            .map(|(idx, (_, position))| (idx, position))
    }
}
#[account]
pub struct Orderbook {
    pub market: Pubkey,
    pub bids: Vec<(u64, Vec<Order>)>, // Price level -> Orders at that price
    pub asks: Vec<(u64, Vec<Order>)>, // Price level -> Orders at that price
}

impl Orderbook {
    pub const SIZE: usize = 32 + 
                          4 + (50 * (8 + 4 + (20 * (8 + 8 + 32 + 1 + 8 + 8 + 1 + 8 + 1 + 1)))) +
                          4 + (50 * (8 + 4 + (20 * (8 + 8 + 32 + 1 + 8 + 8 + 1 + 8 + 1 + 1))));

    // Find orders for a specific user
    pub fn find_orders_for_user(&self, user: &Pubkey) -> Vec<(Side, u64, usize, usize)> {
        let mut orders = Vec::new();
        
        // Check bids
        for (price_idx, (price, price_orders)) in self.bids.iter().enumerate() {
            for (order_idx, order) in price_orders.iter().enumerate() {
                if &order.user == user {
                    orders.push((Side::Bid, *price, price_idx, order_idx));
                }
            }
        
        }
        
        // Check asks
        for (price_idx, (price, price_orders)) in self.asks.iter().enumerate() {
            for (order_idx, order) in price_orders.iter().enumerate() {
                if &order.user == user {
                    orders.push((Side::Ask, *price, price_idx, order_idx));
                }
            }
        }
        
        orders
    }

    // Get the best bid price
    pub fn best_bid_price(&self) -> Option<u64> {
        self.bids
            .first()
            .map(|(price, _)| *price)
    }
    
    // Get the best ask price
    pub fn best_ask_price(&self) -> Option<u64> {
        self.asks
            .first()
            .map(|(price, _)| *price)
    }

    // Calculate mid-price
    pub fn mid_price(&self) -> Option<u64> {
        match (self.best_bid_price(), self.best_ask_price()) {
            (Some(bid), Some(ask)) => Some((bid + ask) / 2),
            (Some(bid), None) => Some(bid),
            (None, Some(ask)) => Some(ask),
            (None, None) => None,
        }
    }
    
    // Place a bid order at a specific price level
    pub fn place_bid(&mut self, price: u64, order: Order) {
        let position = self.bids
            .iter()
            .position(|(p, _)| *p == price);
            
        if let Some(idx) = position {
            // Add to existing price level
            self.bids[idx].1.push(order);
        } else {
            // Create new price level
            self.bids.push((price, vec![order]));
            // Sort bids in descending order (highest first)
            self.bids.sort_by(|(a, _), (b, _)| b.cmp(a));
        }
    }
    
    // Place an ask order at a specific price level
    pub fn place_ask(&mut self, price: u64, order: Order) {
        let position = self.asks
            .iter()
            .position(|(p, _)| *p == price);
            
        if let Some(idx) = position {
            // Add to existing price level
            self.asks[idx].1.push(order);
        } else {
            // Create new price level
            self.asks.push((price, vec![order]));
            // Sort asks in ascending order (lowest first)
            self.asks.sort_by(|(a, _), (b, _)| a.cmp(b));
        }
    }
} 
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug)]
pub struct Order {
    pub id: u64,
    pub client_id: u64,
    pub user: Pubkey,
    pub side: Side,
    pub price: u64,
    pub size: u64,
    pub remaining_size: u64,
    pub time_in_force: u8,
    pub timestamp: u64,
    pub reduce_only: bool,
    pub post_only: bool,
}

impl Order {
    pub fn new(
        id: u64,
        client_id: u64,
        user: Pubkey,
        side: Side,
        price: u64,
        size: u64,
        time_in_force: u8,
        reduce_only: bool,
        post_only: bool,
    ) -> Self {
        let timestamp = Clock::get().unwrap().unix_timestamp as u64;
        Self {
            id,
            client_id,
            user,
            side,
            price,
            size,
            remaining_size: size,
            time_in_force,
            timestamp,
            reduce_only,
            post_only,
        }
    }
    
    pub fn is_filled(&self) -> bool {
        self.remaining_size == 0
    }
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug)]
pub struct Position {
    pub side: Side,
    pub size: u64,
    pub margin: u64,
    pub entry_price: u64,
    pub leverage: u16,
    pub last_funding_index: i64,
    pub realized_pnl: i64,
    pub liquidation_price: u64,
    pub last_updated_timestamp: u64,
}

impl Position {
    pub fn new(side: Side, margin: u64) -> Self {
        Self {
            side,
            size: 0,
            margin,
            entry_price: 0,
            leverage: 1,
            last_funding_index: 0,
            realized_pnl: 0,
            liquidation_price: 0,
            last_updated_timestamp: Clock::get().unwrap().unix_timestamp as u64,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.size == 0
    }

    // Calculate the position's unrealized PnL at a given price
    pub fn calculate_unrealized_pnl(&self, current_price: u64) -> i64 {
        if self.size == 0 {
            return 0;
        }

        match self.side {
            Side::Bid => ((current_price as i128 - self.entry_price as i128) * self.size as i128 / 1_000_000) as i64,
            Side::Ask => ((self.entry_price as i128 - current_price as i128) * self.size as i128 / 1_000_000) as i64,
        }
    }

    // Calculate position equity (margin + unrealized PnL)
    pub fn equity(&self, current_price: u64) -> u64 {
        let unrealized_pnl = self.calculate_unrealized_pnl(current_price);
        if unrealized_pnl < 0 && self.margin < unrealized_pnl.unsigned_abs() {
            0
        } else {
            self.margin + unrealized_pnl.max(-(self.margin as i64)) as u64
        }
    }

    // Calculate the position's notional value
    pub fn notional_value(&self, current_price: u64) -> u64 {
        self.size * current_price / 1_000_000
    }

    // Update the position's liquidation price
    pub fn update_liquidation_price(&mut self, maintenance_margin_ratio: u16) {
        if self.size == 0 {
            self.liquidation_price = 0;
            return;
        }

        let maintenance_ratio = maintenance_margin_ratio as f64 / 10000.0;

        match self.side {
            Side::Bid => {
                let ratio = 1.0 - (1.0 / self.leverage as f64) + maintenance_ratio;
                self.liquidation_price = (self.entry_price as f64 * ratio) as u64;
            },
            Side::Ask => {
                let ratio = 1.0 + (1.0 / self.leverage as f64) - maintenance_ratio;
                self.liquidation_price = (self.entry_price as f64 * ratio) as u64;
            }
        }
    }

    // Check if the position is liquidatable at the current price
    pub fn is_liquidatable(&self, current_price: u64, maintenance_margin_ratio: u16) -> bool {
        if self.size == 0 {
            return false;
        }

        let position_value = self.notional_value(current_price);
        let required_margin = position_value * maintenance_margin_ratio as u64 / 10000;
        
        let equity = self.equity(current_price);
        equity < required_margin
    }
} 
// Enums
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Side {
    Bid,
    Ask,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum OrderType {
    Limit,
    ImmediateOrCancel,
    PostOnly,
    Market,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum SelfTradeBehavior {
    DecrementTake,
    CancelMaker,
    CancelTaker,
    CancelBoth,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum MarketStatus {
    Active,
    Paused,
    Closed,
}

// Events
#[event]
pub struct MarketCreated {
    pub market: Pubkey,
    pub base_mint: Pubkey,
    pub quote_mint: Pubkey,
    pub market_name: String,
    pub market_symbol: String,
    pub asset_id: String,
    pub is_perpetual: bool,
    pub min_base_order_size: u64,
    pub tick_size: u64,
    pub taker_fee_bps: u16,
    pub maker_rebate_bps: u16,
    pub max_leverage: u16,
}

#[event]
pub struct MarketStatusChanged {
    pub market: Pubkey,
    pub status: MarketStatus,
    pub timestamp: u64,
}

#[event]
pub struct OrderMatched {
    pub market: Pubkey,
    pub order_id: u64,
    pub maker_order_id: u64,
    pub client_id: u64,
    pub maker_client_id: u64,
    pub user: Pubkey,
    pub maker: Pubkey,
    pub side: Side,
    pub price: u64,
    pub size: u64,
    pub quote_amount: u64,
    pub taker_fee: u64,
    pub maker_rebate: u64,
    pub remaining_size: u64,
    pub timestamp: u64,
}

#[event]
pub struct BidOrderAdded {
    pub market: Pubkey,
    pub order_id: u64,
    pub client_id: u64,
    pub user: Pubkey,
    pub price: u64,
    pub size: u64,
    pub reduce_only: bool,
    pub post_only: bool,
    pub timestamp: u64,
}

#[event]
pub struct AskOrderAdded {
    pub market: Pubkey,
    pub order_id: u64,
    pub client_id: u64,
    pub user: Pubkey,
    pub price: u64,
    pub size: u64,
    pub reduce_only: bool,
    pub post_only: bool,
    pub timestamp: u64,
}

#[event]
pub struct OrderCancelled {
    pub market: Pubkey,
    pub order_id: u64,
    pub client_id: u64,
    pub user: Pubkey,
    pub side: Side,
    pub price: u64,
    pub remaining_size: u64,
    pub reduce_only: bool,
    pub timestamp: u64,
}

#[event]
pub struct FundingRateUpdated {
    pub market: Pubkey,
    pub oracle_price: u64,
    pub mark_price: u64,
    pub premium_index: i64,
    pub funding_rate: i64,
    pub timestamp: u64,
}

#[event]
pub struct PositionLiquidated {
    pub market: Pubkey,
    pub user: Pubkey,
    pub liquidator: Pubkey,
    pub side: Side,
    pub size: u64,
    pub position_value: u64,
    pub maintenance_margin: u64,
    pub liquidation_fee: u64,
    pub remaining: u64,
    pub oracle_price: u64,
    pub timestamp: u64,
}

#[event]
pub struct CollateralDeposited {
    pub market: Pubkey,
    pub user: Pubkey,
    pub amount: u64,
    pub total_margin: u64,
    pub timestamp: u64,
}

#[event]
pub struct CollateralWithdrawn {
    pub market: Pubkey,
    pub user: Pubkey,
    pub amount: u64,
    pub remaining_margin: u64,
    pub timestamp: u64,
}

#[event]
pub struct PositionUpdated {
    pub market: Pubkey,
    pub user: Pubkey,
    pub side: Side,
    pub size: u64,
    pub margin: u64,
    pub entry_price: u64,
    pub leverage: u16,
    pub realized_pnl: i64,
    pub liquidation_price: u64,
    pub timestamp: u64,
}

// Error codes
#[error_code]
pub enum ErrorCode {
    #[msg("Invalid parameters")]
    InvalidParameters,
    
    #[msg("Order size too small")]
    OrderSizeTooSmall,
    
    #[msg("Invalid tick size")]
    InvalidTickSize,
    
    #[msg("Market inactive")]
    MarketInactive,
    
    #[msg("Order not found")]
    OrderNotFound,
    
    #[msg("Invalid orderbook")]
    InvalidOrderbook,
    
    #[msg("Invalid vault")]
    InvalidVault,
    
    #[msg("Invalid authority")]
    InvalidAuthority,
    
    #[msg("Post-only order would match")]
    PostOnlyWouldMatch,
    
    #[msg("Invalid reduce-only order - would increase position")]
    InvalidReduceOnlyOrder,
    
    #[msg("Invalid reduce-only size - exceeds position size")]
    InvalidReduceOnlySize,
    
    #[msg("No position to reduce")]
    NoPositionToReduce,
    
    #[msg("Not a perpetual market")]
    NotPerpetualMarket,
    
    #[msg("Funding rate update too soon")]
    FundingRateTooSoon,
    
    #[msg("Position not found")]
    PositionNotFound,
    
    #[msg("Position not liquidatable")]
    PositionNotLiquidatable,
    
    #[msg("Insufficient margin")]
    InsufficientMargin,
    
    #[msg("Withdrawal would trigger liquidation")]
    WithdrawalWouldTriggerLiquidation,
    
    #[msg("Exceeds maximum leverage")]
    ExceedsMaxLeverage,
    
    #[msg("Asset not available or inactive")]
    AssetNotAvailable,
    
    #[msg("Invalid registry")]
    InvalidRegistry,
    
    #[msg("Invalid price feed")]
    InvalidPriceFeed,
    
    #[msg("Invalid order type")]
    InvalidOrderType,
    
    #[msg("Self-trade prevented")]
    SelfTradePrevented,
    
    #[msg("Price outside allowed range")]
    PriceOutOfRange,
    
    #[msg("Market full - too many orders or positions")]
    MarketFull,
}

#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(
        init,
        payer = authority,
        space = 8 + Market::SIZE
    )]
    pub market: Account<'info, Market>,
    
    #[account(
        init,
        payer = authority,
        space = 8 + Orderbook::SIZE
    )]
    pub orderbook: Account<'info, Orderbook>,
    
    pub base_mint: Account<'info, Mint>,
    pub quote_mint: Account<'info, Mint>,
    
    #[account(
        init,
        payer = authority,
        token::mint = base_mint,
        token::authority = vault_signer,
    )]
    pub base_vault: Account<'info, TokenAccount>,
    
    #[account(
        init,
        payer = authority,
        token::mint = quote_mint,
        token::authority = vault_signer,
    )]
    pub quote_vault: Account<'info, TokenAccount>,
    
    /// CHECK: The vault signer PDA
    #[account(
        seeds = [b"vault_signer", market.key().as_ref()],
        bump,
    )]
    pub vault_signer: AccountInfo<'info>,
    
    #[account(mut)]
    pub authority: Signer<'info>,
    
    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
    pub rent: Sysvar<'info, Rent>,
}


#[derive(Accounts)]
pub struct PlaceOrder<'info> {
    #[account(mut)]
    pub market: Account<'info, Market>,
    
    #[account(mut, constraint = orderbook.market == market.key() @ ErrorCode::InvalidOrderbook)]
    pub orderbook: Account<'info, Orderbook>,
    
    /// This account is optional for perpetual markets
    #[account(mut)]
    pub user_base_account: Option<Account<'info, TokenAccount>>,
    
    #[account(mut)]
    pub user_quote_account: Account<'info, TokenAccount>,
    
    #[account(mut, constraint = base_vault.key() == market.base_vault @ ErrorCode::InvalidVault)]
    pub base_vault: Account<'info, TokenAccount>,
    
    #[account(mut, constraint = quote_vault.key() == market.quote_vault @ ErrorCode::InvalidVault)]
    pub quote_vault: Account<'info, TokenAccount>,
    
    /// CHECK: The vault signer PDA
    #[account(
        seeds = [b"vault_signer", market.key().as_ref()],
        bump = market.vault_signer_bump,
    )]
    pub vault_signer: AccountInfo<'info>,
    
    #[account(signer)]
    pub user: AccountInfo<'info>,
    
    /// Registry account only needed for checking perpetual assets
    #[account(constraint = registry.key() == market.registry @ ErrorCode::InvalidRegistry)]
    pub registry: Account<'info, omniliquid_registry::Registry>,
    
    /// Optional Pyth price feed for price validation
    pub pyth_price_feed: Option<Account<'info, PriceUpdateV2>>,
    
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct CancelOrder<'info> {
    #[account(mut)]
    pub market: Account<'info, Market>,
    
    #[account(mut, constraint = orderbook.market == market.key() @ ErrorCode::InvalidOrderbook)]
    pub orderbook: Account<'info, Orderbook>,
    
    #[account(mut)]
    pub user_base_account: Option<Account<'info, TokenAccount>>,
    
    #[account(mut)]
    pub user_quote_account: Option<Account<'info, TokenAccount>>,
    
    #[account(mut)]
    pub base_vault: Option<Account<'info, TokenAccount>>,
    
    #[account(mut)]
    pub quote_vault: Option<Account<'info, TokenAccount>>,
    
    /// CHECK: The vault signer PDA
    #[account(
        seeds = [b"vault_signer", market.key().as_ref()],
        bump = market.vault_signer_bump,
    )]
    pub vault_signer: Option<AccountInfo<'info>>,
    
    #[account(signer)]
    pub user: AccountInfo<'info>,
    
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct ChangeMarketStatus<'info> {
    #[account(mut, constraint = market.authority == authority.key() @ ErrorCode::InvalidAuthority)]
    pub market: Account<'info, Market>,
    
    #[account(signer)]
    pub authority: AccountInfo<'info>,
}

#[derive(Accounts)]
pub struct UpdateFundingRate<'info> {
    #[account(mut)]
    pub market: Account<'info, Market>,
    
    #[account(constraint = registry.key() == market.registry @ ErrorCode::InvalidRegistry)]
    pub registry: Account<'info, omniliquid_registry::Registry>,
    
    pub pyth_price_feed: Account<'info, PriceUpdateV2>,
    
    #[account(mut)]
    pub orderbook: Account<'info, Orderbook>,
    
    #[account(mut, signer)]
    pub authority: AccountInfo<'info>,
}

// For LiquidatePosition
#[derive(Accounts)]
pub struct LiquidatePosition<'info> {
    #[account(mut)]
    pub market: Account<'info, Market>,
    
    #[account(constraint = registry.key() == market.registry @ ErrorCode::InvalidRegistry)]
    pub registry: Account<'info, omniliquid_registry::Registry>,
    
    pub pyth_price_feed: Account<'info, PriceUpdateV2>,
    
    #[account(mut, constraint = quote_vault.key() == market.quote_vault @ ErrorCode::InvalidVault)]
    pub quote_vault: Account<'info, TokenAccount>,
    
    #[account(mut)]
    pub liquidator_quote_account: Account<'info, TokenAccount>,
    
    #[account(mut)]
    pub user_quote_account: Account<'info, TokenAccount>,
    
    /// CHECK: The vault signer PDA
    #[account(
        seeds = [b"vault_signer", market.key().as_ref()],
        bump = market.vault_signer_bump,
    )]
    pub vault_signer: AccountInfo<'info>,
    
    /// This is the account being liquidated
    /// CHECK: Not a signer, verified in the program
    pub user: AccountInfo<'info>,
    
    #[account(mut, signer)]
    pub liquidator: AccountInfo<'info>,
    
    pub token_program: Program<'info, Token>,
}
// For ManageCollateral
#[derive(Accounts)]
pub struct ManageCollateral<'info> {
    #[account(mut)]
    pub market: Account<'info, Market>,
    
    #[account(constraint = registry.key() == market.registry @ ErrorCode::InvalidRegistry)]
    pub registry: Account<'info, omniliquid_registry::Registry>,
    
    pub pyth_price_feed: Option<Account<'info, PriceUpdateV2>>,
    
    #[account(mut, constraint = quote_vault.key() == market.quote_vault @ ErrorCode::InvalidVault)]
    pub quote_vault: Account<'info, TokenAccount>,
    
    #[account(mut)]
    pub user_quote_account: Account<'info, TokenAccount>,
    
    /// CHECK: The vault signer PDA
    #[account(
        seeds = [b"vault_signer", market.key().as_ref()],
        bump = market.vault_signer_bump,
    )]
    pub vault_signer: AccountInfo<'info>,
    
    #[account(mut, signer)]
    pub user: AccountInfo<'info>,
    
    pub token_program: Program<'info, Token>,
}

// Helper function to convert hex string to feed ID
pub fn get_feed_id_from_hex(hex_string: &str) -> Result<[u8; 32]> {
    let mut feed_id = [0u8; 32];
    let hex_string = hex_string.strip_prefix("0x").unwrap_or(hex_string);
    
    require!(hex_string.len() == 64, ErrorCode::InvalidParameters);
    
    for i in 0..32 {
        let byte_str = &hex_string[i * 2..i * 2 + 2];
        feed_id[i] = u8::from_str_radix(byte_str, 16).map_err(|_| ErrorCode::InvalidParameters)?;
    }
    
    Ok(feed_id)
}

// Helper functions for Pyth price feed
fn get_pyth_price(price_update: &Account<PriceUpdateV2>, market: &Account<Market>) -> Result<u64> {
    // Maximum age check is now handled by get_price_no_older_than
    let price = price_update.get_price_no_older_than(
        &Clock::get()?,
        market.max_oracle_age,
        &market.oracle_feed_id
    ).map_err(|_| ErrorCode::InvalidPriceFeed)?;
    
    // Convert to 6 decimals (standard for our pricing)
    let exponent = price.exponent;
    let mantissa = price.price;
    
    // Calculate the scaled price with 6 decimal places
    let scaled_price = if exponent < -6 {
        // If price has more precision than 6 decimals, truncate
        let factor = 10i64.pow((-6 - exponent) as u32);
        mantissa.abs() as u64 / factor as u64
    } else {
        // If price has less precision, multiply
        let factor = 10i64.pow((exponent + 6) as u32);
        mantissa.abs() as u64 * factor as u64
    };
    
    Ok(scaled_price)
}
// Program implementation start
#[program]
pub mod omniliquid_clob {
    use super::*;


    pub fn initialize(
        ctx: Context<Initialize>,
        market_name: String,
        market_symbol: String,
        asset_id: String,
        is_perpetual: bool,
        settle_with_usdc: bool,
        min_base_order_size: u64,
        tick_size: u64,
        taker_fee_bps: u16,
        maker_rebate_bps: u16,
        max_leverage: u16,
        funding_interval: u64,
        vault_signer_bump: u8,
        registry: Pubkey,
        oracle_feed_id_hex: String,
        max_oracle_age: u64,
    ) -> Result<()> {
        // Validate parameters
        require!(
            taker_fee_bps <= 500 && maker_rebate_bps <= taker_fee_bps,
            ErrorCode::InvalidParameters
        );
        
        require!(
            min_base_order_size > 0 && tick_size > 0,
            ErrorCode::InvalidParameters
        );
        
        require!(
            market_name.len() <= 32 && market_symbol.len() <= 16 && asset_id.len() <= 16,
            ErrorCode::InvalidParameters
        );
        
        require!(max_leverage > 0 && max_leverage <= 10000, ErrorCode::InvalidParameters);
        
        // Parse oracle feed ID
        let oracle_feed_id = get_feed_id_from_hex(&oracle_feed_id_hex)?;
        
        let market = &mut ctx.accounts.market;
        let orderbook = &mut ctx.accounts.orderbook;
        
        // Initialize market
        market.authority = ctx.accounts.authority.key();
        market.base_mint = ctx.accounts.base_mint.key();
        market.quote_mint = ctx.accounts.quote_mint.key();
        market.base_vault = ctx.accounts.base_vault.key();
        market.quote_vault = ctx.accounts.quote_vault.key();
        market.vault_signer_bump = vault_signer_bump;
        market.registry = registry;
        
        // Set basic parameters
        market.min_base_order_size = min_base_order_size;
        market.tick_size = tick_size;
        market.taker_fee_bps = taker_fee_bps;
        market.maker_rebate_bps = maker_rebate_bps;
        market.max_leverage = max_leverage;
        
        // Initialize order tracking
        market.next_order_id = 1;
        market.next_client_id = 1;
        market.status = MarketStatus::Active;
        
        // Set market metadata
        market.market_name = market_name.clone();
        market.market_symbol = market_symbol.clone();
        market.asset_id = asset_id.clone();
        market.is_perpetual = is_perpetual;
        market.settle_with_usdc = settle_with_usdc;
        
        // Initialize perpetual market data
        let current_timestamp = Clock::get()?.unix_timestamp as u64;
        market.last_funding_timestamp = current_timestamp;
        market.last_oracle_price = 0;
        market.oracle_price_offset = 0;
        market.mark_price_twap = 0;
        market.open_interest_long = 0;
        market.open_interest_short = 0;
        market.cumulative_funding_long = 0;
        market.cumulative_funding_short = 0;
        market.funding_interval = funding_interval;
        
        // Set oracle parameters
        market.oracle_feed_id = oracle_feed_id;
        market.max_oracle_age = max_oracle_age;
        
        // Initialize positions and orderbook
        market.user_positions = Vec::new();
        orderbook.market = market.key();
        orderbook.bids = Vec::new();
        orderbook.asks = Vec::new();
        
        emit!(MarketCreated {
            market: market.key(),
            base_mint: market.base_mint,
            quote_mint: market.quote_mint,
            market_name,
            market_symbol,
            asset_id,
            is_perpetual,
            min_base_order_size,
            tick_size,
            taker_fee_bps,
            maker_rebate_bps,
            max_leverage,
        });
        
        Ok(())
    }

    pub fn change_market_status(
        ctx: Context<ChangeMarketStatus>, 
        new_status: MarketStatus
    ) -> Result<()> {
        let market = &mut ctx.accounts.market;
        market.status = new_status;
        
        emit!(MarketStatusChanged {
            market: market.key(),
            status: new_status,
            timestamp: Clock::get()?.unix_timestamp as u64,
        });
        
        Ok(())
    }
}
  pub fn update_funding_rate(ctx: Context<UpdateFundingRate>) -> Result<()> {
        let market = &mut ctx.accounts.market;
        let orderbook = &ctx.accounts.orderbook;
        
        // Verify this is a perpetual market
        require!(market.is_perpetual, ErrorCode::NotPerpetualMarket);
        
        // Check if enough time has passed since last update
        let current_time = Clock::get()?.unix_timestamp as u64;
        require!(
            current_time >= market.last_funding_timestamp + market.funding_interval,
            ErrorCode::FundingRateTooSoon
        );
        
        // Get oracle price from Pyth
        let oracle_price = get_pyth_price(&ctx.accounts.pyth_price_feed, market)?;
        
        // Calculate mark price from orderbook
        let mark_price = match orderbook.mid_price() {
            Some(price) => price,
            None => oracle_price, // If orderbook is empty, use oracle price
        };
        
        // Calculate funding rate components
        // Premium index = (mark_price - oracle_price) / oracle_price
        let premium_index = if oracle_price > 0 {
            ((mark_price as i128 - oracle_price as i128) * 10000 / oracle_price as i128) as i64
        } else {
            0
        };
        
        // Funding rate = premium_index / 24 (hourly rate)
        let funding_rate = premium_index / 24;
        
        // Copy current funding index before updating positions
        let current_funding_long = market.cumulative_funding_long;
        
        // Apply funding payments to all positions
        for (_, position) in market.user_positions.iter_mut() {
            if position.size > 0 {
                let position_value = position.size * oracle_price / 1_000_000;
                
                // Calculate funding payment based on position side
                let funding_payment = match position.side {
                    Side::Bid => -funding_rate * position_value as i64 / 10000,
                    Side::Ask => funding_rate * position_value as i64 / 10000,
                };
                
                // Apply funding payment to position's realized PnL
                position.realized_pnl += funding_payment;
                
                // Update position's last funding index
                position.last_funding_index = current_funding_long;
            }
        }
        
        // Update market data
        market.last_oracle_price = oracle_price;
        market.mark_price_twap = mark_price; // For simplicity, just using current mark price
        market.last_funding_timestamp = current_time;
        
        // Update cumulative funding rates
        market.cumulative_funding_long += funding_rate;
        market.cumulative_funding_short -= funding_rate;
        
        emit!(FundingRateUpdated {
            market: market.key(),
            oracle_price,
            mark_price,
            premium_index,
            funding_rate,
            timestamp: current_time,
        });
        
        Ok(())
    }

pub fn place_order(
    ctx: Context<PlaceOrder>,
    client_id: Option<u64>,
    side: Side,
    price: u64,
    size: u64,
    order_type: OrderType,
    self_trade_behavior: SelfTradeBehavior,
    reduce_only: bool,
    post_only: bool,
    leverage: Option<u16>,
) -> Result<()> {
    let market = &mut ctx.accounts.market;
    let orderbook = &mut ctx.accounts.orderbook;
    let user_key = ctx.accounts.user.key();
    
    // Validate market is active
    require!(market.status == MarketStatus::Active, ErrorCode::MarketInactive);
    
    // Validate order size
    require!(size >= market.min_base_order_size, ErrorCode::OrderSizeTooSmall);
    
    // Validate tick size for limit orders
        if order_type != OrderType::Market {
            require!(price % market.tick_size == 0, ErrorCode::InvalidTickSize);
            
            // Validate price is within reasonable range of current price if perpmarket
            if market.is_perpetual && ctx.accounts.pyth_price_feed.is_some() {
                let oracle_price = get_pyth_price(ctx.accounts.pyth_price_feed.as_ref().unwrap(), market)?;
                
                // Price should be within ±50% of oracle price
                let min_price = oracle_price / 2;
                let max_price = oracle_price * 3 / 2;
                
                require!(
                    price >= min_price && price <= max_price,
                    ErrorCode::PriceOutOfRange
                );
            }
        }
        
    // Check for reduce_only constraints
    if reduce_only {
        if let Some((_, position)) = market.get_position(&user_key) {
            match side {
                Side::Bid => {
                    // Can only reduce an ASK position
                    require!(
                        position.side == Side::Ask && position.size > 0,
                        ErrorCode::InvalidReduceOnlyOrder
                    );
                    
                    // Cannot exceed position size
                    require!(
                        size <= position.size,
                        ErrorCode::InvalidReduceOnlySize
                    );
                },
                Side::Ask => {
                    // Can only reduce a BID position
                    require!(
                        position.side == Side::Bid && position.size > 0,
                        ErrorCode::InvalidReduceOnlyOrder
                    );
                    
                    // Cannot exceed position size
                    require!(
                        size <= position.size,
                        ErrorCode::InvalidReduceOnlySize
                    );
                }
            }
        } else {
            return Err(ErrorCode::NoPositionToReduce.into());
        }
    }
    
    // For perpetual markets, check if the position needs to be created
    // or if leverage needs to be set
    if market.is_perpetual {
        // If this is a new position, check if leverage is provided
        if leverage.is_some() {
            let lev = leverage.unwrap();
            require!(
                lev > 0 && lev <= market.max_leverage,
                ErrorCode::ExceedsMaxLeverage
            );
            
            // Find or create position
            let position_idx_opt = market.user_positions
                .iter()
                .position(|(pubkey, _)| *pubkey == user_key);
            
            if let Some(idx) = position_idx_opt {
                // Update leverage on existing position
                let position = &mut market.user_positions[idx].1;
                position.leverage = lev;
            } else {
                // Create a new position with the specified leverage
                let mut new_position = Position::new(side, 0);
                new_position.leverage = lev;
                market.user_positions.push((user_key, new_position));
            }
        }
    }
    
    // Generate order ID and client ID
    let order_id = market.next_order_id;
    market.next_order_id += 1;
    
    let client_order_id = client_id.unwrap_or_else(|| {
        let id = market.next_client_id;
        market.next_client_id += 1;
        id
    });
    
    // Create the order
    let new_order = Order::new(
        order_id,
        client_order_id,
        user_key,
        side,
        price,
        size,
        0, // Time in force not implemented
        reduce_only,
        post_only,
    );
    
    let timestamp = Clock::get()?.unix_timestamp as u64;
    
    // Process the order based on order type
    match order_type {
        OrderType::Market => {
            let mut filled_size = 0;
            
            if side == Side::Bid {
                // Match against asks
                let mut price_levels_to_remove = Vec::new();
                
                for (idx, (ask_price, ask_orders)) in orderbook.asks.iter_mut().enumerate() {
                    // Track orders to remove at this price level
                    let mut orders_to_remove = Vec::new();
                    
                    for (order_idx, ask_order) in ask_orders.iter_mut().enumerate() {
                        // Check for self-trade
                        if ask_order.user == user_key {
                            match self_trade_behavior {
                                SelfTradeBehavior::CancelBoth | SelfTradeBehavior::CancelMaker => {
                                    orders_to_remove.push(order_idx);
                                    continue;
                                },
                                SelfTradeBehavior::CancelTaker => {
                                    return Err(ErrorCode::SelfTradePrevented.into());
                                },
                                SelfTradeBehavior::DecrementTake => {},
                            }
                        }
                        
                        // Calculate match amount
                        let remaining_to_fill = size - filled_size;
                        let match_amount = std::cmp::min(ask_order.remaining_size, remaining_to_fill);
                        
                        if match_amount > 0 {
                            // Calculate quote amount and fees
                            let quote_amount = match_amount * *ask_price / 1_000_000;
                            let taker_fee = quote_amount * market.taker_fee_bps as u64 / 10000;
                            let maker_rebate = quote_amount * market.maker_rebate_bps as u64 / 10000;
                            
                            // Update the maker order
                            ask_order.remaining_size -= match_amount;
                            filled_size += match_amount;
                            
                            // Emit match event
                            emit!(OrderMatched {
                                market: market.key(),
                                order_id,
                                maker_order_id: ask_order.id,
                                client_id: client_order_id,
                                maker_client_id: ask_order.client_id,
                                user: user_key,
                                maker: ask_order.user,
                                side,
                                price: *ask_price,
                                size: match_amount,
                                quote_amount,
                                taker_fee,
                                maker_rebate,
                                remaining_size: size - filled_size,
                                timestamp,
                            });
                            
                            // Process token transfers for spot markets
                            if !market.is_perpetual {
                                // Transfer logic would go here - omitted for brevity
                            }
                            
                            // Update positions for perpetual markets
                            if market.is_perpetual {
                                let market_key = market.key(); // Clone key before mutable borrow
                                // Update taker position
                                let taker_position_opt = market.get_position_mut(&user_key);

                                if let Some((_, taker_position)) = taker_position_opt {
                                    if taker_position.size == 0 {
                                        // New position
                                        taker_position.side = Side::Bid;
                                        taker_position.size = match_amount;
                                        taker_position.entry_price = *ask_price;
                                    } else if taker_position.side == Side::Bid {
                                        // Add to existing position
                                        let new_size = taker_position.size + match_amount;
                                        taker_position.entry_price =
                                            (taker_position.entry_price * taker_position.size +
                                             *ask_price * match_amount) / new_size;
                                        taker_position.size = new_size;
                                    } else {
                                        // Reduce or flip position
                                        if match_amount < taker_position.size {
                                            taker_position.size -= match_amount;
                                        } else {
                                            taker_position.side = Side::Bid;
                                            taker_position.size = match_amount - taker_position.size;
                                            taker_position.entry_price = *ask_price;
                                        }
                                    }

                                    // Update position metadata
                                    taker_position.last_updated_timestamp = timestamp;
                                    taker_position.update_liquidation_price(500); // Example 5% maintenance margin

                                    // Emit position update
                                    emit!(PositionUpdated {
                                        market: market_key,
                                        user: user_key,
                                        side: taker_position.side,
                                        size: taker_position.size,
                                        margin: taker_position.margin,
                                        entry_price: taker_position.entry_price,
                                        leverage: taker_position.leverage,
                                        realized_pnl: taker_position.realized_pnl,
                                        liquidation_price: taker_position.liquidation_price,
                                        timestamp,
                                    });
                                } else {
                                    // Create new position
                                    let mut new_position = Position::new(Side::Bid, 0);
                                    new_position.size = match_amount;
                                    new_position.entry_price = *ask_price;
                                    new_position.update_liquidation_price(500);
                                    new_position.last_updated_timestamp = timestamp;

                                    market.user_positions.push((user_key, new_position.clone()));

                                    // Emit position update
                                    emit!(PositionUpdated {
                                        market: market_key,
                                        user: user_key,
                                        side: new_position.side,
                                        size: new_position.size,
                                        margin: new_position.margin,
                                        entry_price: new_position.entry_price,
                                        leverage: new_position.leverage,
                                        realized_pnl: new_position.realized_pnl,
                                        liquidation_price: new_position.liquidation_price,
                                        timestamp,
                                    });
                                }
                                
                                // Update maker position (similar logic would be here)
                                
                                // Update market's open interest
                                market.open_interest_long += match_amount;
                            }
                            
                            // If maker order is fully filled, mark for removal
                            if ask_order.remaining_size == 0 {
                                orders_to_remove.push(order_idx);
                            }
                            
                            // Exit if order fully filled
                            if filled_size >= size {
                                break;
                            }
                        }
                    }
                    
                    // Remove filled orders
                    for idx in orders_to_remove.iter().rev() {
                        ask_orders.remove(*idx);
                    }
                    
                    // If price level is empty, mark for removal
                    if ask_orders.is_empty() {
                        price_levels_to_remove.push(idx);
                    }
                    
                    // Exit if order fully filled
                    if filled_size >= size {
                        break;
                    }
                }
                
                // Remove empty price levels
                for idx in price_levels_to_remove.iter().rev() {
                    orderbook.asks.remove(*idx);
                }
            } else {
                // Side::Ask - matching against bids
                // Similar logic as above but matching against the bid side
                // Omitted for brevity
            }
            
            // Market orders should have at least some fill
            require!(filled_size > 0, ErrorCode::OrderNotFound);
        },
        OrderType::Limit => {
            // Try to match immediately like a market order
            let mut filled_size = 0;
            
            if side == Side::Bid {
                // Match against asks (similar to Market order logic above)
                // ...
            } else {
                // Match against bids (similar to Market order logic above)
                // ...
            }
            
            // If not fully filled, add remainder to book
            if filled_size < size {
                let remaining_size = size - filled_size;
                let mut remaining_order = new_order.clone();
                remaining_order.remaining_size = remaining_size;
                
                if side == Side::Bid {
                    orderbook.place_bid(price, remaining_order);
                    
                    emit!(BidOrderAdded {
                        market: market.key(),
                        order_id,
                        client_id: client_order_id,
                        user: user_key,
                        price,
                        size: remaining_size,
                        reduce_only,
                        post_only,
                        timestamp,
                    });
                } else {
                    orderbook.place_ask(price, remaining_order);
                    
                    emit!(AskOrderAdded {
                        market: market.key(),
                        order_id,
                        client_id: client_order_id,
                        user: user_key,
                        price,
                        size: remaining_size,
                        reduce_only,
                        post_only,
                        timestamp,
                    });
                }
            }
        },
        OrderType::PostOnly => {
            // Check if the order would match immediately
            let would_match = match side {
                Side::Bid => {
                    orderbook.asks.iter().any(|(ask_price, _)| *ask_price <= price)
                },
                Side::Ask => {
                    orderbook.bids.iter().any(|(bid_price, _)| *bid_price >= price)
                }
            };
            
            // If the order would match, reject it
            require!(!would_match, ErrorCode::PostOnlyWouldMatch);
            
            // Add order to the orderbook
            if side == Side::Bid {
                orderbook.place_bid(price, new_order);
                
                emit!(BidOrderAdded {
                    market: market.key(),
                    order_id,
                    client_id: client_order_id,
                    user: user_key,
                    price,
                    size,
                    reduce_only,
                    post_only,
                    timestamp,
                });
            } else {
                orderbook.place_ask(price, new_order);
                
                emit!(AskOrderAdded {
                    market: market.key(),
                    order_id,
                    client_id: client_order_id,
                    user: user_key,
                    price,
                    size,
                    reduce_only,
                    post_only,
                    timestamp,
                });
            }
        },
        OrderType::ImmediateOrCancel => {
            // Similar to Limit order, but don't add remainder to the book
            let mut filled_size = 0;
            
            if side == Side::Bid {
                // Match against asks (similar to Market order logic)
                // ...
            } else {
                // Match against bids (similar to Market order logic)
                // ...
            }
            
            // IOC orders should have at least some fill
            if filled_size == 0 {
                return Err(ErrorCode::OrderNotFound.into());
            }
            
            // IOC orders do not get added to the book even if partially filled
        }
    }
    
    Ok(())
}
    pub fn cancel_order(
        ctx: Context<CancelOrder>, 
        order_id: u64, 
        side: Side,
        price: u64
    ) -> Result<()> {
        let market = &ctx.accounts.market;
        let orderbook = &mut ctx.accounts.orderbook;
        let user_key = ctx.accounts.user.key();
        let timestamp = Clock::get()?.unix_timestamp as u64;
        
        // Find the order in the orderbook
        match side {
            Side::Bid => {
                // Find the price level
                let price_level_opt = orderbook.bids.iter_mut()
                    .position(|(p, _)| *p == price);
                
                if let Some(price_idx) = price_level_opt {
                    // Find the order at this price level
                    let order_opt = orderbook.bids[price_idx].1.iter()
                        .position(|order| order.id == order_id && order.user == user_key);
                    
                    if let Some(order_idx) = order_opt {
                        // Get order details before removal for the event
                        let order = &orderbook.bids[price_idx].1[order_idx];
                        let client_id = order.client_id;
                        let remaining_size = order.remaining_size;
                        let reduce_only = order.reduce_only;
                        
                        // Remove the order
                        orderbook.bids[price_idx].1.remove(order_idx);
                        
                        // If no more orders at this price level, remove the price level
                        if orderbook.bids[price_idx].1.is_empty() {
                            orderbook.bids.remove(price_idx);
                        }
                        
                        // For spot markets, return locked tokens
                        if !market.is_perpetual && ctx.accounts.user_quote_account.is_some() {
                            let quote_amount = remaining_size * price / 1_000_000;
                            
                            // Return quote tokens to user
                            if quote_amount > 0 && ctx.accounts.quote_vault.is_some() && ctx.accounts.vault_signer.is_some() {
                                // Create PDA signer seeds
                                let market_key = market.key();
                                let seeds = &[
                                    b"vault_signer".as_ref(),
                                    market_key.as_ref(),
                                    &[market.vault_signer_bump],
                                ];
                                let signer = &[&seeds[..]];
                                
                                token::transfer(
                                    CpiContext::new_with_signer(
                                        ctx.accounts.token_program.to_account_info(),
                                        Transfer {
                                            from: ctx.accounts.quote_vault.as_ref().unwrap().to_account_info(),
                                            to: ctx.accounts.user_quote_account.as_ref().unwrap().to_account_info(),
                                            authority: ctx.accounts.vault_signer.as_ref().unwrap().to_account_info(),
                                        },
                                        signer,
                                    ),
                                    quote_amount,
                                )?;
                            }
                        }
                        
                        emit!(OrderCancelled {
                            market: market.key(),
                            order_id,
                            client_id,
                            user: user_key,
                            side,
                            price,
                            remaining_size,
                            reduce_only,
                            timestamp,
                        });
                        
                        return Ok(());
                    }
                }
            },
            Side::Ask => {
                // Find the price level
                let price_level_opt = orderbook.asks.iter_mut()
                    .position(|(p, _)| *p == price);
                
                if let Some(price_idx) = price_level_opt {
                    // Find the order at this price level
                    let order_opt = orderbook.asks[price_idx].1.iter()
                        .position(|order| order.id == order_id && order.user == user_key);
                    
                    if let Some(order_idx) = order_opt {
                        // Get order details before removal for the event
                        let order = &orderbook.asks[price_idx].1[order_idx];
                        let client_id = order.client_id;
                        let remaining_size = order.remaining_size;
                        let reduce_only = order.reduce_only;
                        
                        // Remove the order
                        orderbook.asks[price_idx].1.remove(order_idx);
                        
                        // If no more orders at this price level, remove the price level
                        if orderbook.asks[price_idx].1.is_empty() {
                            orderbook.asks.remove(price_idx);
                        }
                        
                        // For spot markets, return locked tokens
                        if !market.is_perpetual && ctx.accounts.user_base_account.is_some() {
                            // Return base tokens to user
                            if remaining_size > 0 && ctx.accounts.base_vault.is_some() && ctx.accounts.vault_signer.is_some() {
                                // Create PDA signer seeds
                                let market_key = market.key();
                                let seeds = &[
                                    b"vault_signer".as_ref(),
                                    market_key.as_ref(),
                                    &[market.vault_signer_bump],
                                ];
                                let signer = &[&seeds[..]];
                                
                                token::transfer(
                                    CpiContext::new_with_signer(
                                        ctx.accounts.token_program.to_account_info(),
                                        Transfer {
                                            from: ctx.accounts.base_vault.as_ref().unwrap().to_account_info(),
                                            to: ctx.accounts.user_base_account.as_ref().unwrap().to_account_info(),
                                            authority: ctx.accounts.vault_signer.as_ref().unwrap().to_account_info(),
                                        },
                                        signer,
                                    ),
                                    remaining_size,
                                )?;
                            }
                        }
                        
                        emit!(OrderCancelled {
                            market: market.key(),
                            order_id,
                            client_id,
                            user: user_key,
                            side,
                            price,
                            remaining_size,
                            reduce_only,
                            timestamp,
                        });
                        
                        return Ok(());
                    }
                }
            }
        }
        
        // Order not found
        Err(ErrorCode::OrderNotFound.into())
    }

    pub fn liquidate_position(
        ctx: Context<LiquidatePosition>,
        liquidate_user: Pubkey
    ) -> Result<()> {
        let market = &mut ctx.accounts.market;
        let liquidator_key = ctx.accounts.liquidator.key();
        let timestamp = Clock::get()?.unix_timestamp as u64;
        
        // Verify this is a perpetual market
        require!(market.is_perpetual, ErrorCode::NotPerpetualMarket);
        
        // Get current oracle price from Pyth
        let oracle_price = get_pyth_price(&ctx.accounts.pyth_price_feed, market)?;
        
        // Find user position
        let position_index_opt = market.user_positions
            .iter()
            .position(|(pubkey, _)| *pubkey == liquidate_user);
        
        let position_index = position_index_opt.ok_or(ErrorCode::PositionNotFound)?;
            
        // Get asset parameters from registry
        let asset_id = &market.asset_id;
        let registry = &ctx.accounts.registry;
        
        let asset_opt = registry.supported_assets
            .iter()
            .find(|(id, _)| id == asset_id);
            
        let (maintenance_margin_ratio, liquidation_fee) = match asset_opt {
            Some((_, asset)) => (asset.maintenance_margin_ratio, asset.liquidation_fee),
            None => return Err(ErrorCode::AssetNotAvailable.into()),
        };
        
        // Check position before moving it
        {
            let (_, position) = &market.user_positions[position_index];
            
            // Calculate position value and required margin
            let position_value = position.notional_value(oracle_price);
            let required_margin = position_value * maintenance_margin_ratio as u64 / 10000;
            
            // Check if position is liquidatable
            require!(
                position.is_liquidatable(oracle_price, maintenance_margin_ratio),
                ErrorCode::PositionNotLiquidatable
            );
        }
        
        // Get position details for liquidation
        let position = &market.user_positions[position_index].1;
        let position_side = position.side;
        let position_size = position.size;
        let position_value = position.notional_value(oracle_price);
        let maintenance_margin = position_value * maintenance_margin_ratio as u64 / 10000;
            
        // Calculate liquidation fee
        let fee_amount = position_value * liquidation_fee as u64 / 10000;
        
        // Calculate remaining margin after liquidation
        let remaining = if position.margin > fee_amount {
            position.margin - fee_amount
        } else {
            0
        };
        
        // Transfer liquidation fee to liquidator
        if fee_amount > 0 {
            // Create PDA signer seeds
            let market_key = market.key();
            let seeds = &[
                b"vault_signer".as_ref(),
                market_key.as_ref(),
                &[market.vault_signer_bump],
            ];
            let signer = &[&seeds[..]];
            
            token::transfer(
                CpiContext::new_with_signer(
                    ctx.accounts.token_program.to_account_info(),
                    Transfer {
                        from: ctx.accounts.quote_vault.to_account_info(),
                        to: ctx.accounts.liquidator_quote_account.to_account_info(),
                        authority: ctx.accounts.vault_signer.to_account_info(),
                    },
                    signer,
                ),
                fee_amount,
            )?;
        }
        
        // Transfer remaining collateral to the user
        if remaining > 0 {
            // Create PDA signer seeds
            let market_key = market.key();
            let seeds = &[
                b"vault_signer".as_ref(),
                market_key.as_ref(),
                &[market.vault_signer_bump],
            ];
            let signer = &[&seeds[..]];
            
            token::transfer(
                CpiContext::new_with_signer(
                    ctx.accounts.token_program.to_account_info(),
                    Transfer {
                        from: ctx.accounts.quote_vault.to_account_info(),
                        to: ctx.accounts.user_quote_account.to_account_info(),
                        authority: ctx.accounts.vault_signer.to_account_info(),
                    },
                    signer,
                ),
                remaining,
            )?;
        }
        
        // Update market open interest
        match position_side {
            Side::Bid => market.open_interest_long -= position_size,
            Side::Ask => market.open_interest_short -= position_size,
        }
        
        // Remove the position
        market.user_positions.remove(position_index);
        
        emit!(PositionLiquidated {
            market: market.key(),
            user: liquidate_user,
            liquidator: liquidator_key,
            side: position_side,
            size: position_size,
            position_value,
            maintenance_margin,
            liquidation_fee: fee_amount,
            remaining,
            oracle_price,
            timestamp,
        });
        
        Ok(())
    }

    pub fn deposit_collateral(
        ctx: Context<ManageCollateral>,
        amount: u64
    ) -> Result<()> {
        let market = &mut ctx.accounts.market;
        let user_key = ctx.accounts.user.key();
        let timestamp = Clock::get()?.unix_timestamp as u64;
        
        // Verify this is a perpetual market
        require!(market.is_perpetual, ErrorCode::NotPerpetualMarket);
        
        // Verify amount is positive
        require!(amount > 0, ErrorCode::InvalidParameters);
        
        // Transfer tokens from user to vault
        token::transfer(
            CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                Transfer {
                    from: ctx.accounts.user_quote_account.to_account_info(),
                    to: ctx.accounts.quote_vault.to_account_info(),
                    authority: ctx.accounts.user.to_account_info(),
                },
            ),
            amount,
        )?;
        
        // Find user position or create new one
        let position_index = market.user_positions
            .iter()
            .position(|(pubkey, _)| *pubkey == user_key);
        
        if let Some(idx) = position_index {
            // Update existing position
            market.user_positions[idx].1.margin += amount;
            let total_margin = market.user_positions[idx].1.margin;
            
            // Update liquidation price if position is active
            if market.user_positions[idx].1.size > 0 {
                if let Some(pyth_account) = &ctx.accounts.pyth_price_feed {
                    // Get oracle price
                 let oracle_price = get_pyth_price(pyth_account, &ctx.accounts.market)?;
                    
                    // Get maintenance margin ratio from registry
                    let asset_id = &market.asset_id;
                    let asset_opt = ctx.accounts.registry.supported_assets
                        .iter()
                        .find(|(id, _)| id == asset_id);
                        
                    let maintenance_margin_ratio = match asset_opt {
                        Some((_, asset)) => asset.maintenance_margin_ratio,
                        None => return Err(ErrorCode::AssetNotAvailable.into()),
                    };
                    
                    market.user_positions[idx].1.update_liquidation_price(maintenance_margin_ratio);
                }
            }
            
            emit!(CollateralDeposited {
                market: market.key(),
                user: user_key,
                amount,
                total_margin,
                timestamp,
            });
        } else {
            // Create new position with default values
            let side = Side::Bid; // Default side, will be set properly when a trade occurs
            let new_position = Position::new(side, amount);
            
            market.user_positions.push((user_key, new_position));
            
            emit!(CollateralDeposited {
                market: market.key(),
                user: user_key,
                amount,
                total_margin: amount,
                timestamp,
            });
        }
        
        Ok(())
    }
    pub fn withdraw_collateral(
        ctx: Context<ManageCollateral>,
        amount: u64
    ) -> Result<()> {
      let market = &mut ctx.accounts.market;
        let user_key = ctx.accounts.user.key();
        let timestamp = Clock::get()?.unix_timestamp as u64;
        
        // Verify this is a perpetual market
        require!(market.is_perpetual, ErrorCode::NotPerpetualMarket);
        
        // Find user position
        let position_index_opt = market.user_positions
            .iter()
            .position(|(pubkey, _)| *pubkey == user_key);
        
        let position_index = position_index_opt.ok_or(ErrorCode::PositionNotFound)?;
        
        // Prepare an immutable reference to market for use in get_pyth_price
        let market_immut_ref = &*market;
        
        // Check if withdrawal is possible
        {
            let position = &market.user_positions[position_index].1;
            require!(position.margin >= amount, ErrorCode::InsufficientMargin);
            
            // If position is active, check if withdrawal would trigger liquidation
            if position.size > 0 {
                // Check oracle price
                if let Some(pyth_account) = &ctx.accounts.pyth_price_feed {
                    let oracle_price = get_pyth_price(pyth_account, market_immut_ref)?;
                    
                    // Get asset parameters from registry
                    let asset_id = &market.asset_id;
                    let asset_opt = ctx.accounts.registry.supported_assets
                        .iter()
                        .find(|(id, _)| id == asset_id);
                        
                    let maintenance_margin_ratio = match asset_opt {
                        Some((_, asset)) => asset.maintenance_margin_ratio,
                        None => return Err(ErrorCode::AssetNotAvailable.into()),
                    };
                    
                    // Calculate position value and required margin
                    let position_value = position.notional_value(oracle_price);
                    let required_margin = position_value * maintenance_margin_ratio as u64 / 10000;
                    
                    // Ensure remaining margin is sufficient
                    require!(
                        position.margin - amount >= required_margin,
                        ErrorCode::WithdrawalWouldTriggerLiquidation
                    );
                } else {
                    return Err(ErrorCode::InvalidPriceFeed.into());
                }
            }
        }
        
        // Update position
        let position = &mut market.user_positions[position_index].1;
        position.margin -= amount;
        
        // Update liquidation price
        if position.size > 0 {
            // Get maintenance margin ratio
            let asset_id = &market.asset_id;
            let asset_opt = ctx.accounts.registry.supported_assets
                .iter()
                .find(|(id, _)| id == asset_id);
                
            let maintenance_margin_ratio = match asset_opt {
                Some((_, asset)) => asset.maintenance_margin_ratio,
                None => return Err(ErrorCode::AssetNotAvailable.into()),
            };
            
            position.update_liquidation_price(maintenance_margin_ratio);
        }
        
        // Create PDA signer seeds for transfer
        let market_key = market.key();
        let seeds = &[
            b"vault_signer".as_ref(),
            market_key.as_ref(),
            &[market.vault_signer_bump],
        ];
        let signer = &[&seeds[..]];
        
        // Transfer tokens from vault to user
        token::transfer(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                Transfer {
                    from: ctx.accounts.quote_vault.to_account_info(),
                    to: ctx.accounts.user_quote_account.to_account_info(),
                    authority: ctx.accounts.vault_signer.to_account_info(),
                },
                signer,
            ),
            amount,
        )?;
        
        let remaining_margin = position.margin;
        
        emit!(CollateralWithdrawn {
            market: market.key(),
            user: user_key,
            amount,
            remaining_margin,
            timestamp,
        });
        
        // If position is empty (no size) and no margin left, remove the position
        if position.size == 0 && position.margin == 0 {
            market.user_positions.remove(position_index);
        }
        
        Ok(())
    }

    // Additional utility instruction to cancel all orders for a user
    pub fn cancel_all_orders(ctx: Context<CancelOrder>) -> Result<()> {
        let market = &ctx.accounts.market;
        let orderbook = &mut ctx.accounts.orderbook;
        let user_key = ctx.accounts.user.key();
        let timestamp = Clock::get()?.unix_timestamp as u64;
        
        // Find all orders for this user
        let user_orders = orderbook.find_orders_for_user(&user_key);
        
        // No orders found
        if user_orders.is_empty() {
            return Ok(());
        }
        
        // Process bid cancellations
        let mut bid_indices = Vec::new();
        
        for (side, price, price_idx, order_idx) in user_orders.iter().filter(|(s, _, _, _)| *s == Side::Bid) {
            // Get order details before removal
            let order = &orderbook.bids[*price_idx].1[*order_idx];
            let order_id = order.id;
            let client_id = order.client_id;
            let remaining_size = order.remaining_size;
            let reduce_only = order.reduce_only;
            
            // Emit cancel event
            emit!(OrderCancelled {
                market: market.key(),
                order_id,
                client_id,
                user: user_key,
                side: *side,
                price: *price,
                remaining_size,
                reduce_only,
                timestamp,
            });
            
            // Store indices for later removal
            bid_indices.push((*price_idx, *order_idx));
        }
        
        // Remove bid orders from highest to lowest index
        bid_indices.sort_by(|a, b| b.cmp(a));
        
        for (price_idx, order_idx) in bid_indices {
            orderbook.bids[price_idx].1.remove(order_idx);
            
            // If price level is empty, remove it
            if orderbook.bids[price_idx].1.is_empty() {
                orderbook.bids.remove(price_idx);
            }
        }
        
        // Process ask cancellations
        let mut ask_indices = Vec::new();
        
        for (side, price, price_idx, order_idx) in user_orders.iter().filter(|(s, _, _, _)| *s == Side::Ask) {
            // Get order details before removal
            let order = &orderbook.asks[*price_idx].1[*order_idx];
            let order_id = order.id;
            let client_id = order.client_id;
            let remaining_size = order.remaining_size;
            let reduce_only = order.reduce_only;
            
            // Emit cancel event
            emit!(OrderCancelled {
                market: market.key(),
                order_id,
                client_id,
                user: user_key,
                side: *side,
                price: *price,
                remaining_size,
                reduce_only,
                timestamp,
            });
            
            // Store indices for later removal
            ask_indices.push((*price_idx, *order_idx));
        }
        
        // Remove ask orders from highest to lowest index
        ask_indices.sort_by(|a, b| b.cmp(a));
        
        for (price_idx, order_idx) in ask_indices {
            orderbook.asks[price_idx].1.remove(order_idx);
            
            // If price level is empty, remove it
            if orderbook.asks[price_idx].1.is_empty() {
                orderbook.asks.remove(price_idx);
            }
        }
        
        // For spot markets, return locked tokens
        if !market.is_perpetual {
            // Token handling would go here in a complete implementation
        }
        
        Ok(())
    }
    