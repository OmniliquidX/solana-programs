use anchor_lang::prelude::*;
use anchor_spl::token::{self, Token, TokenAccount, Mint, Transfer, MintTo, Burn};
use std::convert::TryFrom;
use std::collections::HashMap;

declare_id!("6zLE2d1m87joeG1te75Qu19y2Y7irdHkPzToWpxkRjnL");

#[program]
pub mod omniliquid_olp_vault {
    use super::*;

    pub fn initialize(
        ctx: Context<Initialize>,
        max_acc_open_pnl_delta_per_token: u64,
        max_daily_acc_pnl_delta_per_token: u64,
        max_supply_increase_daily_p: u16,
        max_discount_p: u16,
        max_discount_threshold_p: u16,
        withdraw_lock_thresholds_p: [u16; 2],
    ) -> Result<()> {
        require!(
            max_daily_acc_pnl_delta_per_token >= MIN_DAILY_ACC_PNL_DELTA &&
            max_discount_threshold_p > 100 * PRECISION_2 &&
            withdraw_lock_thresholds_p[1] > withdraw_lock_thresholds_p[0] &&
            max_supply_increase_daily_p <= MAX_SUPPLY_INCREASE_DAILY_P &&
            max_discount_p <= MAX_DISCOUNT_P,
            VaultError::InvalidParameters
        );

        let vault = &mut ctx.accounts.vault;
        let clock = Clock::get()?;
        
        vault.registry = ctx.accounts.registry.key();
        vault.usdc_mint = ctx.accounts.usdc_mint.key();
        vault.lp_mint = ctx.accounts.lp_mint.key();
        vault.treasury_account = ctx.accounts.treasury_account.key();
        
        vault.max_acc_open_pnl_delta_per_token = max_acc_open_pnl_delta_per_token;
        vault.max_daily_acc_pnl_delta_per_token = max_daily_acc_pnl_delta_per_token;
        vault.max_supply_increase_daily_p = max_supply_increase_daily_p;
        vault.max_discount_p = max_discount_p;
        vault.max_discount_threshold_p = max_discount_threshold_p;
        vault.withdraw_lock_thresholds_p = withdraw_lock_thresholds_p;
        
        vault.current_epoch = 1;
        vault.share_to_assets_price = PRECISION_18;
        vault.current_epoch_start = clock.unix_timestamp as u32;
        vault.last_max_supply_update_ts = clock.unix_timestamp as u32;
        vault.last_daily_acc_pnl_delta_reset_ts = clock.unix_timestamp as u32;
        
        vault.acc_rewards_per_token = 0;
        vault.locked_deposits_count = 0;
        vault.acc_pnl_per_token = 0;
        vault.acc_pnl_per_token_used = 0;
        vault.daily_acc_pnl_delta_per_token = 0;
        
        vault.total_deposited = 0;
        vault.total_closed_pnl = 0;
        vault.total_rewards = 0;
        vault.total_liability = 0;
        vault.total_locked_discounts = 0;
        vault.total_discounts = 0;
        
        vault.authority_bump = *ctx.bumps.get("vault_authority").unwrap();
        
        // Set withdraw epochs locks - these values are hardcoded in the contract
        vault.withdraw_epochs_locks = [3, 2, 1];
        
        Ok(())
    }

    pub fn update_max_acc_open_pnl_delta_per_token(
        ctx: Context<OnlyGov>,
        new_value: u64,
    ) -> Result<()> {
        let vault = &mut ctx.accounts.vault;
        vault.max_acc_open_pnl_delta_per_token = new_value;
        
        emit!(MaxAccOpenPnlDeltaPerTokenUpdated { value: new_value });
        Ok(())
    }

    pub fn update_max_daily_acc_pnl_delta_per_token(
        ctx: Context<OnlyGov>,
        new_value: u64,
    ) -> Result<()> {
        require!(
            new_value >= MIN_DAILY_ACC_PNL_DELTA,
            VaultError::InvalidParameters
        );
        
        let vault = &mut ctx.accounts.vault;
        vault.max_daily_acc_pnl_delta_per_token = new_value;
        
        emit!(MaxDailyAccPnlDeltaPerTokenUpdated { value: new_value });
        Ok(())
    }

    pub fn update_withdraw_lock_thresholds_p(
        ctx: Context<OnlyGov>,
        new_value: [u16; 2],
    ) -> Result<()> {
        require!(
            new_value[1] > new_value[0],
            VaultError::InvalidParameters
        );
        
        let vault = &mut ctx.accounts.vault;
        vault.withdraw_lock_thresholds_p = new_value;
        
        emit!(WithdrawLockThresholdsPUpdated { value: new_value });
        Ok(())
    }

    pub fn update_max_supply_increase_daily_p(
        ctx: Context<OnlyGov>,
        new_value: u16,
    ) -> Result<()> {
        require!(
            new_value <= MAX_SUPPLY_INCREASE_DAILY_P,
            VaultError::InvalidParameters
        );
        
        let vault = &mut ctx.accounts.vault;
        vault.max_supply_increase_daily_p = new_value;
        
        emit!(MaxSupplyIncreaseDailyPUpdated { value: new_value });
        Ok(())
    }

    pub fn update_max_discount_p(
        ctx: Context<OnlyGov>,
        new_value: u16,
    ) -> Result<()> {
        require!(
            new_value <= MAX_DISCOUNT_P,
            VaultError::InvalidParameters
        );
        
        let vault = &mut ctx.accounts.vault;
        vault.max_discount_p = new_value;
        
        emit!(MaxDiscountPUpdated { value: new_value });
        Ok(())
    }

    pub fn update_max_discount_threshold_p(
        ctx: Context<OnlyGov>,
        new_value: u16,
    ) -> Result<()> {
        require!(
            new_value > 100 * PRECISION_2,
            VaultError::InvalidParameters
        );
        
        let vault = &mut ctx.accounts.vault;
        vault.max_discount_threshold_p = new_value;
        
        emit!(MaxDiscountThresholdPUpdated { value: new_value });
        Ok(())
    }

    pub fn deposit(
        ctx: Context<Deposit>,
        amount: u64,
    ) -> Result<()> {
        let vault = &mut ctx.accounts.vault;
        
        require!(amount > 0, VaultError::ZeroAmount);
        require!(vault.share_to_assets_price > 0, VaultError::ZeroPrice);
        
        // Try to update max supply
        try_update_current_max_supply(vault)?;
        
        // Calculate number of shares to mint
        let shares = convert_to_shares(amount, vault.share_to_assets_price)?;
        
        // Check max mint limit
        let current_supply = ctx.accounts.lp_mint.supply;
        if vault.acc_pnl_per_token > 0 {
            require!(
                current_supply.checked_add(shares).unwrap() <= vault.current_max_supply,
                VaultError::ExceedsMaxMint
            );
        }
        
        // Scale variables
        scale_variables(vault, shares, amount, true)?;
        
        // Transfer USDC from user to treasury
        let transfer_instruction = Transfer {
            from: ctx.accounts.user_usdc_account.to_account_info(),
            to: ctx.accounts.treasury_account.to_account_info(),
            authority: ctx.accounts.user.to_account_info(),
        };
        
        token::transfer(
            CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                transfer_instruction,
            ),
            amount,
        )?;
        
