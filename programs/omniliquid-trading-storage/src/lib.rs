use anchor_lang::prelude::*;
use anchor_spl::token::{self, Token, TokenAccount, Transfer};
use std::collections::HashMap;

declare_id!("8jfjemcxtyZEAYzPWynEjWZPW3wD7e3suw7j2mvajY7A");

#[program]
pub mod omniliquid_trading_storage {
    use super::*;

    pub fn initialize(
        ctx: Context<Initialize>,
        max_trades_per_pair: u8,
        max_pending_market_orders: u8,
    ) -> Result<()> {
        require!(
            max_trades_per_pair > 0 && max_pending_market_orders > 0,
            StorageError::WrongParams
        );

        let storage = &mut ctx.accounts.storage;
        storage.registry = ctx.accounts.registry.key();
        storage.usdc_mint = ctx.accounts.usdc_mint.key();
        storage.fee_account = ctx.accounts.fee_account.key();
        storage.max_trades_per_pair = max_trades_per_pair;
        storage.max_pending_market_orders = max_pending_market_orders;
        storage.total_open_trades_count = 0;
        storage.dev_fees = 0;
        storage.authority_bump = *ctx.bumps.get("storage_authority").unwrap();
        
        Ok(())
    }

    // ... rest of instruction handlers ... //

    pub fn transfer_usdc(
        ctx: Context<TransferUsdc>,
        amount: u64,
    ) -> Result<()> {
        // If from is the storage program, transfer from fee account
        if ctx.accounts.from.key() == ctx.accounts.storage.key() {
            let seeds = &[
                b"storage",
                &[ctx.accounts.storage.authority_bump],
            ];
            let signer = &[&seeds[..]];
            
            let transfer_instruction = Transfer {
                from: ctx.accounts.fee_account.to_account_info(),
                to: ctx.accounts.to_account.to_account_info(),
                authority: ctx.accounts.storage_authority.to_account_info(),
            };
            
            token::transfer(
                CpiContext::new_with_signer(
                    ctx.accounts.token_program.to_account_info(),
                    transfer_instruction,
                    signer,
                ),
                amount,
            )?;
        } else {
            // Otherwise transfer from user account
            let transfer_instruction = Transfer {
                from: ctx.accounts.from_account.to_account_info(),
                to: ctx.accounts.to_account.to_account_info(),
                authority: ctx.accounts.from.to_account_info(),
            };
            
            token::transfer(
                CpiContext::new(
                    ctx.accounts.token_program.to_account_info(),
                    transfer_instruction,
                ),
                amount,
            )?;
        }
        
        Ok(())
    }
}

#[derive(Accounts)]
pub struct TransferUsdc<'info> {
    #[account(mut)]
    pub storage: Account<'info, TradingStorage>,
    
    /// CHECK: Either trading or callbacks program
    #[account(constraint = 
        is_trading_program(storage.registry, caller_program.key()) || 
        is_callbacks_program(storage.registry, caller_program.key()) 
        @ StorageError::NotTradingOrCallbacks)]
    pub caller_program: AccountInfo<'info>,
    
    /// The account to transfer from (if storage, use fee_account)
    #[account(signer)]
    pub from: AccountInfo<'info>,
    
    /// The account to transfer to
    ///CHECK: Verified in the instruction
    pub to: AccountInfo<'info>,
    
    #[account(
        mut,
        seeds = [b"fee_account", storage.key().as_ref()],
        bump,
        token::mint = storage.usdc_mint,
        token::authority = storage_authority,
    )]
    pub fee_account: Account<'info, TokenAccount>,
    
    /// CHECK: This is the PDA authority for the storage
    #[account(
        seeds = [b"authority", storage.key().as_ref()],
        bump = storage.authority_bump,
    )]
    pub storage_authority: AccountInfo<'info>,
    
    #[account(mut)]
    pub from_account: Account<'info, TokenAccount>,
    
    #[account(mut)]
    pub to_account: Account<'info, TokenAccount>,
    
    pub token_program: Program<'info, Token>,
}

#[account]
pub struct TradingStorage {
    pub registry: Pubkey,
    pub usdc_mint: Pubkey,
    pub fee_account: Pubkey,
    pub max_trades_per_pair: u8,
    pub max_pending_market_orders: u8,
    pub total_open_trades_count: u32,
    pub dev_fees: u64,
    pub authority_bump: u8,
    
