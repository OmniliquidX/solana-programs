use anchor_lang::prelude::*;
use anchor_spl::token::{self, Mint, MintTo, Token, TokenAccount, Transfer, Burn};
use std::convert::TryFrom;

declare_id!("CiTbKMyLecpE5LWcB1TkKFPtpnKD4AK1TAedmt4PjTgB");

#[program]
pub mod omniliquid_omni_token {
    use super::*;

    pub fn initialize(
        ctx: Context<Initialize>,
        name: String,
        symbol: String,
        uri: String,
        max_supply: u64,
    ) -> Result<()> {
        let token_config = &mut ctx.accounts.token_config;
        token_config.authority = ctx.accounts.authority.key();
        token_config.mint = ctx.accounts.mint.key();
        token_config.name = name;
        token_config.symbol = symbol;
        token_config.uri = uri;
        token_config.max_supply = max_supply;
        token_config.total_supply = 0;
        token_config.authority_bump = *ctx.bumps.get("token_authority").ok_or_else(|| error!(TokenError::InvalidAuthority))?;
        
        // Emit token initialized event
        emit!(TokenInitialized {
            authority: token_config.authority,
            mint: token_config.mint,
            name: token_config.name.clone(),
            symbol: token_config.symbol.clone(),
            max_supply: token_config.max_supply,
        });
        
        Ok(())
    }

    pub fn mint_tokens(
        ctx: Context<MintTokens>,
        amount: u64,
    ) -> Result<()> {
        let token_config = &mut ctx.accounts.token_config;
        
        // Check max supply
        require!(
            token_config.total_supply.checked_add(amount).unwrap() <= token_config.max_supply,
            TokenError::ExceedsMaxSupply
        );
        
        // Mint new tokens
        let seeds = &[
            b"token_authority".as_ref(),
            &[token_config.authority_bump],
        ];
        let signer = &[&seeds[..]];
        
        // Mint tokens to recipient
        let cpi_accounts = MintTo {
            mint: ctx.accounts.mint.to_account_info(),
            to: ctx.accounts.recipient.to_account_info(),
            authority: ctx.accounts.token_authority.to_account_info(),
        };
        
        token::mint_to(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                cpi_accounts,
                signer,
            ),
            amount,
        )?;
        
        // Update total supply
        token_config.total_supply = token_config.total_supply.checked_add(amount).unwrap();
        
        // Emit mint event
        emit!(TokensMinted {
            amount,
            recipient: ctx.accounts.recipient.owner,
            new_supply: token_config.total_supply,
        });
        
        Ok(())
    }

    pub fn burn_tokens(
        ctx: Context<BurnTokens>,
        amount: u64,
    ) -> Result<()> {
        let token_config = &mut ctx.accounts.token_config;
        
        // Burn tokens from the user account
        let cpi_accounts = Burn {
            mint: ctx.accounts.mint.to_account_info(),
            from: ctx.accounts.token_account.to_account_info(),
            authority: ctx.accounts.owner.to_account_info(),
        };
        
        token::burn(
            CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                cpi_accounts,
            ),
            amount,
        )?;
        
        // Update total supply
        token_config.total_supply = token_config.total_supply.checked_sub(amount).unwrap();
        
        // Emit burn event
        emit!(TokensBurned {
            amount,
            owner: ctx.accounts.owner.key(),
            new_supply: token_config.total_supply,
        });
        
        Ok(())
    }

    pub fn transfer_ownership(
        ctx: Context<TransferOwnership>,
        new_authority: Pubkey,
    ) -> Result<()> {
        let token_config = &mut ctx.accounts.token_config;
        
        // Ensure the new authority is not the zero address
        require!(new_authority != Pubkey::default(), TokenError::InvalidAuthority);
        
        // Store the old authority for the event
        let old_authority = token_config.authority;
        
        // Update the authority
        token_config.authority = new_authority;
        
        // Emit ownership transfer event
        emit!(OwnershipTransferred {
            old_authority,
            new_authority,
        });
        
        Ok(())
    }

    pub fn update_metadata(
        ctx: Context<UpdateMetadata>,
        name: Option<String>,
        symbol: Option<String>,
        uri: Option<String>,
        max_supply: Option<u64>,
    ) -> Result<()> {
        let token_config = &mut ctx.accounts.token_config;
        
        // Update fields that are provided
        if let Some(name) = name {
            token_config.name = name;
        }
        
        if let Some(symbol) = symbol {
            token_config.symbol = symbol;
        }
        
        if let Some(uri) = uri {
            token_config.uri = uri;
        }
        
        if let Some(max_supply) = max_supply {
            // Ensure new max supply is not less than current total supply
            require!(
                max_supply >= token_config.total_supply,
                TokenError::InvalidMaxSupply
            );
            
            token_config.max_supply = max_supply;
        }
        
        // Emit metadata updated event
        emit!(MetadataUpdated {
            name: token_config.name.clone(),
            symbol: token_config.symbol.clone(),
            uri: token_config.uri.clone(),
            max_supply: token_config.max_supply,
        });
        
        Ok(())
    }
}