        // Mint LP tokens to user
        let mint_instruction = MintTo {
            mint: ctx.accounts.lp_mint.to_account_info(),
            to: ctx.accounts.user_lp_account.to_account_info(),
            authority: ctx.accounts.vault_authority.to_account_info(),
        };
        
        let vault_seeds = &[
            b"vault",
            &[vault.authority_bump],
        ];
        let signer = &[&vault_seeds[..]];
        
        token::mint_to(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                mint_instruction,
                signer,
            ),
            shares,
        )?;
        
        // Update total deposited amount
        vault.total_deposited = vault.total_deposited.checked_add(amount).unwrap();
        
        emit!(Deposited {
            user: ctx.accounts.user.key(),
            amount,
            shares,
        });
        
        Ok(())
    }

    pub fn redeem(
        ctx: Context<Redeem>,
        shares: u64,
    ) -> Result<()> {
        let vault = &mut ctx.accounts.vault;
        
        require!(shares > 0, VaultError::ZeroAmount);
        require!(vault.share_to_assets_price > 0, VaultError::ZeroPrice);
        
        // Make sure epoch values are updated
        try_new_open_pnl_request_or_epoch()?;
        
        // Check user has sufficient LP tokens
        require!(
            ctx.accounts.user_lp_account.amount >= shares,
            VaultError::InsufficientShares
        );
        
        // Check user has a valid withdraw request
        require!(
            vault.withdraw_requests.get(&(ctx.accounts.user.key(), vault.current_epoch)).unwrap_or(&0) >= &shares,
            VaultError::NoWithdrawRequest
        );
        
        // Calculate assets to return
        let assets = convert_to_assets(shares, vault.share_to_assets_price)?;
        
        // Scale variables
        scale_variables(vault, shares, assets, false)?;
        
        // Burn LP tokens from user
        let burn_instruction = Burn {
            mint: ctx.accounts.lp_mint.to_account_info(),
            from: ctx.accounts.user_lp_account.to_account_info(),
            authority: ctx.accounts.user.to_account_info(),
        };
        
        token::burn(
            CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                burn_instruction,
            ),
            shares,
        )?;
        
        // Transfer USDC to user
        let vault_seeds = &[
            b"vault",
            &[vault.authority_bump],
        ];
        let signer = &[&vault_seeds[..]];
        
        let transfer_instruction = Transfer {
            from: ctx.accounts.treasury_account.to_account_info(),
            to: ctx.accounts.user_usdc_account.to_account_info(),
            authority: ctx.accounts.vault_authority.to_account_info(),
        };
        
        token::transfer(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                transfer_instruction,
                signer,
            ),
            assets,
        )?;
        
        // Update withdraw request
        let key = (ctx.accounts.user.key(), vault.current_epoch);
        let current_request = vault.withdraw_requests.get(&key).unwrap_or(&0);
        vault.withdraw_requests.insert(key, current_request - shares);
        
        // Update total deposited amount
        vault.total_deposited = vault.total_deposited.checked_sub(assets).unwrap();
        
        emit!(Redeemed {
            user: ctx.accounts.user.key(),
            shares,
            assets,
        });
        
        Ok(())
    }

    pub fn make_withdraw_request(
        ctx: Context<MakeWithdrawRequest>,
        shares: u64,
    ) -> Result<()> {
        let vault = &mut ctx.accounts.vault;
        
        // Make sure epoch values are updated
        try_new_open_pnl_request_or_epoch()?;
        
        // Check if user has sufficient LP tokens
        require!(
            ctx.accounts.user_lp_account.amount >= shares,
            VaultError::InsufficientShares
        );
        
        // Calculate unlock epoch
        let unlock_epoch = vault.current_epoch + withdraw_epochs_timelock(vault)?;
        
        // Update withdraw request
        let key = (ctx.accounts.user.key(), unlock_epoch);
        let current_request = vault.withdraw_requests.get(&key).unwrap_or(&0);
        
        vault.withdraw_requests.insert(key, current_request + shares);
        
        emit!(WithdrawRequested {
            user: ctx.accounts.user.key(),
            shares,
            current_epoch: vault.current_epoch,
            unlock_epoch,
        });
        
        Ok(())
    }

    pub fn cancel_withdraw_request(
        ctx: Context<CancelWithdrawRequest>,
        shares: u64,
        unlock_epoch: u16,
    ) -> Result<()> {
        let vault = &mut ctx.accounts.vault;
        
        // Check if user has a withdraw request for the specified epoch
        let key = (ctx.accounts.user.key(), unlock_epoch);
        let current_request = vault.withdraw_requests.get(&key).unwrap_or(&0);
        
        require!(
            *current_request >= shares,
            VaultError::InsufficientWithdrawRequest
        );
        
        // Update withdraw request
        vault.withdraw_requests.insert(key, current_request - shares);
        
        emit!(WithdrawCanceled {
            user: ctx.accounts.user.key(),
            shares,
            current_epoch: vault.current_epoch,
            unlock_epoch,
        });
        
        Ok(())
    }

    pub fn deposit_with_discount_and_lock(
        ctx: Context<DepositWithLock>,
        assets: u64,
        lock_duration: u32,
    ) -> Result<()> {
        let vault = &mut ctx.accounts.vault;
        
        require!(assets > 0, VaultError::ZeroAmount);
        require!(vault.share_to_assets_price > 0, VaultError::ZeroPrice);
        require!(
            lock_duration >= MIN_LOCK_DURATION && lock_duration <= MAX_LOCK_DURATION,
            VaultError::InvalidLockDuration
        );
        require!(vault.max_discount_p > 0, VaultError::NoActiveDiscount);
        
        // Calculate discount
        let collat_p = collateralization_p(vault)?;
        let discount_p = lock_discount_p(vault, collat_p, lock_duration)?;
        
        require!(discount_p > 0, VaultError::NoDiscount);
        
        // Calculate simulated assets with discount
        let simulated_assets = assets
            .checked_mul(100 * PRECISION_2 + discount_p)
            .unwrap()
            .checked_div(100 * PRECISION_2)
            .unwrap();
        
        // Calculate shares
        let shares = convert_to_shares(simulated_assets, vault.share_to_assets_price)?;
        
        // Execute discount and lock
        execute_discount_and_lock(
            &ctx,
            simulated_assets,
            assets,
            shares,
            lock_duration,
        )?;
        
        Ok(())
    }

    pub fn mint_with_discount_and_lock(
        ctx: Context<DepositWithLock>,
        shares: u64,
        lock_duration: u32,
    ) -> Result<()> {
        let vault = &mut ctx.accounts.vault;
        
        require!(shares > 0, VaultError::ZeroAmount);
        require!(vault.share_to_assets_price > 0, VaultError::ZeroPrice);
        require!(
            lock_duration >= MIN_LOCK_DURATION && lock_duration <= MAX_LOCK_DURATION,
            VaultError::InvalidLockDuration
        );
        require!(vault.max_discount_p > 0, VaultError::NoActiveDiscount);
        
        // Check max mint limit
        let current_supply = ctx.accounts.lp_mint.supply;
        if vault.acc_pnl_per_token > 0 {
            require!(
                current_supply.checked_add(shares).unwrap() <= vault.current_max_supply,
                VaultError::ExceedsMaxMint
            );
        }
        
        // Calculate assets
        let assets = convert_to_assets(shares, vault.share_to_assets_price)?;
        
        // Calculate discount
        let collat_p = collateralization_p(vault)?;
        let discount_p = lock_discount_p(vault, collat_p, lock_duration)?;
        
        require!(discount_p > 0, VaultError::NoDiscount);
        
        // Calculate discounted assets
        let discounted_assets = assets
            .checked_mul(100 * PRECISION_2)
            .unwrap()
            .checked_div(100 * PRECISION_2 + discount_p)
            .unwrap();
        
        // Execute discount and lock
        execute_discount_and_lock(
            &ctx,
            assets,
            discounted_assets,
            shares,
            lock_duration,
        )?;
        
        Ok(())
    }

    pub fn unlock_deposit(
        ctx: Context<UnlockDeposit>,
        deposit_id: u64,
    ) -> Result<()> {
        let vault = &mut ctx.accounts.vault;
        let clock = Clock::get()?;
        
        // Check that deposit exists
        let locked_deposit = match vault.locked_deposits.get(&deposit_id) {
            Some(deposit) => deposit.clone(),
            None => return Err(VaultError::DepositNotFound.into()),
        };
        
        // Check that deposit belongs to the NFT owner
        require!(
            locked_deposit.owner == ctx.accounts.nft_owner.key(),
            VaultError::NotDepositOwner
        );
        
        // Check that lock duration has elapsed
        require!(
            clock.unix_timestamp as u32 >= locked_deposit.at_timestamp + locked_deposit.lock_duration,
            VaultError::DepositNotUnlocked
        );
        
        // Calculate PnL impact of unlocking
        let acc_pnl_delta = (locked_deposit.assets_discount as i64)
            .checked_mul(PRECISION_18 as i64)
            .unwrap()
            .checked_div(ctx.accounts.lp_mint.supply as i64)
            .unwrap();
        
        // Update acc_pnl_per_token
        vault.acc_pnl_per_token = vault.acc_pnl_per_token.checked_add(acc_pnl_delta).unwrap();
        
        // Check we have enough assets
        let max_acc_pnl_per_token = max_acc_pnl_per_token(vault)? as i64;
        require!(
            vault.acc_pnl_per_token <= max_acc_pnl_per_token,
            VaultError::NotEnoughAssets
        );
        
        // Update acc_pnl_per_token_used
        vault.acc_pnl_per_token_used = vault.acc_pnl_per_token_used.checked_add(acc_pnl_delta).unwrap();
        
        // Update share_to_assets_price
        update_share_to_assets_price(vault)?;
        
        // Update liability and locked discounts
        vault.total_liability = vault.total_liability.checked_add(locked_deposit.assets_discount as i64).unwrap();
        vault.total_locked_discounts = vault.total_locked_discounts.checked_sub(locked_deposit.assets_discount).unwrap();
        
        // Burn NFT
        // In a real implementation, this would be a CPI call to the NFT program
        
        // Transfer LP tokens to recipient
        let vault_seeds = &[
            b"vault",
            &[vault.authority_bump],
        ];
        let signer = &[&vault_seeds[..]];
        
        let transfer_instruction = Transfer {
            from: ctx.accounts.vault_lp_account.to_account_info(),
            to: ctx.accounts.recipient_lp_account.to_account_info(),
            authority: ctx.accounts.vault_authority.to_account_info(),
        };
        
        token::transfer(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                transfer_instruction,
                signer,
            ),
            locked_deposit.shares,
        )?;
        
        // Remove locked deposit
        vault.locked_deposits.remove(&deposit_id);
        
        emit!(DepositUnlocked {
            deposit_id,
            owner: locked_deposit.owner,
            recipient: ctx.accounts.recipient.key(),
            deposit: locked_deposit,
        });
        
        Ok(())
    }

    pub fn distribute_reward(
        ctx: Context<DistributeReward>,
        assets: u64,
    ) -> Result<()> {
        let vault = &mut ctx.accounts.vault;
        
        // Transfer USDC from caller to treasury
        let transfer_instruction = Transfer {
            from: ctx.accounts.caller_usdc_account.to_account_info(),
            to: ctx.accounts.treasury_account.to_account_info(),
            authority: ctx.accounts.caller.to_account_info(),
        };
        
        token::transfer(
            CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                transfer_instruction,
            ),
            assets,
        )?;
        
        // Update acc_rewards_per_token
        let lp_supply = ctx.accounts.lp_mint.supply;
        vault.acc_rewards_per_token = vault.acc_rewards_per_token
            .checked_add(assets.checked_mul(PRECISION_18).unwrap().checked_div(lp_supply).unwrap())
            .unwrap();
        
        // Update share_to_assets_price
        update_share_to_assets_price(vault)?;
        
        // Update total rewards
        vault.total_rewards = vault.total_rewards.checked_add(assets).unwrap();
        
        emit!(RewardDistributed {
            distributor: ctx.accounts.caller.key(),
            amount: assets,
            acc_rewards_per_token: vault.acc_rewards_per_token,
        });
        
        Ok(())
    }

    pub fn send_assets(
        ctx: Context<SendAssets>,
        assets: u64,
    ) -> Result<()> {
        let vault = &mut ctx.accounts.vault;
        
        // Calculate PnL impact
        let lp_supply = ctx.accounts.lp_mint.supply;
        let acc_pnl_delta = (assets as i64)
            .checked_mul(PRECISION_18 as i64)
            .unwrap()
            .checked_div(lp_supply as i64)
            .unwrap();
        
        // Update acc_pnl_per_token
        vault.acc_pnl_per_token = vault.acc_pnl_per_token.checked_add(acc_pnl_delta).unwrap();
        
        // Check we have enough assets
        let max_acc_pnl_per_token = max_acc_pnl_per_token(vault)? as i64;
        require!(
            vault.acc_pnl_per_token <= max_acc_pnl_per_token,
            VaultError::NotEnoughAssets
        );
        
        // Update daily PnL delta
        try_reset_daily_acc_pnl_delta(vault)?;
        vault.daily_acc_pnl_delta_per_token = vault.daily_acc_pnl_delta_per_token.checked_add(acc_pnl_delta).unwrap();
        
        // Check daily PnL delta limit
        require!(
            vault.daily_acc_pnl_delta_per_token <= vault.max_daily_acc_pnl_delta_per_token as i64,
            VaultError::MaxDailyPnlReached
        );
        
        // Update liability and closed PnL
        vault.total_liability = vault.total_liability.checked_add(assets as i64).unwrap();
        vault.total_closed_pnl = vault.total_closed_pnl.checked_add(assets as i64).unwrap();
        
        // Try to update current max supply and request new epoch
        try_new_open_pnl_request_or_epoch()?;
        try_update_current_max_supply(vault)?;
        
        // Transfer USDC to receiver
        let vault_seeds = &[
            b"vault",
            &[vault.authority_bump],
        ];
        let signer = &[&vault_seeds[..]];
        
        let transfer_instruction = Transfer {
            from: ctx.accounts.treasury_account.to_account_info(),
            to: ctx.accounts.receiver_usdc_account.to_account_info(),
            authority: ctx.accounts.vault_authority.to_account_info(),
        };
        
        token::transfer(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                transfer_instruction,
                signer,
            ),
            assets,
        )?;
        
        emit!(AssetsSent {
            sender: ctx.accounts.callbacks.key(),
            receiver: ctx.accounts.receiver.key(),
            amount: assets,
        });
        
        Ok(())
    }

    pub fn receive_assets(
        ctx: Context<ReceiveAssets>,
        assets: u64,
    ) -> Result<()> {
        let vault = &mut ctx.accounts.vault;
        
        // Transfer USDC from caller to treasury
        let transfer_instruction = Transfer {
            from: ctx.accounts.caller_usdc_account.to_account_info(),
            to: ctx.accounts.treasury_account.to_account_info(),
            authority: ctx.accounts.caller.to_account_info(),
        };
        
        token::transfer(
            CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                transfer_instruction,
            ),
            assets,
        )?;
        
        // Calculate PnL impact
        let lp_supply = ctx.accounts.lp_mint.supply;
        let acc_pnl_delta = -((assets as i64)
            .checked_mul(PRECISION_18 as i64)
            .unwrap()
            .checked_div(lp_supply as i64)
            .unwrap());
        
        // Update acc_pnl_per_token
        vault.acc_pnl_per_token = vault.acc_pnl_per_token.checked_add(acc_pnl_delta).unwrap();
        
        // Update daily PnL delta
        try_reset_daily_acc_pnl_delta(vault)?;
        vault.daily_acc_pnl_delta_per_token = vault.daily_acc_pnl_delta_per_token.checked_add(acc_pnl_delta).unwrap();
        
        // Update liability and closed PnL
        vault.total_liability = vault.total_liability.checked_sub(assets as i64).unwrap();
        vault.total_closed_pnl = vault.total_closed_pnl.checked_sub(assets as i64).unwrap();
        
        // Try to update current max supply and request new epoch
        try_new_open_pnl_request_or_epoch()?;
        try_update_current_max_supply(vault)?;
        
        emit!(AssetsReceived {
            sender: ctx.accounts.caller.key(),
            user: ctx.accounts.user.key(),
            amount: assets,
        });
        
        Ok(())
    }

    pub fn update_acc_pnl_per_token_used(
        ctx: Context<UpdateAccPnlPerTokenUsed>,
        prev_positive_open_pnl: u64,
        new_positive_open_pnl: u64,
    ) -> Result<u64> {
        let vault = &mut ctx.accounts.vault;
        
        let delta = (new_positive_open_pnl as i64).checked_sub(prev_positive_open_pnl as i64).unwrap();
        let lp_supply = ctx.accounts.lp_mint.supply;
        
        // Calculate max delta
        let max_acc_pnl_per_token = max_acc_pnl_per_token(vault)? as i64;
        let max_delta = std::cmp::min(
            ((max_acc_pnl_per_token - vault.acc_pnl_per_token) as u64)
                .checked_mul(lp_supply)
                .unwrap()
                .checked_div(PRECISION_6)
                .unwrap() as i64,
            (vault.max_acc_open_pnl_delta_per_token)
                .checked_mul(lp_supply)
                .unwrap()
                .checked_div(PRECISION_6)
                .unwrap() as i64
        );
        
        let adjusted_delta = std::cmp::min(delta, max_delta);
        
        // Update acc_pnl_per_token
        vault.acc_pnl_per_token = vault.acc_pnl_per_token
            .checked_add(adjusted_delta
                .checked_mul(PRECISION_6 as i64)
                .unwrap()
                .checked_div(lp_supply as i64)
                .unwrap())
            .unwrap();
        
        // Update acc_pnl_per_token_used
        vault.acc_pnl_per_token_used = vault.acc_pnl_per_token;
        
        // Update share_to_assets_price
        update_share_to_assets_price(vault)?;
        
        // Update epoch state
        vault.current_epoch += 1;
        vault.current_epoch_start = Clock::get()?.unix_timestamp as u32;
        vault.current_epoch_positive_open_pnl = (prev_positive_open_pnl as i64 + adjusted_delta) as u64;
        
        // Try to update current max supply
        try_update_current_max_supply(vault)?;
        
        emit!(AccPnlPerTokenUsedUpdated {
            updater: ctx.accounts.open_pnl.key(),
            current_epoch: vault.current_epoch,
            prev_positive_open_pnl,
            new_positive_open_pnl,
            current_epoch_positive_open_pnl: vault.current_epoch_positive_open_pnl,
            acc_pnl_per_token_used: vault.acc_pnl_per_token_used,
        });
        
        Ok(vault.current_epoch_positive_open_pnl)
    }
}



