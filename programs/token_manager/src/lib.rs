use anchor_lang::{
    prelude::*,
    solana_program::{
        account_info::AccountInfo,
        program::invoke_signed,
        pubkey::Pubkey,
        system_instruction,
        sysvar::rent::Rent,
    },
};

use anchor_spl::{
    token_2022::{
        mint_to,
        MintTo,
        Token2022,
        ID as TOKEN_2022_PROGRAM_ID,
    },
    token_interface::{
        Mint,
        TokenAccount,
        TokenInterface,
    },
};

use spl_pod::primitives::PodBool;

use spl_tlv_account_resolution::{
    account::ExtraAccountMeta,
    state::ExtraAccountMetaList,
};

use spl_transfer_hook_interface::{
    collect_extra_account_metas_signer_seeds,
    instruction::ExecuteInstruction,
};


declare_id!("DFUYFchyBFtTjwGUKwdd6KsozCkT1Qkpx18KJAk5Esv5");

#[program]
pub mod token_manager {
    use spl_pod::solana_program::program::invoke;

    use super::*;

    /// Initializes the TokenManager state account.
    /// This account will store all created token mints along with their ISIN codes.
    pub fn initialize_token_manager(ctx: Context<InitializeTokenManager>) -> Result<()> {
        ctx.accounts.token_manager.tokens = Vec::new();
        ctx.accounts.token_manager.whitelist = Vec::new();
        ctx.accounts.token_manager.current_token_index = 0;
        ctx.accounts.token_manager.creator = ctx.accounts.signer.key();
        Ok(())
    }

