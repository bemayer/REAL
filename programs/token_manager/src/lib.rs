use anchor_lang::prelude::*;
use anchor_spl::token_interface::{Mint, TokenInterface};


declare_id!("DFUYFchyBFtTjwGUKwdd6KsozCkT1Qkpx18KJAk5Esv5");

#[program]
pub mod token_manager {
    use super::*;

    // Initializes the TokenManager state account.
    // This account will store all created token mints along with their ISIN codes.
    pub fn initialize_token_manager(ctx: Context<InitializeTokenManager>) -> Result<()> {
        let state = &mut ctx.accounts.token_manager;
        state.tokens = Vec::new();
        Ok(())
    }

    // Creates a new share (i.e. deploys a new token mint) with the specified number of decimals
    // and the provided ISIN code. The mint authority is set to the TokenManager PDA.
    pub fn create_new_share(
        ctx: Context<CreateNewShare>,
        _decimals: u8,
        isin: String,
    ) -> Result<()> {
        // Record the new token’s mint address along with its ISIN.
        let token_share = TokenShare {
            mint: ctx.accounts.mint.key(),
            isin,
        };
        ctx.accounts.token_manager.tokens.push(token_share);

        Ok(())
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
        seeds = [b"token", token_manager.key().as_ref(), &token_manager.tokens.len().to_le_bytes()],
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
    #[max_len(100)]
    pub tokens: Vec<TokenShare>,
}

#[account]
#[derive(InitSpace)]
pub struct TokenShare {
    pub mint: Pubkey,
    #[max_len(12)]
    pub isin: String,
}