// Helper functions for Vault logic
// Helper functions for Vault logic
fn try_update_current_max_supply(vault: &mut Vault) -> Result<()> {
    let clock = Clock::get()?;
    
    if (clock.unix_timestamp as u32).checked_sub(vault.last_max_supply_update_ts).unwrap() >= 24 * 60 * 60 {
        vault.current_max_supply = vault.lp_mint_supply
            .checked_mul(100 * PRECISION_2 + vault.max_supply_increase_daily_p as u64)
            .unwrap()
            .checked_div(100 * PRECISION_2)
            .unwrap();
        
        vault.last_max_supply_update_ts = clock.unix_timestamp as u32;
        
        emit!(CurrentMaxSupplyUpdated {
            value: vault.current_max_supply,
        });
    }
    
    Ok(())
}

fn try_reset_daily_acc_pnl_delta(vault: &mut Vault) -> Result<()> {
    let clock = Clock::get()?;
    
    if (clock.unix_timestamp as u32).checked_sub(vault.last_daily_acc_pnl_delta_reset_ts).unwrap() >= 24 * 60 * 60 {
        vault.daily_acc_pnl_delta_per_token = 0;
        vault.last_daily_acc_pnl_delta_reset_ts = clock.unix_timestamp as u32;
        
        emit!(DailyAccPnlDeltaReset {});
    }
    
    Ok(())
}