    /// Creates a new token share by deploying a new token mint with the specified number of decimals and ISIN code.
    /// Also initializes the transfer hook to use this program for transfer validation.
    /// Uses SPL Token 2022 metadata extensions for token metadata.
    ///
    /// # Arguments
    ///
    /// * `decimals` - The number of decimals for the token mint.
    /// * `isin` - The unique ISIN code identifier for the token.
    pub fn create_new_share(
        ctx: Context<CreateNewShare>,
        decimals: u8,
        isin: String,
    ) -> Result<()> {
        msg!("Creating new share with ISIN: {}", isin);
        // Validate ISIN format (should be 12 characters)
        if isin.len() != 12 {
            return Err(error!(TokenManagerError::InvalidIsinLength));
        }

        // Validate that current_token_index won't overflow
        let next_index = ctx
            .accounts
            .token_manager
            .current_token_index
            .checked_add(1)
            .ok_or(error!(TokenManagerError::IndexOverflow))
            .expect("Index overflow");

        // 1. Calculate required space for mint with all extensions
        // TODO: Calculate space correctly
        // let space = ExtensionType::try_calculate_account_len::<spl_token_2022::state::Mint>(&[
        //     ExtensionType::TransferHook,
        //     ExtensionType::TokenMetadata,
        // ]).expect("Failed to calculate space");
        let space = 133;

        // 2. Calculate rent exemption
        let rent = Rent::get().expect("Rent not found");
        let lamports = rent.minimum_balance(space);

        // 3. Get PDA seeds from Anchor's context
        let token_mint_bump = ctx.bumps.token_mint;
        let token_manager = ctx.accounts.token_manager.key();
        let token_mint_seeds = &[
            b"token-mint",
            token_manager.as_ref(),
            &ctx.accounts.token_manager.current_token_index.to_le_bytes(),
            &[token_mint_bump],
        ];
        let token_mint_signer = &[&token_mint_seeds[..]];

        // 4. Create the mint account
        msg!(
            "Creating token mint account {} with {} bytes",
            ctx.accounts.token_mint.key(),
            space,
        );

        invoke_signed(
            &system_instruction::create_account(
                &ctx.accounts.signer.key(),
                &ctx.accounts.token_mint.key(),
                lamports,
                space as u64,
                &TOKEN_2022_PROGRAM_ID,
            ),
            &[
                ctx.accounts.signer.to_account_info(),
                ctx.accounts.token_mint.to_account_info(),
                ctx.accounts.system_program.to_account_info(),
            ],
            token_mint_signer,
        ).expect("Failed to create SPL Token 2022 mint account");

        // 5. Initialize the mint
        msg!("Initializing mint {}", ctx.accounts.token_mint.key());
        let init_mint_ix = spl_token_2022::instruction::initialize_mint2(
            &TOKEN_2022_PROGRAM_ID,
            &ctx.accounts.token_mint.key(),
            &ctx.accounts.token_mint_authority.key(),
            Some(&ctx.accounts.token_mint_authority.key()),
            decimals,
        ).expect("Failed to create SPL Token 2022 mint instruction");

        msg!("Invoking SPL Token 2022 mint instruction");

        invoke(
            &init_mint_ix,
            &[
                ctx.accounts.token_mint.to_account_info(),
                ctx.accounts.token_mint_authority.to_account_info(),
                ctx.accounts.token_program.to_account_info(),
            ],
        ).expect("Failed to invoke SPL Token 2022 mint instruction");

        // 6. Initialize the transfer hook extension
        msg!(
            "Initializing transfer hook for token {}",
            ctx.accounts.token_mint.key()
        );
        let hook_ix = spl_token_2022::extension::transfer_hook::instruction::initialize(
            &TOKEN_2022_PROGRAM_ID,
            &ctx.accounts.token_mint.key(),
            Some(ctx.accounts.token_manager.key()),
            Some(*ctx.program_id),
        )?;

        invoke_signed(
            &hook_ix,
            &[
                ctx.accounts.token_program.to_account_info(),
                ctx.accounts.token_mint.to_account_info(),
            ],
            token_mint_signer,
        )?;

        // 7. Initialize the token metadata extension
        // Create the metadata account
        let metadata_name = format!("Security Token {}", isin);
        let metadata_symbol = isin.clone();
        let metadata_uri = String::new();

        let metadata_space = metadata_name.len() + metadata_symbol.len() + metadata_uri.len() + 100;
        let metadata_lamports = rent.minimum_balance(metadata_space);
        let token_mint = ctx.accounts.token_mint.key();
        let metadata_seeds = &[b"token-metadata", token_mint.as_ref()];
        let metadata_signer = &[&metadata_seeds[..]];

        invoke_signed(
            &system_instruction::create_account(
                &ctx.accounts.signer.key(),
                &ctx.accounts.token_metadata.key(),
                metadata_lamports,
                metadata_space as u64,
                &TOKEN_2022_PROGRAM_ID,
            ),
            &[
                ctx.accounts.signer.to_account_info(),
                ctx.accounts.token_metadata.to_account_info(),
                ctx.accounts.system_program.to_account_info(),
            ],
            metadata_signer,
        )?;

        let init_metadata_ix = spl_token_metadata_interface::instruction::initialize(
            &TOKEN_2022_PROGRAM_ID,
            &ctx.accounts.token_metadata.key(),
            &ctx.accounts.token_mint.key(),
            &ctx.accounts.signer.key(),
            &ctx.accounts.signer.key(),
            metadata_name,
            metadata_symbol,
            metadata_uri,
        );

        invoke_signed(
            &init_metadata_ix,
            &[
                ctx.accounts.token_program.to_account_info(),
                ctx.accounts.token_mint.to_account_info(),
                ctx.accounts.token_metadata.to_account_info(),
                ctx.accounts.signer.to_account_info(),
            ],
            metadata_signer,
        )?;

        // 8. Create the extra account meta list for transfer hook validation
        let account_metas = vec![ExtraAccountMeta {
            discriminator: 0,
            address_config: ctx.accounts.token_manager.key().to_bytes(),
            is_signer: PodBool::from(false),
            is_writable: PodBool::from(false),
        }];

        // Use standardized signer seeds for the meta list
        let bump = [ctx.bumps.extra_account_meta_list];
        let mint = ctx.accounts.token_mint.key();
        let signer_seeds = collect_extra_account_metas_signer_seeds(&mint, &bump);

        // Calculate account size for meta list
        let account_size = u64::try_from(ExtraAccountMetaList::size_of(account_metas.len())?)?;
        let meta_list_lamports = rent.minimum_balance(account_size as usize); // Fixed: Making rent exempt

        // Create and allocate the PDA account for meta list
        invoke_signed(
            &system_instruction::create_account(
                &ctx.accounts.signer.key(),
                &ctx.accounts.extra_account_meta_list.key(),
                meta_list_lamports,
                account_size,
                &TOKEN_2022_PROGRAM_ID,
            ),
            &[
                ctx.accounts.signer.to_account_info(),
                ctx.accounts.extra_account_meta_list.clone(),
                ctx.accounts.system_program.to_account_info(),
            ],
            &[&signer_seeds],
        )?;

        // Initialize the meta list data
        let mut data = ctx.accounts.extra_account_meta_list.try_borrow_mut_data()?;
        ExtraAccountMetaList::init::<ExecuteInstruction>(&mut data, &account_metas)?;

        msg!(
            "Extra account meta list initialized for token {}",
            ctx.accounts.token_mint.key()
        );

        // 9. Store the token information and update index
        let token_share = TokenShare {
            token_index: ctx.accounts.token_manager.current_token_index,
            mint: ctx.accounts.token_mint.key(),
            isin,
        };

        ctx.accounts.token_manager.tokens.push(token_share);
        ctx.accounts.token_manager.current_token_index = next_index;

        msg!(
            "Token share created successfully with index {}",
            ctx.accounts.token_manager.current_token_index - 1
        );

        Ok(())
    }

