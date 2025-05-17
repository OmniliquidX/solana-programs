use anchor_lang::prelude::*;
use anchor_lang::solana_program::pubkey::Pubkey;

declare_id!("3pjibswEuCbXPtdemyuvDxbTMaGYxsJBG73uZpZajeRK");

#[program]
pub mod omniliquid_registry {
    use super::*;

    pub fn initialize(ctx: Context<Initialize>, gov: Pubkey, dev: Pubkey, manager: Pubkey) -> Result<()> {
        let registry = &mut ctx.accounts.registry;
        registry.gov = gov;
        registry.dev = dev;
        registry.manager = manager;
        registry.owner = ctx.accounts.owner.key();
        registry.authority_bump = ctx.bumps.registry_authority;
        registry.programs = Vec::new();
        registry.supported_assets = Vec::new();
        Ok(())
    }

    pub fn set_gov(ctx: Context<OnlyOwner>, new_gov: Pubkey) -> Result<()> {
        require!(
            new_gov != ctx.accounts.registry.dev && 
            new_gov != ctx.accounts.registry.manager && 
            new_gov != ctx.accounts.registry.owner,
            RegistryError::HasAlreadyRole
        );
        
        let registry = &mut ctx.accounts.registry;
        registry.gov = new_gov;
        
        emit!(GovUpdated { gov: new_gov });
        Ok(())
    }

    pub fn set_dev(ctx: Context<OnlyOwner>, new_dev: Pubkey) -> Result<()> {
        require!(
            new_dev != ctx.accounts.registry.gov && 
            new_dev != ctx.accounts.registry.manager && 
            new_dev != ctx.accounts.registry.owner,
            RegistryError::HasAlreadyRole
        );
        
        let registry = &mut ctx.accounts.registry;
        registry.dev = new_dev;
        
        emit!(DevUpdated { dev: new_dev });
        Ok(())
    }

    pub fn set_manager(ctx: Context<OnlyOwner>, new_manager: Pubkey) -> Result<()> {
        require!(
            new_manager != ctx.accounts.registry.gov && 
            new_manager != ctx.accounts.registry.dev && 
            new_manager != ctx.accounts.registry.owner,
            RegistryError::HasAlreadyRole
        );
        
        let registry = &mut ctx.accounts.registry;
        registry.manager = new_manager;
        
        emit!(ManagerUpdated { manager: new_manager });
        Ok(())
    }

    pub fn register_program(ctx: Context<OnlyGov>, name: String, program_id: Pubkey) -> Result<()> {
        let registry = &mut ctx.accounts.registry;
        
        // Check if program already registered
        require!(
            !registry.programs.iter().any(|(program_name, _)| program_name == &name),
            RegistryError::AlreadyRegistered
        );
        
        // Store program in registry
        registry.programs.push((name.clone(), program_id));
        
        emit!(ProgramRegistered { 
            name: name, 
            program_id: program_id 
        });
        
        Ok(())
    }

    pub fn update_program(ctx: Context<OnlyGov>, name: String, program_id: Pubkey) -> Result<()> {
        let registry = &mut ctx.accounts.registry;
        
        // Find program index
        let program_index = registry.programs
            .iter()
            .position(|(program_name, _)| program_name == &name)
            .ok_or(RegistryError::NotFound)?;
        
        // Update program in registry
        registry.programs[program_index].1 = program_id;
        
        emit!(ProgramUpdated { 
            name: name, 
            program_id: program_id 
        });
        
        Ok(())
    }

    pub fn unregister_program(ctx: Context<OnlyGov>, name: String) -> Result<()> {
        let registry = &mut ctx.accounts.registry;
        
        // Find program index
        let program_index = registry.programs
            .iter()
            .position(|(program_name, _)| program_name == &name)
            .ok_or(RegistryError::NotFound)?;
        
        // Get program ID before removal
        let program_id = registry.programs[program_index].1;
        
        // Remove program from registry
        registry.programs.remove(program_index);
        
        emit!(ProgramUnregistered { 
            name: name, 
            program_id: program_id 
        });
        
        Ok(())
    }

    // Add RWA asset support
    pub fn register_asset(
        ctx: Context<OnlyGov>, 
        asset_id: String, 
        asset_type: AssetType, 
        pyth_price_feed: Pubkey,
        min_order_size: u64,
        max_leverage: u16,
        maintenance_margin_ratio: u16,
        liquidation_fee: u16,
        funding_rate_multiplier: u16,
        active: bool
    ) -> Result<()> {
        let registry = &mut ctx.accounts.registry;
        
        // Check if asset already registered
        require!(
            !registry.supported_assets.iter().any(|(id, _)| id == &asset_id),
            RegistryError::AssetAlreadyRegistered
        );
        
        // Store asset in registry
        registry.supported_assets.push((asset_id.clone(), Asset {
            asset_type: asset_type.clone(), 
            pyth_price_feed,
            min_order_size,
            max_leverage,
            maintenance_margin_ratio,
            liquidation_fee,
            funding_rate_multiplier,
            active
        }));
        
        emit!(AssetRegistered { 
            asset_id: asset_id, 
            asset_type,
            pyth_price_feed,
            active
        });
        
        Ok(())
    }