fn try_new_open_pnl_request_or_epoch() -> Result<()> {
    // In a real implementation, this would be a CPI call to the OpenPnl program
    // This is a placeholder
    Ok(())
}

fn update_share_to_assets_price(vault: &mut Vault) -> Result<()> {
    let max_acc_pnl = max_acc_pnl_per_token(vault)? as i64;
    
    if vault.acc_pnl_per_token_used > 0 {
        vault.share_to_assets_price = (max_acc_pnl - vault.acc_pnl_per_token_used) as u64;
    } else {
        vault.share_to_assets_price = (max_acc_pnl + vault.acc_pnl_per_token_used.abs()) as u64;
    }
    
    emit!(ShareToAssetsPriceUpdated {
        value: vault.share_to_assets_price,
    });
    
    Ok(())
}

fn max_acc_pnl_per_token(vault: &Vault) -> Result<u64> {
    Ok(vault.acc_rewards_per_token + PRECISION_18)
}

fn collateralization_p(vault: &Vault) -> Result<u64> {
    let max_acc_pnl = max_acc_pnl_per_token(vault)?;
    
    if vault.acc_pnl_per_token_used > 0 {
        Ok((max_acc_pnl - vault.acc_pnl_per_token_used as u64)
            .checked_mul(100 * PRECISION_2)
            .unwrap()
            .checked_div(max_acc_pnl)
            .unwrap())
    } else {
        Ok((max_acc_pnl + vault.acc_pnl_per_token_used.abs() as u64)
            .checked_mul(100 * PRECISION_2)
            .unwrap()
            .checked_div(max_acc_pnl)
            .unwrap())
    }
}