    /// Adds a wallet authorization to the whitelist for a token identified by its ISIN.
    pub fn add_to_whitelist(ctx: Context<Whitelist>, wallet: Pubkey, isin: String) -> Result<()> {
        // Verify the signer is the creator of the token manager
        if ctx.accounts.signer.key() != ctx.accounts.token_manager.creator {
            return Err(error!(TokenManagerError::Unauthorized));
        }

        // Check if the whitelist is full
        if ctx.accounts.token_manager.whitelist.len() >= 10 {
            return Err(error!(TokenManagerError::WhitelistFull));
        }

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

    /// Removes a wallet authorization from the whitelist.
    pub fn remove_from_whitelist(
        ctx: Context<Whitelist>,
        wallet: Pubkey,
        isin: String,
    ) -> Result<()> {
        // Verify the signer is the creator of the token manager
        if ctx.accounts.signer.key() != ctx.accounts.token_manager.creator {
            return Err(error!(TokenManagerError::Unauthorized));
        }

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

    #[interface(spl_transfer_hook_interface::execute)]
    pub fn transfer_hook(ctx: Context<TransferHook>) -> Result<()> {
        let mint_key = ctx.accounts.mint.key();
        let owner_key = ctx.accounts.owner.key();

        // Check if the wallet is whitelisted
        if let Some(_) = &ctx
            .accounts
            .token_manager
            .whitelist
            .iter()
            .find(|auth| auth.mint == mint_key && auth.authority == owner_key)
        {
            return Ok(());
        }

        Err(error!(TokenManagerError::TransferNotAllowed))
    }

    pub fn get_token(ctx: Context<GetToken>, isin: String) -> Result<Pubkey> {
        if let Some(token) = ctx
            .accounts
            .token_manager
            .tokens
            .iter()
            .find(|t| t.isin == isin)
        {
            return Ok(token.mint);
        }

        Err(error!(TokenManagerError::TokenNotFound))
    }

    pub fn mint_tokens(ctx: Context<MintToken>, amount: u64) -> Result<()> {
        // Verify the signer is the creator of the token manager
        if ctx.accounts.signer.key() != ctx.accounts.token_manager.creator {
            return Err(error!(TokenManagerError::Unauthorized));
        }

        let token_mint_authority_bump = ctx.bumps.token_mint_authority;
        let token_mint_authority_key = ctx.accounts.token_mint_authority.key();
        let token_mint_authority_seeds = &[
            b"token-mint-authority",
            token_mint_authority_key.as_ref(),
            &[token_mint_authority_bump],
        ];
        let token_mint_authority_signer_seeds = &[&token_mint_authority_seeds[..]];

        // Mint tokens with the correct CPI context
        let cpi_accounts = MintTo {
            mint: ctx.accounts.token_mint.to_account_info(),
            to: ctx.accounts.destination.to_account_info(),
            authority: ctx.accounts.token_mint_authority.to_account_info(),
        };

        mint_to(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                cpi_accounts,
                token_mint_authority_signer_seeds,
            ),
            amount,
        )?;

        msg!(
            "Minted {} tokens of PubKey {} to {}",
            amount,
            ctx.accounts.token_mint.key(),
            ctx.accounts.destination.key()
        );

        Ok(())
    }
}

#[derive(Accounts)]
pub struct InitializeTokenManager<'info> {
    /// The wallet signing the transaction and paying for account creation
    #[account(mut)]
    pub signer: Signer<'info>,

