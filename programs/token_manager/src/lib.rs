use anchor_lang::prelude::*;
use anchor_spl::token_interface::{Mint, TokenInterface};

declare_id!("DFUYFchyBFtTjwGUKwdd6KsozCkT1Qkpx18KJAk5Esv5");

#[program]
pub mod token_manager {
    use super::*;

    /// Initializes the TokenManager state account.
    /// This account will store all created token mints along with their ISIN codes.
    pub fn initialize_token_manager(ctx: Context<InitializeTokenManager>) -> Result<()> {
        ctx.accounts.token_manager.tokens = Vec::new();
        ctx.accounts.token_manager.current_token_index = 0;
        Ok(())
    }

    /// Creates a new token share by deploying a new token mint with the specified number of decimals and ISIN code.
    ///
    /// # Arguments
    ///
    /// * `_decimals` - The number of decimals for the token mint.
    /// * `isin` - The unique ISIN code identifier for the token.
    pub fn create_new_share(
        ctx: Context<CreateNewShare>,
        _decimals: u8,
        isin: String,
    ) -> Result<()> {
        let token_share = TokenShare {
            mint: ctx.accounts.mint.key(),
            isin,
        };
        ctx.accounts.token_manager.tokens.push(token_share);
        ctx.accounts.token_manager.current_token_index += 1;
        Ok(())
    }

    /// Adds a wallet authorization to the whitelist for a token identified by its ISIN.
    ///
    /// # Arguments
    ///
    /// * `wallet` - The wallet public key to be added to the whitelist.
    /// * `isin` - The unique ISIN code used to find the token.
    pub fn add_to_whitelist(
        ctx: Context<Whitelist>,
        wallet: Pubkey,
        isin: String,
    ) -> Result<()> {
        if let Some(token) = &ctx
            .accounts
            .token_manager
            .tokens
            .iter()
            .find(|token| token.isin == isin)
        {
            let authorization = Authorization {
                mint: token.mint,
                authority: wallet,
            };
            ctx.accounts.token_manager.whitelist.push(authorization);
            return Ok(());
        }
        return Err(error!(TokenManagerError::TokenNotFound));
    }

    /// Removes a wallet authorization from the whitelist for a token identified by its ISIN.
    ///
    /// # Arguments
    ///
    /// * `wallet` - The wallet public key to be removed from the whitelist.
    /// * `isin` - The unique ISIN code used to find the token.
    pub fn remove_from_whitelist(
        ctx: Context<Whitelist>,
        wallet: Pubkey,
        isin: String,
    ) -> Result<()> {
        if let Some(token) = &ctx
            .accounts
            .token_manager
            .tokens
            .iter()
            .find(|token| token.isin == isin)
        {
            if let Some(index) = &ctx
                .accounts
                .token_manager
                .whitelist
                .iter()
                .position(|auth| auth.mint == token.mint && auth.authority == wallet)
            {
                ctx.accounts.token_manager.whitelist.remove(*index);
                return Ok(());
            }
            return Err(error!(TokenManagerError::WalletNotFound));
        }
        Err(error!(TokenManagerError::TokenNotFound))
    }
}

#[derive(Accounts)]
pub struct InitializeTokenManager<'info> {
    #[account(mut)]
    pub signer: Signer<'info>,
    #[account(
        init,
        payer = signer,
        space = TokenManager::INIT_SPACE,
        seeds = [b"token-manager", signer.key().as_ref()],
        bump,
    )]
    pub token_manager: Account<'info, TokenManager>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(_decimals: u8)]
pub struct CreateNewShare<'info> {
    #[account(mut)]
    pub signer: Signer<'info>,
    #[account(
        mut,
        seeds = [b"token-manager", signer.key().as_ref()],
        bump,
    )]
    pub token_manager: Account<'info, TokenManager>,
    #[account(
        init,
        payer = signer,
        seeds = [b"token", token_manager.key().as_ref(), &token_manager.current_token_index.to_le_bytes()],
        bump,
        mint::decimals = _decimals,
        mint::authority = token_manager.key(),
    )]
    pub mint: InterfaceAccount<'info, Mint>,
    pub token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,
}

#[account]
#[derive(InitSpace)]
pub struct TokenManager {
    pub current_token_index: u64,
    #[max_len(10)]
    pub tokens: Vec<TokenShare>,
    #[max_len(10)]
    pub whitelist: Vec<Authorization>,
}

#[account]
#[derive(InitSpace)]
pub struct TokenShare {
    #[max_len(12)]
    pub isin: String,
    pub mint: Pubkey,
}

#[account]
#[derive(InitSpace)]
pub struct Authorization {
    pub mint: Pubkey,
    pub authority: Pubkey,
}

#[derive(Accounts)]
pub struct Whitelist<'info> {
    #[account(mut)]
    pub signer: Signer<'info>,
    #[account(
        mut,
        seeds = [b"token-manager", signer.key().as_ref()],
        bump,
    )]
    pub token_manager: Account<'info, TokenManager>,
}

#[error_code]
pub enum TokenManagerError {
    #[msg("Token not found")]
    TokenNotFound,
    #[msg("Wallet not found")]
    WalletNotFound,
}