fn withdraw_epochs_timelock(vault: &Vault) -> Result<u16> {
    let collat_p = collateralization_p(vault)?;
    let over_collat_p = collat_p.checked_sub(std::cmp::min(collat_p, 100 * PRECISION_2)).unwrap();
    
    if over_collat_p > vault.withdraw_lock_thresholds_p[1] as u64 {
        Ok(vault.withdraw_epochs_locks[2])
    } else if over_collat_p > vault.withdraw_lock_thresholds_p[0] as u64 {
        Ok(vault.withdraw_epochs_locks[1])
    } else {
        Ok(vault.withdraw_epochs_locks[0])
    }
}

fn lock_discount_p(vault: &Vault, collat_p: u64, lock_duration: u32) -> Result<u64> {
    let discount_p = if collat_p <= 100 * PRECISION_2 {
        vault.max_discount_p as u64
    } else if collat_p <= vault.max_discount_threshold_p as u64 {
        (vault.max_discount_p as u64)
            .checked_mul(vault.max_discount_threshold_p as u64 - collat_p)
            .unwrap()
            .checked_div(vault.max_discount_threshold_p as u64 - 100 * PRECISION_2)
            .unwrap()
    } else {
        0
    };
    
    Ok(discount_p
        .checked_mul(lock_duration as u64)
        .unwrap()
        .checked_div(MAX_LOCK_DURATION as u64)
        .unwrap())
}

fn convert_to_shares(assets: u64, share_to_assets_price: u64) -> Result<u64> {
    Ok(assets
        .checked_mul(PRECISION_18)
        .unwrap()
        .checked_div(share_to_assets_price)
        .unwrap())
}

fn convert_to_assets(shares: u64, share_to_assets_price: u64) -> Result<u64> {
    if shares == u64::MAX && share_to_assets_price >= PRECISION_18 {
        return Ok(shares);
    }
    
    Ok(shares
        .checked_mul(share_to_assets_price)
        .unwrap()
        .checked_div(PRECISION_18)
        .unwrap())
}

fn scale_variables(vault: &mut Vault, shares: u64, assets: u64, is_deposit: bool) -> Result<()> {
    // Adjust acc_pnl_per_token based on new supply
    if vault.acc_pnl_per_token < 0 {
        let supply = vault.lp_mint_supply;
        let new_supply = if is_deposit {
            supply.checked_add(shares).unwrap()
        } else {
            supply.checked_sub(shares).unwrap()
        };
        
        vault.acc_pnl_per_token = vault.acc_pnl_per_token
            .checked_mul(supply as i64)
            .unwrap()
            .checked_div(new_supply as i64)
            .unwrap();
    } else if vault.acc_pnl_per_token > 0 {
        let supply = vault.lp_mint_supply;
        let liability_delta = (shares as i64)
            .checked_mul(vault.total_liability)
            .unwrap()
            .checked_div(supply as i64)
            .unwrap();
        
        if is_deposit {
            vault.total_liability = vault.total_liability.checked_add(liability_delta).unwrap();
        } else {
            vault.total_liability = vault.total_liability.checked_sub(liability_delta).unwrap();
        }
    }
    
    // Update total deposited
    if is_deposit {
        vault.total_deposited = vault.total_deposited.checked_add(assets).unwrap();
    } else {
        vault.total_deposited = vault.total_deposited.checked_sub(assets).unwrap();
    }
    
    Ok(())
}