#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(
        init,
        payer = payer,
        space = 8 + TokenConfig::SIZE,
        seeds = [b"token_config", mint.key().as_ref()],
        bump
    )]
    pub token_config: Account<'info, TokenConfig>,
    
    #[account(
        init,
        payer = payer,
        mint::decimals = 9,
        mint::authority = token_authority,
    )]
    pub mint: Account<'info, Mint>,
    
    /// CHECK: This is the PDA that will be the mint authority
    #[account(
        seeds = [b"token_authority"],
        bump,
    )]
    pub token_authority: AccountInfo<'info>,
    
    #[account(mut)]
    pub authority: Signer<'info>,
    
    #[account(mut)]
    pub payer: Signer<'info>,
    
    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
    pub rent: Sysvar<'info, Rent>,
}

#[derive(Accounts)]
pub struct MintTokens<'info> {
    #[account(
        mut,
        seeds = [b"token_config", mint.key().as_ref()],
        bump,
        has_one = mint,
        has_one = authority @ TokenError::InvalidAuthority,
    )]
    pub token_config: Account<'info, TokenConfig>,
    
    #[account(mut)]
    pub mint: Account<'info, Mint>,
    
    #[account(mut)]
    pub recipient: Account<'info, TokenAccount>,
    
    /// CHECK: This is the PDA that is the mint authority
    #[account(
        seeds = [b"token_authority"],
        bump = token_config.authority_bump,
    )]
    pub token_authority: AccountInfo<'info>,
    
    #[account(signer)]
    pub authority: AccountInfo<'info>,
    
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct BurnTokens<'info> {
    #[account(
        mut,
        seeds = [b"token_config", mint.key().as_ref()],
        bump,
        has_one = mint,
    )]
    pub token_config: Account<'info, TokenConfig>,
    
    #[account(mut)]
    pub mint: Account<'info, Mint>,
    
    #[account(
        mut,
        constraint = token_account.mint == mint.key() @ TokenError::InvalidMint,
        constraint = token_account.owner == owner.key() @ TokenError::InvalidOwner,
    )]
    pub token_account: Account<'info, TokenAccount>,
    
    #[account(signer)]
    pub owner: AccountInfo<'info>,
    
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct TransferOwnership<'info> {
    #[account(
        mut,
        seeds = [b"token_config", mint.key().as_ref()],
        bump,
        has_one = authority @ TokenError::InvalidAuthority,
    )]
    pub token_config: Account<'info, TokenConfig>,
    
    pub mint: Account<'info, Mint>,
    
    #[account(signer)]
    pub authority: AccountInfo<'info>,
}

#[derive(Accounts)]
pub struct UpdateMetadata<'info> {
    #[account(
        mut,
        seeds = [b"token_config", mint.key().as_ref()],
        bump,
        has_one = authority @ TokenError::InvalidAuthority,
    )]
    pub token_config: Account<'info, TokenConfig>,
    
    pub mint: Account<'info, Mint>,
    
    #[account(signer)]
    pub authority: AccountInfo<'info>,
}

#[account]
pub struct TokenConfig {
    pub authority: Pubkey,
    pub mint: Pubkey,
    pub name: String,
    pub symbol: String,
    pub uri: String,
    pub max_supply: u64,
    pub total_supply: u64,
    pub authority_bump: u8,
}

impl TokenConfig {
    pub const SIZE: usize = 32 + 32 + 4 + 32 + 4 + 10 + 4 + 200 + 8 + 8 + 1;
}

#[event]
pub struct TokenInitialized {
    pub authority: Pubkey,
    pub mint: Pubkey,
    pub name: String,
    pub symbol: String,
    pub max_supply: u64,
}

#[event]
pub struct TokensMinted {
    pub amount: u64,
    pub recipient: Pubkey,
    pub new_supply: u64,
}

#[event]
pub struct TokensBurned {
    pub amount: u64,
    pub owner: Pubkey,
    pub new_supply: u64,
}

#[event]
pub struct OwnershipTransferred {
    pub old_authority: Pubkey,
    pub new_authority: Pubkey,
}

#[event]
pub struct MetadataUpdated {
    pub name: String,
    pub symbol: String,
    pub uri: String,
    pub max_supply: u64,
}

#[error_code]
pub enum TokenError {
    #[msg("Invalid authority")]
    InvalidAuthority,
    
    #[msg("Exceeds maximum supply")]
    ExceedsMaxSupply,
    
    #[msg("Invalid maximum supply")]
    InvalidMaxSupply,
    
    #[msg("Invalid mint")]
    InvalidMint,
    
    #[msg("Invalid owner")]
    InvalidOwner,
}