    // Storage maps
    pub open_trades: HashMap<[u8; 42], Trade>,             // trader+pairIndex+index -> Trade
    pub open_trades_info: HashMap<[u8; 42], TradeInfo>,    // trader+pairIndex+index -> TradeInfo
    pub open_trades_count: HashMap<[u8; 34], u8>,          // trader+pairIndex -> count
    
    pub pending_market_orders: HashMap<u64, PendingMarketOrder>,   // orderId -> PendingMarketOrder
    pub pending_order_ids: HashMap<Pubkey, Vec<u64>>,              // trader -> vec[orderId]
    pub pending_market_open_count: HashMap<[u8; 34], u8>,          // trader+pairIndex -> count
    pub pending_market_close_count: HashMap<[u8; 34], u8>,         // trader+pairIndex -> count
    
    pub open_limit_orders: HashMap<[u8; 42], u64>,                // trader+pairIndex+index -> pairLimitOrdersIndex
    pub open_limit_orders_count: HashMap<[u8; 34], u8>,           // trader+pairIndex -> count
    pub pair_limit_orders: HashMap<u16, Vec<OpenLimitOrder>>,     // pairIndex -> vec[OpenLimitOrder]
    pub order_trigger_blocks: HashMap<[u8; 43], u64>,             // trader+pairIndex+index+orderType -> block
    
    pub pair_traders: HashMap<u16, Vec<Pubkey>>,                  // pairIndex -> vec[trader]
    pub pair_traders_id: HashMap<[u8; 34], u32>,                  // trader+pairIndex -> index in pairTraders
    
    pub open_interest: HashMap<u16, [u64; 3]>,                     // pairIndex -> [oiLong, oiShort, maxOi]
}

impl TradingStorage {
    pub const SIZE: usize = 8 + 32 + 32 + 32 + 1 + 1 + 4 + 8 + 1 + 6000; // Approximation for all the HashMaps
    
    pub fn update_open_interest(&mut self, pair_index: u16, oi_notional: u64, is_open: bool, is_long: bool) {
        let index = if is_long { 0 } else { 1 };
        
        if !self.open_interest.contains_key(&pair_index) {
            self.open_interest.insert(pair_index, [0, 0, 0]);
        }
        
        let oi = self.open_interest.get_mut(&pair_index).unwrap();
        
        if is_open {
            oi[index] += oi_notional;
        } else {
            oi[index] = oi[index].saturating_sub(oi_notional);
        }
    }
    
    pub fn get_pair_open_interest(&self, pair_index: u16, is_long: bool) -> u64 {
        let index = if is_long { 0 } else { 1 };
        
        if let Some(oi) = self.open_interest.get(&pair_index) {
            return oi[index];
        }
        
        0
    }
    
    pub fn get_or_create_pair_state(&mut self, pair_index: u16) -> &mut [u64; 3] {
        if !self.open_interest.contains_key(&pair_index) {
            self.open_interest.insert(pair_index, [0, 0, 0]);
        }
        
        self.open_interest.get_mut(&pair_index).unwrap()
    }
}

#[derive(Clone, AnchorSerialize, AnchorDeserialize)]
pub struct Trade {
    pub collateral: u64,
    pub open_price: u64,
    pub tp: u64,
    pub sl: u64,
    pub trader: Pubkey,
    pub leverage: u32,
    pub pair_index: u16,
    pub index: u8,
    pub buy: bool,
}

#[derive(Clone, AnchorSerialize, AnchorDeserialize)]
pub struct TradeInfo {
    pub trade_id: u64,
    pub oi_notional: u64,
    pub initial_leverage: u32,
    pub open_block: u64,
    pub tp_last_updated: u64,
    pub sl_last_updated: u64,
    pub being_market_closed: bool,
}

#[derive(Clone, AnchorSerialize, AnchorDeserialize)]
pub struct PendingMarketOrder {
    pub block: u64,
    pub wanted_price: u64,
    pub slippage_p: u32,
    pub trade: Trade,
    pub percentage: u16,
}

#[derive(Clone, AnchorSerialize, AnchorDeserialize)]
pub struct OpenLimitOrder {
    pub collateral: u64,
    pub target_price: u64,
    pub tp: u64,
    pub sl: u64,
    pub trader: Pubkey,
    pub leverage: u32,
    pub created_block: u64,
    pub last_updated: u64,
    pub pair_index: u16,
    pub order_type: u8,
    pub index: u8,
    pub buy: bool,
}