    pub fn update_asset(
        ctx: Context<OnlyGov>, 
        asset_id: String, 
        min_order_size: Option<u64>,
        max_leverage: Option<u16>,
        maintenance_margin_ratio: Option<u16>,
        liquidation_fee: Option<u16>,
        funding_rate_multiplier: Option<u16>,
        active: Option<bool>
    ) -> Result<()> {
        let registry = &mut ctx.accounts.registry;
        
        // Find asset index
        let asset_index = registry.supported_assets
            .iter()
            .position(|(id, _)| id == &asset_id)
            .ok_or(RegistryError::AssetNotFound)?;
        
        // Get and update asset
        let asset = &mut registry.supported_assets[asset_index].1;
        
        if let Some(min_size) = min_order_size {
            asset.min_order_size = min_size;
        }
        
        if let Some(leverage) = max_leverage {
            asset.max_leverage = leverage;
        }
        
        if let Some(margin) = maintenance_margin_ratio {
            asset.maintenance_margin_ratio = margin;
        }
        
        if let Some(fee) = liquidation_fee {
            asset.liquidation_fee = fee;
        }
        
        if let Some(multiplier) = funding_rate_multiplier {
            asset.funding_rate_multiplier = multiplier;
        }
        
        if let Some(is_active) = active {
            asset.active = is_active;
        }
        
        emit!(AssetUpdated { 
            asset_id: asset_id,
            active: asset.active
        });
        
        Ok(())
    }

    pub fn deactivate_asset(ctx: Context<OnlyGov>, asset_id: String) -> Result<()> {
        let registry = &mut ctx.accounts.registry;
        
        // Find asset index
        let asset_index = registry.supported_assets
            .iter()
            .position(|(id, _)| id == &asset_id)
            .ok_or(RegistryError::AssetNotFound)?;
        
        // Deactivate asset
        registry.supported_assets[asset_index].1.active = false;
        
        emit!(AssetUpdated { 
            asset_id: asset_id,
            active: false
        });
        
        Ok(())
    }
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, PartialEq, Eq)]
pub enum AssetType {
    Crypto,
    Stock,
    Forex,
    Commodity,
    Index,
    LongTail
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct Asset {
    pub asset_type: AssetType,
    pub pyth_price_feed: Pubkey,
    pub min_order_size: u64,
    pub max_leverage: u16,
    pub maintenance_margin_ratio: u16,    // Basis points (e.g., 500 = 5%)
    pub liquidation_fee: u16,             // Basis points
    pub funding_rate_multiplier: u16,     // Multiplier for base funding rate
    pub active: bool
}

#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(
        init,
        payer = owner,
        space = 8 + Registry::MAX_SIZE,
        seeds = [b"registry"],
        bump
    )]
    pub registry: Account<'info, Registry>,
    
    #[account(mut)]
    pub owner: Signer<'info>,
    
    /// CHECK: This is the PDA that will be the authority for various operations
    #[account(
        seeds = [b"authority", registry.key().as_ref()],
        bump,
    )]
    pub registry_authority: AccountInfo<'info>,
    
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct OnlyOwner<'info> {
    #[account(mut, has_one = owner @ RegistryError::NotOwner)]
    pub registry: Account<'info, Registry>,
    
    #[account(signer)]
    pub owner: AccountInfo<'info>,
}

#[derive(Accounts)]
pub struct OnlyGov<'info> {
    #[account(mut, has_one = gov @ RegistryError::NotGov)]
    pub registry: Account<'info, Registry>,
    
    #[account(signer)]
    pub gov: AccountInfo<'info>,
}

#[account]
pub struct Registry {
    pub gov: Pubkey,
    pub dev: Pubkey,
    pub manager: Pubkey,
    pub owner: Pubkey,
    pub authority_bump: u8,
    pub programs: Vec<(String, Pubkey)>,
    pub supported_assets: Vec<(String, Asset)>,
}

impl Registry {
    pub const MAX_SIZE: usize = 32 + 32 + 32 + 32 + 1 + 
    // Space for Vec<(String, Pubkey)>: 4 (vec len) + estimated capacity for 50 entries
    4 + (50 * (4 + 20 + 32)) + 
    // Space for Vec<(String, Asset)>: 4 (vec len) + estimated capacity for 50 entries
    4 + (50 * (4 + 10 + 1 + 32 + 8 + 2 + 2 + 2 + 2 + 1));
}

#[event]
pub struct GovUpdated {
    pub gov: Pubkey,
}

#[event]
pub struct DevUpdated {
    pub dev: Pubkey,
}

#[event]
pub struct ManagerUpdated {
    pub manager: Pubkey,
}

#[event]
pub struct ProgramRegistered {
    pub name: String,
    pub program_id: Pubkey,
}

#[event]
pub struct ProgramUpdated {
    pub name: String,
    pub program_id: Pubkey,
}

#[event]
pub struct ProgramUnregistered {
    pub name: String,
    pub program_id: Pubkey,
}

#[event]
pub struct AssetRegistered {
    pub asset_id: String,
    pub asset_type: AssetType,
    pub pyth_price_feed: Pubkey,
    pub active: bool,
}

#[event]
pub struct AssetUpdated {
    pub asset_id: String,
    pub active: bool,
}

#[error_code]
pub enum RegistryError {
    #[msg("Address already has a role")]
    HasAlreadyRole,
    #[msg("Program already registered")]
    AlreadyRegistered,
    #[msg("Program not found")]
    NotFound,
    #[msg("Not owner")]
    NotOwner,
    #[msg("Not gov")]
    NotGov,
    #[msg("Asset already registered")]
    AssetAlreadyRegistered,
    #[msg("Asset not found")]
    AssetNotFound,
}