    /// The main account that stores token information and whitelist
    /// Created as a PDA derived from "token-manager" + signer
    #[account(
        init,
        payer = signer,
        space = TokenManager::INIT_SPACE,
        seeds = [b"token-manager", signer.key().as_ref()],
        bump,
    )]
    pub token_manager: Account<'info, TokenManager>,

    /// Required for creating new accounts
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct CreateNewShare<'info> {
    /// The wallet signing and paying for the transaction
    #[account(mut)]
    pub signer: Signer<'info>,

    /// Account storing token metadata and whitelist information
    #[account(
        mut,
        seeds = [b"token-manager", signer.key().as_ref()],
        bump,
    )]
    pub token_manager: Account<'info, TokenManager>,

    /// The SPL token mint being created for this share
    /// Uses token-mint + token_manager + index as seeds
    #[account(
        mut,
        seeds = [b"token-mint", token_manager.key().as_ref(), &token_manager.current_token_index.to_le_bytes()],
        bump,
    )]
    /// CHECK: This is initialized within the instruction
    pub token_mint: UncheckedAccount<'info>,

    /// Metadata account that will store token information
    #[account(
        mut,
        seeds = [b"token-metadata", token_mint.key().as_ref()],
        bump,
    )]
    /// CHECK: We initialize this in the instruction if needed
    pub token_metadata: UncheckedAccount<'info>,

    /// PDA that serves as the mint authority for token_mint
    /// Only this program can sign as this authority
    #[account(
        seeds = [b"token-mint-authority", token_mint.key().as_ref()],
        bump,
    )]
    pub token_mint_authority: SystemAccount<'info>,

    /// Account storing metadata for SPL's transfer hook
    /// Lists additional accounts to pass during transfers
    /// CHECK: This account is verified in the CreateNewShare implementation
    #[account(
        mut,
        seeds = [b"extra-account-meta-list", token_mint.key().as_ref()],
        bump)
    ]
    pub extra_account_meta_list: AccountInfo<'info>,

    /// Token program interface for SPL Token 2022
    pub token_program: Interface<'info, TokenInterface>,

    /// Required for creating new accounts
    pub system_program: Program<'info, System>,

    /// Required for rent calculations
    pub rent: Sysvar<'info, Rent>,
}

#[derive(Accounts)]
pub struct Whitelist<'info> {
    /// The wallet signing the transaction
    #[account(mut)]
    pub signer: Signer<'info>,

    /// The account containing the whitelist to be modified
    /// Only the creator should modify the whitelist
    #[account(
        mut,
        seeds = [b"token-manager", signer.key().as_ref()],
        bump,
    )]
    pub token_manager: Account<'info, TokenManager>,
}