fn execute_discount_and_lock(
    ctx: &Context<DepositWithLock>,
    simulated_assets: u64,
    assets_deposited: u64,
    shares: u64,
    lock_duration: u32,
) -> Result<()> {
    require!(
        simulated_assets > assets_deposited,
        VaultError::NoDiscount
    );
    
    let vault = &mut ctx.accounts.vault;
    let assets_discount = simulated_assets.checked_sub(assets_deposited).unwrap();
    let clock = Clock::get()?;
    
    // Create locked deposit
    vault.locked_deposits_count += 1;
    let deposit_id = vault.locked_deposits_count;
    
    vault.locked_deposits.insert(deposit_id, LockedDeposit {
        owner: ctx.accounts.recipient.key(),
        shares,
        assets_deposited,
        assets_discount,
        at_timestamp: clock.unix_timestamp as u32,
        lock_duration,
    });
    
    // Scale variables for deposit
    scale_variables(vault, shares, assets_deposited, true)?;
    
    // Transfer USDC from user to treasury
    let transfer_instruction = Transfer {
        from: ctx.accounts.user_usdc_account.to_account_info(),
        to: ctx.accounts.treasury_account.to_account_info(),
        authority: ctx.accounts.user.to_account_info(),
    };
    
    token::transfer(
        CpiContext::new(
            ctx.accounts.token_program.to_account_info(),
            transfer_instruction,
        ),
        assets_deposited,
    )?;
    
    // Mint LP tokens to vault
    let mint_instruction = MintTo {
        mint: ctx.accounts.lp_mint.to_account_info(),
        to: ctx.accounts.vault_lp_account.to_account_info(),
        authority: ctx.accounts.vault_authority.to_account_info(),
    };
    
    let vault_seeds = &[
        b"vault",
        &[vault.authority_bump],
    ];
    let signer = &[&vault_seeds[..]];
    
    token::mint_to(
        CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            mint_instruction,
            signer,
        ),
        shares,
    )?;
    
    // Update total discounts
    vault.total_discounts = vault.total_discounts.checked_add(assets_discount).unwrap();
    vault.total_locked_discounts = vault.total_locked_discounts.checked_add(assets_discount).unwrap();
    
    // Mint NFT to recipient
    // In a real implementation, this would be a CPI call to mint an NFT
    // representing the locked deposit
    
    emit!(DepositLocked {
        deposit_id,
        user: ctx.accounts.user.key(),
        recipient: ctx.accounts.recipient.key(),
        deposit: vault.locked_deposits.get(&deposit_id).unwrap().clone(),
    });
    
    Ok(())
}

// Account structures
#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(
        init,
        payer = payer,
        space = 8 + Vault::SIZE,
        seeds = [b"vault"],
        bump
    )]
    pub vault: Account<'info, Vault>,
    
    /// CHECK: The registry account that holds governance information
    pub registry: AccountInfo<'info>,
    
    pub usdc_mint: Account<'info, Mint>,
    
    #[account(
        init,
        payer = payer,
        mint::decimals = 6,
        mint::authority = vault_authority,
    )]
    pub lp_mint: Account<'info, Mint>,
    
    #[account(
        init,
        payer = payer,
        token::mint = usdc_mint,
        token::authority = vault_authority,
        seeds = [b"treasury", vault.key().as_ref()],
        bump,
    )]
    pub treasury_account: Account<'info, TokenAccount>,
    
    #[account(
        init,
        payer = payer,
        token::mint = lp_mint,
        token::authority = vault_authority,
        seeds = [b"vault_lp", vault.key().as_ref()],
        bump,
    )]
    pub vault_lp_account: Account<'info, TokenAccount>,
    
    /// CHECK: This is the PDA that will be the authority for various operations
    #[account(
        seeds = [b"authority", vault.key().as_ref()],
        bump,
    )]
    pub vault_authority: AccountInfo<'info>,
    
    #[account(mut)]
    pub payer: Signer<'info>,
    
    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
    pub rent: Sysvar<'info, Rent>,
}

#[derive(Accounts)]
pub struct OnlyGov<'info> {
    #[account(mut)]
    pub vault: Account<'info, Vault>,
    
    /// CHECK: Registry account, needed to verify gov role
    #[account(constraint = registry.key() == vault.registry @ VaultError::InvalidRegistry)]
    pub registry: AccountInfo<'info>,
    
    #[account(signer, constraint = is_gov(registry.to_account_info(), gov.key()) @ VaultError::NotGov)]
    pub gov: AccountInfo<'info>,
}

#[derive(Accounts)]
pub struct Deposit<'info> {
    #[account(mut)]
    pub vault: Account<'info, Vault>,
    
    #[account(mut, constraint = lp_mint.key() == vault.lp_mint @ VaultError::InvalidMint)]
    pub lp_mint: Account<'info, Mint>,
    
    #[account(mut, constraint = treasury_account.key() == vault.treasury_account @ VaultError::InvalidTreasury)]
    pub treasury_account: Account<'info, TokenAccount>,
    
    #[account(mut)]
    pub user_usdc_account: Account<'info, TokenAccount>,
    
    #[account(mut)]
    pub user_lp_account: Account<'info, TokenAccount>,
    
    #[account(signer)]
    pub user: AccountInfo<'info>,
    
    /// CHECK: This is the vault authority PDA
    #[account(
        seeds = [b"authority", vault.key().as_ref()],
        bump = vault.authority_bump,
    )]
    pub vault_authority: AccountInfo<'info>,
    
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct Redeem<'info> {
    #[account(mut)]
    pub vault: Account<'info, Vault>,
    
    #[account(mut, constraint = lp_mint.key() == vault.lp_mint @ VaultError::InvalidMint)]
    pub lp_mint: Account<'info, Mint>,
    
    #[account(mut, constraint = treasury_account.key() == vault.treasury_account @ VaultError::InvalidTreasury)]
    pub treasury_account: Account<'info, TokenAccount>,
    
    #[account(mut)]
    pub user_usdc_account: Account<'info, TokenAccount>,
    
    #[account(mut)]
    pub user_lp_account: Account<'info, TokenAccount>,
    
    #[account(signer)]
    pub user: AccountInfo<'info>,
    
    /// CHECK: This is the vault authority PDA
    #[account(
        seeds = [b"authority", vault.key().as_ref()],
        bump = vault.authority_bump,
    )]
    pub vault_authority: AccountInfo<'info>,
    
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct MakeWithdrawRequest<'info> {
    #[account(mut)]
    pub vault: Account<'info, Vault>,
    
    #[account(constraint = lp_mint.key() == vault.lp_mint @ VaultError::InvalidMint)]
    pub lp_mint: Account<'info, Mint>,
    
    #[account(constraint = user_lp_account.mint == vault.lp_mint @ VaultError::InvalidMint)]
    pub user_lp_account: Account<'info, TokenAccount>,
    
    #[account(signer)]
    pub user: AccountInfo<'info>,
}

#[derive(Accounts)]
pub struct CancelWithdrawRequest<'info> {
    #[account(mut)]
    pub vault: Account<'info, Vault>,
    
    #[account(signer)]
    pub user: AccountInfo<'info>,
}

