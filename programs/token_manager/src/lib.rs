use anchor_lang::prelude::*;
use anchor_spl::token_2022::{
    self,
    Token2022,
};

declare_id!("41BBJyFbgxQ2kZFz7w5VzBwYAQ4L329zVVJHRXofeNjF");

#[program]
pub mod token_manager {
    use super::*;

    /// Creates a new SPL Token 2022 mint.
    /// - `decimals` is the number of decimal places this mint will have.
    /// - The authority that signs this transaction becomes the mint authority (and optionally the freeze authority).
    pub fn create_new_share(
        ctx: Context<CreateNewShare>,
        decimals: u8
    ) -> Result<()> {
        // Build the CPI context to call create_mint in anchor_spl::token_2022
        let cpi_ctx = CpiContext::new(
            ctx.accounts.token_program.to_account_info(),
            token_2022::InitializeMint2 {
                mint: ctx.accounts.mint.to_account_info(),
            },
        );

        // Use anchor_spl::token_2022 to create a new mint.
        // - The authority will be the mint_authority.
        // - We also set the same authority as the freeze_authority here.
        //   If you do not want a freeze authority, pass `None`.
        token_2022::initialize_mint2(
            cpi_ctx,
            decimals,
            &ctx.accounts.authority.key(),
            Some(&ctx.accounts.authority.key()),
        )?;

        Ok(())
    }
}

#[derive(Accounts)]
pub struct CreateNewShare<'info> {
    /// The wallet signing the transaction and paying for account creation.
    #[account(mut)]
    pub authority: Signer<'info>,

    /// The new mint account for our SPL Token 2022.
    /// We use `init` to create it, specifying space for a Mint (82 bytes),
    /// setting the owner to the Token2022 program, and deriving a PDA with seeds.
    /// CHECK: why should we check that ?
    #[account(
        init,
        payer = authority,
        space = 82,
        seeds = [b"my-mint".as_ref()],
        bump,
        owner = anchor_spl::token_2022::ID
    )]
    pub mint: AccountInfo<'info>,

    /// Standard programs needed by Anchor and our CPI.
    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token2022>,
    pub rent: Sysvar<'info, Rent>,
}