#[derive(Accounts)]
pub struct TransferHook<'info> {
    /// The token account sending tokens
    /// Must have the specified mint and be owned by owner
    #[account(
        token::mint = mint,
        token::authority = owner,
    )]
    pub source_token: InterfaceAccount<'info, TokenAccount>,

    /// The mint of the token being transferred
    pub mint: InterfaceAccount<'info, Mint>,

    /// The token account receiving tokens
    /// Must have the specified mint
    #[account(
        token::mint = mint,
    )]
    pub destination_token: InterfaceAccount<'info, TokenAccount>,

    /// The authority (owner) of the source token account
    /// The program verifies if this wallet is whitelisted
    /// CHECK: This account is verified in the TransferHook implementation
    pub owner: UncheckedAccount<'info>,

    /// Account containing extra metadata for the transfer hook
    /// Created by SPL Token 2022 program
    /// CHECK: This account is verified in the TransferHook implementation
    #[account(
        mut,
        seeds = [b"extra-account-meta-list", mint.key().as_ref()],
        bump)
    ]
    pub extra_account_meta_list: AccountInfo<'info>,

    /// Account storing the whitelist of authorized wallets
    /// Used to validate if the owner can transfer tokens
    #[account(
        seeds = [b"token-manager", token_manager.creator.as_ref()],
        bump,
    )]
    pub token_manager: Account<'info, TokenManager>,
}

/// Structure for querying a token mint by ISIN
#[derive(Accounts)]
pub struct GetToken<'info> {
    /// The token manager containing the tokens information
    #[account(
        seeds = [b"token-manager", signer.key().as_ref()],
        bump,
    )]
    pub token_manager: Account<'info, TokenManager>,

    /// The wallet signing the transaction
    pub signer: Signer<'info>,
}

/// Structure for the mint_tokens instruction
#[derive(Accounts)]
pub struct MintToken<'info> {
    /// The wallet signing the transaction
    #[account(mut)]
    pub signer: Signer<'info>,

    /// The token manager containing token information
    #[account(
        seeds = [b"token-manager", signer.key().as_ref()],
        bump,
        constraint = signer.key() == token_manager.creator @ TokenManagerError::Unauthorized,
    )]
    pub token_manager: Account<'info, TokenManager>,

    /// The token mint
    #[account(mut)]
    pub token_mint: InterfaceAccount<'info, Mint>,

    /// The PDA with authority to mint tokens
    #[account(
        seeds = [b"token-mint-authority", token_mint.key().as_ref()],
        bump,
    )]
    pub token_mint_authority: SystemAccount<'info>,

    /// The account receiving the tokens
    #[account(
        mut,
        constraint = destination.mint == token_mint.key() @ TokenManagerError::InvalidTokenAccount
    )]
    pub destination: InterfaceAccount<'info, TokenAccount>,

    /// The Token 2022 program
    pub token_program: Program<'info, Token2022>,
}

#[account]
#[derive(InitSpace)]
pub struct TokenShare {
    pub token_index: u64,
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

#[account]
#[derive(InitSpace)]
pub struct TokenManager {
    pub creator: Pubkey,
    pub current_token_index: u64,
    #[max_len(10)]
    pub tokens: Vec<TokenShare>,
    #[max_len(10)]
    pub whitelist: Vec<Authorization>,
}

#[error_code]
pub enum TokenManagerError {
    #[msg("Token not found")]
    TokenNotFound,
    #[msg("Wallet not found")]
    WalletNotFound,
    #[msg("Transfer not allowed")]
    TransferNotAllowed,
    #[msg("Failed to initialize transfer hook")]
    TransferHookInitFailed,
    #[msg("Invalid token account")]
    InvalidTokenAccount,
    #[msg("Invalid ISIN length")]
    InvalidIsinLength,
    #[msg("Unauthorized operation")]
    Unauthorized,
    #[msg("Index overflow")]
    IndexOverflow,
    #[msg("Whitelist is full")]
    WhitelistFull,
}