#[derive(Accounts)]
pub struct DepositWithLock<'info> {
    #[account(mut)]
    pub vault: Account<'info, Vault>,
    
    #[account(mut, constraint = lp_mint.key() == vault.lp_mint @ VaultError::InvalidMint)]
    pub lp_mint: Account<'info, Mint>,
    
    #[account(mut, constraint = treasury_account.key() == vault.treasury_account @ VaultError::InvalidTreasury)]
    pub treasury_account: Account<'info, TokenAccount>,
    
    #[account(mut)]
    pub user_usdc_account: Account<'info, TokenAccount>,
    
    #[account(
        mut,
        constraint = vault_lp_account.mint == vault.lp_mint @ VaultError::InvalidMint,
        seeds = [b"vault_lp", vault.key().as_ref()],
        bump,
    )]
    pub vault_lp_account: Account<'info, TokenAccount>,
    
    #[account(signer)]
    pub user: AccountInfo<'info>,
    
    /// CHECK: This is the recipient of the NFT
    pub recipient: AccountInfo<'info>,
    
    /// CHECK: This is the vault authority PDA
    #[account(
        seeds = [b"authority", vault.key().as_ref()],
        bump = vault.authority_bump,
    )]
    pub vault_authority: AccountInfo<'info>,
    
    /// CHECK: NFT mint account, would be used in real implementation
    pub nft_mint: AccountInfo<'info>,
    
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct UnlockDeposit<'info> {
    #[account(mut)]
    pub vault: Account<'info, Vault>,
    
    #[account(constraint = lp_mint.key() == vault.lp_mint @ VaultError::InvalidMint)]
    pub lp_mint: Account<'info, Mint>,
    
    #[account(
        mut,
        constraint = vault_lp_account.mint == vault.lp_mint @ VaultError::InvalidMint,
        seeds = [b"vault_lp", vault.key().as_ref()],
        bump,
    )]
    pub vault_lp_account: Account<'info, TokenAccount>,
    
    #[account(mut)]
    pub recipient_lp_account: Account<'info, TokenAccount>,
    
    #[account(signer)]
    pub user: AccountInfo<'info>,
    
    /// CHECK: NFT owner, verified in the instruction handler
    pub nft_owner: AccountInfo<'info>,
    
    /// CHECK: Recipient of the LP tokens
    pub recipient: AccountInfo<'info>,
    
    /// CHECK: This is the vault authority PDA
    #[account(
        seeds = [b"authority", vault.key().as_ref()],
        bump = vault.authority_bump,
    )]
    pub vault_authority: AccountInfo<'info>,
    
    /// CHECK: NFT mint account, would be used in real implementation
    pub nft_mint: AccountInfo<'info>,
    
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct DistributeReward<'info> {
    #[account(mut)]
    pub vault: Account<'info, Vault>,
    
    #[account(constraint = lp_mint.key() == vault.lp_mint @ VaultError::InvalidMint)]
    pub lp_mint: Account<'info, Mint>,
    
    #[account(mut, constraint = treasury_account.key() == vault.treasury_account @ VaultError::InvalidTreasury)]
    pub treasury_account: Account<'info, TokenAccount>,
    
    #[account(mut)]
    pub caller_usdc_account: Account<'info, TokenAccount>,
    
    #[account(signer)]
    pub caller: AccountInfo<'info>,
    
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct SendAssets<'info> {
    #[account(mut)]
    pub vault: Account<'info, Vault>,
    
    #[account(constraint = lp_mint.key() == vault.lp_mint @ VaultError::InvalidMint)]
    pub lp_mint: Account<'info, Mint>,
    
    #[account(mut, constraint = treasury_account.key() == vault.treasury_account @ VaultError::InvalidTreasury)]
    pub treasury_account: Account<'info, TokenAccount>,
    
    #[account(mut)]
    pub receiver_usdc_account: Account<'info, TokenAccount>,
    
    /// CHECK: This is the callbacks program that calls this instruction
    #[account(signer, constraint = is_callbacks_program(vault.registry, callbacks.key()) @ VaultError::NotCallbacks)]
    pub callbacks: AccountInfo<'info>,
    
    /// CHECK: This is the receiver of the assets
    pub receiver: AccountInfo<'info>,
    
    /// CHECK: This is the vault authority PDA
    #[account(
        seeds = [b"authority", vault.key().as_ref()],
        bump = vault.authority_bump,
    )]
    pub vault_authority: AccountInfo<'info>,
    
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct ReceiveAssets<'info> {
    #[account(mut)]
    pub vault: Account<'info, Vault>,
    
    #[account(constraint = lp_mint.key() == vault.lp_mint @ VaultError::InvalidMint)]
    pub lp_mint: Account<'info, Mint>,
    
    #[account(mut, constraint = treasury_account.key() == vault.treasury_account @ VaultError::InvalidTreasury)]
    pub treasury_account: Account<'info, TokenAccount>,
    
    #[account(mut)]
    pub caller_usdc_account: Account<'info, TokenAccount>,
    
    #[account(signer)]
    pub caller: AccountInfo<'info>,
    
    /// CHECK: This is the user associated with this transaction
    pub user: AccountInfo<'info>,
    
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct UpdateAccPnlPerTokenUsed<'info> {
    #[account(mut)]
    pub vault: Account<'info, Vault>,
    
    #[account(constraint = lp_mint.key() == vault.lp_mint @ VaultError::InvalidMint)]
    pub lp_mint: Account<'info, Mint>,
    
    /// CHECK: This is the OpenPnl program that calls this instruction
    #[account(signer, constraint = is_open_pnl_program(vault.registry, open_pnl.key()) @ VaultError::NotOpenPnl)]
    pub open_pnl: AccountInfo<'info>,
}

// Main Vault state account
#[account]
pub struct Vault {
    pub registry: Pubkey,
    pub usdc_mint: Pubkey,
    pub lp_mint: Pubkey,
    pub treasury_account: Pubkey,
    
    // Parameters
    pub max_acc_open_pnl_delta_per_token: u64,
    pub max_daily_acc_pnl_delta_per_token: u64,
    pub max_supply_increase_daily_p: u16,
    pub max_discount_p: u16,
    pub max_discount_threshold_p: u16,
    pub withdraw_lock_thresholds_p: [u16; 2],
    pub withdraw_epochs_locks: [u16; 3],
    
    // State
    pub current_epoch: u16,
    pub current_epoch_start: u32,
    pub last_max_supply_update_ts: u32,
    pub last_daily_acc_pnl_delta_reset_ts: u32,
    
    pub share_to_assets_price: u64,
    pub acc_rewards_per_token: u64,
    pub locked_deposits_count: u64,
    pub max_acc_open_pnl_delta_per_token_used: u64,
    pub current_epoch_positive_open_pnl: u64,
    pub current_max_supply: u64,
    
    // PnL tracking
    pub acc_pnl_per_token: i64,
    pub acc_pnl_per_token_used: i64,
    pub daily_acc_pnl_delta_per_token: i64,
    
    // Totals
    pub total_deposited: u64,
    pub total_closed_pnl: i64,
    pub total_rewards: u64,
    pub total_liability: i64,
    pub total_locked_discounts: u64,
    pub total_discounts: u64,
    
    // Vault authority
    pub authority_bump: u8,
    
    // LP mint supply (maintained separately for easier calculations)
    pub lp_mint_supply: u64,
    
    // User data
    pub withdraw_requests: HashMap<(Pubkey, u16), u64>, // (user, unlockEpoch) -> shares
    pub locked_deposits: HashMap<u64, LockedDeposit>,   // depositId -> LockedDeposit
}