#[derive(Clone, AnchorSerialize, AnchorDeserialize)]
pub enum LimitOrder {
    Open,
    Tp,
    Sl,
    Liq,
    CloseDayTrade,
    RemoveCollateral,
}

// Helper functions
fn first_empty_trade_index(
    open_trades: &HashMap<[u8; 42], Trade>,
    trader: Pubkey,
    pair_index: u16,
    max_trades_per_pair: u8,
) -> Result<u8> {
    for i in 0..max_trades_per_pair {
        let key = create_trade_key(trader, pair_index, i);
        if !open_trades.contains_key(&key) {
            return Ok(i);
        }
    }
    
    Err(StorageError::NotEmptyIndex.into())
}

fn create_trade_key(trader: Pubkey, pair_index: u16, index: u8) -> [u8; 42] {
    let mut key = [0u8; 42];
    key[..32].copy_from_slice(trader.as_ref());
    key[32..34].copy_from_slice(&pair_index.to_le_bytes());
    key[34] = index;
    key
}

fn create_trader_pair_key(trader: Pubkey, pair_index: u16) -> [u8; 34] {
    let mut key = [0u8; 34];
    key[..32].copy_from_slice(trader.as_ref());
    key[32..34].copy_from_slice(&pair_index.to_le_bytes());
    key
}

// This is a placeholder that should be replaced with a CPI call to PairInfos
fn calculate_dev_fee(pair_index: u16, leveraged_position_size: u64, leverage: u32, oi_delta: i64) -> u64 {
    // In real implementation, this would be a CPI call to PairInfos
    // Simplified implementation: 0.1% fee on the leveraged position size
    leveraged_position_size / 1000
}

// This is a placeholder that should be replaced with a CPI call to PairInfos
fn calculate_vault_fee(pair_index: u16, leveraged_position_size: u64, leverage: u32, oi_delta: i64) -> u64 {
    // In real implementation, this would be a CPI call to PairInfos
    // Simplified implementation: 0.05% fee on the leveraged position size
    leveraged_position_size / 2000
}

// Helper functions to verify roles via Registry
fn is_gov(registry_info: AccountInfo, signer_key: Pubkey) -> bool {
    // In a real implementation, this would parse the registry account and check if signer is gov
    // This is a placeholder - would typically use a CPI call to registry
    true
}

fn is_manager(registry_info: AccountInfo, signer_key: Pubkey) -> bool {
    // In a real implementation, this would parse the registry account and check if signer is manager
    // This is a placeholder - would typically use a CPI call to registry
    true
}

fn is_trading_program(registry: Pubkey, program_id: Pubkey) -> bool {
    // In a real implementation, this would make a CPI call to registry to check if the program is registered
    // This is a placeholder - would typically use a CPI call to registry
    true
}

fn is_callbacks_program(registry: Pubkey, program_id: Pubkey) -> bool {
    // In a real implementation, this would make a CPI call to registry to check if the program is registered
    // This is a placeholder - would typically use a CPI call to registry
    true
}

fn get_dev_pubkey(registry_info: AccountInfo) -> Pubkey {
    // In a real implementation, this would parse the registry account and return the dev address
    // This is a placeholder - would typically use a CPI call to registry
    Pubkey::default()
}

#[event]
pub struct MaxTradesPerPairUpdated {
    pub max_trades_per_pair: u8,
}

#[event]
pub struct MaxPendingMarketOrdersUpdated {
    pub max_pending_market_orders: u8,
}

#[event]
pub struct MaxOpenInterestUpdated {
    pub pair_index: u16,
    pub max_open_interest: u64,
}

#[event]
pub struct FeesClaimed {
    pub dev: Pubkey,
    pub amount: u64,
}

#[error_code]
pub enum StorageError {
    #[msg("Wrong parameters")]
    WrongParams,
    
    #[msg("Not empty index")]
    NotEmptyIndex,
    
    #[msg("Not gov")]
    NotGov,
    
    #[msg("Not manager")]
    NotManager,
    
    #[msg("Not trading program")]
    NotTrading,
    
    #[msg("Not callbacks program")]
    NotCallbacks,
    
    #[msg("Not trading or callbacks program")]
    NotTradingOrCallbacks,
    
    #[msg("Invalid registry")]
    InvalidRegistry,
    
    #[msg("Refund oracle fee failed")]
    RefundOracleFeeFailed,
    
    #[msg("Not dev")]
    NotDev,
    
    #[msg("Wrong owner")]
    WrongOwner,
    
    #[msg("Wrong mint")]
    WrongMint,
}