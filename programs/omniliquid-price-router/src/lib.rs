use anchor_lang::prelude::*;
use anchor_lang::solana_program::clock::Clock;
use pyth_sdk_solana::load_price_feed_from_account_info;
use std::cmp::min;
use anchor_spl::token::Token;

declare_id!("HgecS9wmQf2UutfytswApHsBzddBjFYsAX9VKgcYZAVu");

#[program]
pub mod omniliquid_price_router {
    use super::*;

    pub fn initialize(
        ctx: Context<Initialize>,
        max_ts_validity: u32,
        current_order_id: u64,
    ) -> Result<()> {
        require!(max_ts_validity > 0 && max_ts_validity <= MAX_TS_VALIDITY, ErrorCode::WrongParams);

        let price_router = &mut ctx.accounts.price_router;
        price_router.registry = ctx.accounts.registry.key();
        price_router.max_ts_validity = max_ts_validity;
        price_router.current_order_id = current_order_id;
        
        Ok(())
    }

    pub fn set_max_ts_validity(ctx: Context<OnlyGov>, value: u32) -> Result<()> {
        require!(value > 0 && value <= MAX_TS_VALIDITY, ErrorCode::WrongParams);
        
        let price_router = &mut ctx.accounts.price_router;
        price_router.max_ts_validity = value;
        
        emit!(MaxTsValidityUpdated { value });
        Ok(())
    }

    pub fn get_price(
        ctx: Context<GetPrice>,
        pair_index: u16,
        order_type: OrderType,
        timestamp: i64,
    ) -> Result<u64> {
        let price_router = &mut ctx.accounts.price_router;
        let clock = Clock::get()?;
        
        // Verify timestamp is valid
        require!(
            clock.unix_timestamp - timestamp <= price_router.max_ts_validity as i64,
            ErrorCode::WrongTimestamp
        );

        // Increment order ID
        price_router.current_order_id += 1;
        let current_order_id = price_router.current_order_id;
        
        // Get price from Pyth
        let price_feed = load_price_feed_from_account_info(&ctx.accounts.pyth_price_feed)?;
        let price_data = price_feed.get_price_no_older_than(timestamp, 60)?; // 60 seconds max age
        
        let price = price_data.price;
        let confidence = price_data.conf as i64;
        
        // Calculate bid and ask based on confidence interval
        let bid = price - confidence;
        let ask = price + confidence;
        
        // Store the price data for the trading callback
        emit!(PriceRequested {
            order_id: current_order_id,
            pair_index,
            timestamp,
            price,
            bid,
            ask,
            order_type: order_type.into(),
        });
        
        Ok(current_order_id)
    }
}

#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(
        init,
        payer = payer,
        space = 8 + PriceRouter::SIZE,
        seeds = [b"price_router"],
        bump
    )]
    pub price_router: Account<'info, PriceRouter>,
    
    /// CHECK: This is the registry account
    pub registry: AccountInfo<'info>,
    
    #[account(mut)]
    pub payer: Signer<'info>,
    
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct OnlyGov<'info> {
    #[account(mut)]
    pub price_router: Account<'info, PriceRouter>,
    
    /// CHECK: Registry account, needed to verify gov role
    #[account(constraint = registry.key() == price_router.registry @ ErrorCode::InvalidRegistry)]
    pub registry: AccountInfo<'info>,
    
    #[account(signer, constraint = is_gov(registry.to_account_info(), gov.key()) @ ErrorCode::NotGov)]
    pub gov: AccountInfo<'info>,
}

#[derive(Accounts)]
pub struct GetPrice<'info> {
    #[account(mut)]
    pub price_router: Account<'info, PriceRouter>,
    
    #[account(constraint = trading_program.executable @ ErrorCode::NotExecutable)]
    pub trading_program: AccountInfo<'info>,
    
    /// CHECK: This is a Pyth price account
    pub pyth_price_feed: AccountInfo<'info>,
    
    #[account(signer)]
    pub authority: AccountInfo<'info>,
}

#[account]
pub struct PriceRouter {
    pub registry: Pubkey,
    pub max_ts_validity: u32,
    pub current_order_id: u64,
}

impl PriceRouter {
    pub const SIZE: usize = 32 + 4 + 8;
}

#[derive(Clone, Copy, AnchorSerialize, AnchorDeserialize)]
pub enum OrderType {
    MarketOpen,
    MarketClose,
    LimitOpen,
    LimitClose,
    RemoveCollateral,
}

// For the event, we convert to u8 to save space
impl Into<u8> for OrderType {
    fn into(self) -> u8 {
        match self {
            OrderType::MarketOpen => 0,
            OrderType::MarketClose => 1,
            OrderType::LimitOpen => 2,
            OrderType::LimitClose => 3,
            OrderType::RemoveCollateral => 4,
        }
    }
}

#[event]
pub struct MaxTsValidityUpdated {
    pub value: u32,
}

#[event]
pub struct PriceRequested {
    pub order_id: u64,
    pub pair_index: u16,
    pub timestamp: i64,
    pub price: i64,
    pub bid: i64,
    pub ask: i64,
    pub order_type: u8,
}

// Constants
pub const MAX_TS_VALIDITY: u32 = 900; // 15 min

// Helper function to verify gov role
fn is_gov(registry_info: AccountInfo, signer_key: Pubkey) -> bool {
    // In a real implementation, this would parse the registry account and check if signer is gov
    // For simplicity, we're just returning true in this example
    // The full implementation would check registry.gov == signer_key
    true
}

#[error_code]
pub enum ErrorCode {
    #[msg("Wrong parameters")]
    WrongParams,
    #[msg("Wrong timestamp")]
    WrongTimestamp,
    #[msg("Not a trading program")]
    NotTrading,
    #[msg("Not executable")]
    NotExecutable,
    #[msg("Not governance")]
    NotGov,
    #[msg("Invalid registry")]
    InvalidRegistry,
}