impl Vault {
    // Approximate size of the account
    pub const SIZE: usize = 8 + // discriminator
        32 + 32 + 32 + 32 +     // pubkeys
        8 + 8 + 2 + 2 + 2 + 4 + 6 + // parameters
        2 + 4 + 4 + 4 +         // state timestamps
        8 + 8 + 8 + 8 + 8 + 8 + // state values
        8 + 8 + 8 +             // pnl tracking
        8 + 8 + 8 + 8 + 8 + 8 + // totals
        1 + 8 +                 // authority + lp supply
        500 +                   // withdraw requests hashmap
        500;                    // locked deposits hashmap
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct LockedDeposit {
    pub owner: Pubkey,
    pub shares: u64,
    pub assets_deposited: u64,
    pub assets_discount: u64,
    pub at_timestamp: u32,
    pub lock_duration: u32,
}

// Constants
pub const PRECISION_18: u64 = 1_000_000_000_000_000_000; // 18 decimals
pub const PRECISION_6: u64 = 1_000_000;                  // 6 decimals
pub const PRECISION_2: u64 = 100;                        // 2 decimals

pub const MIN_DAILY_ACC_PNL_DELTA: u64 = 10_000_000_000_000;  // 0.00001 in PRECISION_18
pub const MAX_DISCOUNT_P: u16 = 5000;                         // 50% in PRECISION_2
pub const MAX_SUPPLY_INCREASE_DAILY_P: u16 = 30000;           // 300% in PRECISION_2

pub const MAX_LOCK_DURATION: u32 = 365 * 24 * 60 * 60;        // 1 year in seconds
pub const MIN_LOCK_DURATION: u32 = 7 * 24 * 60 * 60;          // 1 week in seconds

// Events
#[event]
pub struct MaxAccOpenPnlDeltaPerTokenUpdated {
    pub value: u64,
}

#[event]
pub struct MaxDailyAccPnlDeltaPerTokenUpdated {
    pub value: u64,
}

#[event]
pub struct WithdrawLockThresholdsPUpdated {
    pub value: [u16; 2],
}

#[event]
pub struct MaxSupplyIncreaseDailyPUpdated {
    pub value: u16,
}

#[event]
pub struct MaxDiscountPUpdated {
    pub value: u16,
}

#[event]
pub struct MaxDiscountThresholdPUpdated {
    pub value: u16,
}

#[event]
pub struct CurrentMaxSupplyUpdated {
    pub value: u64,
}

#[event]
pub struct DailyAccPnlDeltaReset {}

#[event]
pub struct ShareToAssetsPriceUpdated {
    pub value: u64,
}

#[event]
pub struct Deposited {
    pub user: Pubkey,
    pub amount: u64,
    pub shares: u64,
}

#[event]
pub struct Redeemed {
    pub user: Pubkey,
    pub shares: u64,
    pub assets: u64,
}

#[event]
pub struct WithdrawRequested {
    pub user: Pubkey,
    pub shares: u64,
    pub current_epoch: u16,
    pub unlock_epoch: u16,
}

#[event]
pub struct WithdrawCanceled {
    pub user: Pubkey,
    pub shares: u64,
    pub current_epoch: u16,
    pub unlock_epoch: u16,
}

#[event]
pub struct DepositLocked {
    pub deposit_id: u64,
    pub user: Pubkey,
    pub recipient: Pubkey,
    pub deposit: LockedDeposit,
}

#[event]
pub struct DepositUnlocked {
    pub deposit_id: u64,
    pub owner: Pubkey,
    pub recipient: Pubkey,
    pub deposit: LockedDeposit,
}

#[event]
pub struct RewardDistributed {
    pub distributor: Pubkey,
    pub amount: u64,
    pub acc_rewards_per_token: u64,
}

#[event]
pub struct AssetsSent {
    pub sender: Pubkey,
    pub receiver: Pubkey,
    pub amount: u64,
}

#[event]
pub struct AssetsReceived {
    pub sender: Pubkey,
    pub user: Pubkey,
    pub amount: u64,
}

#[event]
pub struct AccPnlPerTokenUsedUpdated {
    pub updater: Pubkey,
    pub current_epoch: u16,
    pub prev_positive_open_pnl: u64,
    pub new_positive_open_pnl: u64,
    pub current_epoch_positive_open_pnl: u64,
    pub acc_pnl_per_token_used: i64,
}

// Helper functions to verify roles via Registry (these would be implemented as CPI calls in a real system)
fn is_gov(registry_info: AccountInfo, signer_key: Pubkey) -> bool {
    // This would be a CPI call to registry to check if the signer is gov
    // Placeholder implementation
    true
}

fn is_callbacks_program(registry: Pubkey, program_id: Pubkey) -> bool {
    // This would be a CPI call to registry to check if the program is registered as callbacks
    // Placeholder implementation
    true
}
fn is_open_pnl_program(registry: Pubkey, program_id: Pubkey) -> bool {
    // This would be a CPI call to registry to check if the program is registered as open_pnl
    // Placeholder implementation
    true
}

// Error codes
#[error_code]
pub enum VaultError {
    #[msg("Invalid parameters")]
    InvalidParameters,
    
    #[msg("Zero amount")]
    ZeroAmount,
    
    #[msg("Zero price")]
    ZeroPrice,
    
    #[msg("Not enough assets")]
    NotEnoughAssets,
    
    #[msg("Exceeds maximum mint")]
    ExceedsMaxMint,
    
    #[msg("Invalid mint")]
    InvalidMint,
    
    #[msg("Invalid treasury")]
    InvalidTreasury,
    
    #[msg("Invalid registry")]
    InvalidRegistry,
    
    #[msg("Not gov")]
    NotGov,
    
    #[msg("Not callbacks")]
    NotCallbacks,
    
    #[msg("Not open PnL")]
    NotOpenPnl,
    
    #[msg("Insufficient shares")]
    InsufficientShares,
    
    #[msg("No withdraw request")]
    NoWithdrawRequest,
    
    #[msg("Insufficient withdraw request")]
    InsufficientWithdrawRequest,
    
    #[msg("Invalid lock duration")]
    InvalidLockDuration,
    
    #[msg("No active discount")]
    NoActiveDiscount,
    
    #[msg("No discount")]
    NoDiscount,
    
    #[msg("Deposit not found")]
    DepositNotFound,
    
    #[msg("Not deposit owner")]
    NotDepositOwner,
    
    #[msg("Deposit not unlocked")]
    DepositNotUnlocked,
    
    #[msg("Maximum daily PnL reached")]
    MaxDailyPnlReached,
    
    #[msg("Wait for next epoch start")]
    WaitNextEpochStart,
}
