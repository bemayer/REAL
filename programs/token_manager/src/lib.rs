use anchor_lang::{
    prelude::*,
    solana_program::{
        account_info::AccountInfo,
        program::{invoke, invoke_signed},
        pubkey::Pubkey,
        system_instruction,
        sysvar::rent::Rent,
    },
};

use anchor_spl::{
    token_2022::{mint_to, MintTo, Token2022},
    token_2022_extensions::spl_token_metadata_interface,
    token_interface::{Mint, TokenAccount},
};

use spl_token_2022::extension::ExtensionType;

use spl_tlv_account_resolution::{account::ExtraAccountMeta, state::ExtraAccountMetaList};

use spl_transfer_hook_interface::instruction::ExecuteInstruction;

declare_id!("DFUYFchyBFtTjwGUKwdd6KsozCkT1Qkpx18KJAk5Esv5");

#[program]
pub mod token_manager {
    use super::*;

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

    /// Initializes the TokenManager state account.
    /// This account will store all created token mints along with their ISIN codes.
    pub fn initialize_token_manager(ctx: Context<InitializeTokenManager>) -> Result<()> {
        ctx.accounts.token_manager.tokens = Vec::new();
        ctx.accounts.token_manager.whitelist = Vec::new();
        ctx.accounts.token_manager.current_token_index = 0;
        ctx.accounts.token_manager.creator = ctx.accounts.signer.key();
        Ok(())
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
        pub token_mint: AccountInfo<'info>,

        /// Account storing metadata for SPL's transfer hook
        /// Lists additional accounts to pass during transfers
        /// CHECK: This account is verified in the CreateNewShare implementation
        #[account(
        mut,
        seeds = [b"extra-account-metas", token_mint.key().as_ref()],
        bump,
        )]
        pub extra_account_meta_list: AccountInfo<'info>,

        /// Token program interface for SPL Token 2022
        pub token_program: Program<'info, Token2022>,

        /// Required for creating new accounts
        pub system_program: Program<'info, System>,
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
        // Validate ISIN format (should be 12 characters)
        if isin.len() != 12 {
            return Err(error!(TokenManagerError::InvalidIsinLength));
        }

        // 1. Calculate required space for mint with all extensions and metadata
        let name = format!("Security Token {}", isin);
        let symbol = isin.clone();
        let uri = String::new();

        // Calculate space with embedded metadata
        let token_space =
            ExtensionType::try_calculate_account_len::<spl_token_2022::state::Mint>(&[
                ExtensionType::TransferHook,
                ExtensionType::MetadataPointer,
            ])
            .expect("Failed to calculate space");
        let metadata_space = calculate_metadata_space(&name, &symbol, &uri);
        let total_space = token_space + metadata_space;

        // 2. Calculate rent exemption
        let rent = Rent::get()?;
        let lamports = rent.minimum_balance(total_space);

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
        let token_mint_key = &ctx.accounts.token_mint.key();

        invoke_signed(
            &system_instruction::create_account(
                &ctx.accounts.signer.key(),
                token_mint_key,
                lamports,
                token_space as u64,
                &ctx.accounts.token_program.key(),
            ),
            &[
                ctx.accounts.signer.to_account_info(),
                ctx.accounts.token_mint.to_account_info(),
                ctx.accounts.system_program.to_account_info(),
            ],
            token_mint_signer,
        )?;

        // 5. Initialize extensions first

        // Initialize TransferHook extension
        let transfer_hook_ix = spl_token_2022::extension::transfer_hook::instruction::initialize(
            &ctx.accounts.token_program.key(),
            token_mint_key,
            Some(ctx.accounts.token_manager.key()),
            Some(*ctx.program_id),
        )?;

        invoke(
            &transfer_hook_ix,
            &[
                ctx.accounts.token_mint.to_account_info(),
                ctx.accounts.extra_account_meta_list.to_account_info(),
                ctx.accounts.token_program.to_account_info(),
            ],
        )?;

        // Initialize MetadataPointer extension
        let metadata_pointer_ix =
            spl_token_2022::extension::metadata_pointer::instruction::initialize(
                &ctx.accounts.token_program.key(),
                token_mint_key,
                Some(*token_mint_key),
                Some(*token_mint_key),
            )?;

        invoke(
            &metadata_pointer_ix,
            &[
                ctx.accounts.token_mint.to_account_info(),
            ],
        )?;

        // 6. Now initialize the basic mint
        let init_mint_ix = spl_token_2022::instruction::initialize_mint2(
            &ctx.accounts.token_program.key(),
            token_mint_key,
            token_mint_key,
            Some(token_mint_key),
            decimals,
        )?;

        invoke(
            &init_mint_ix,
            &[
                ctx.accounts.token_mint.to_account_info(),
            ],
        )?;

        // Initialize TokenMetadata extension
        let token_metadata_ix = spl_token_metadata_interface::instruction::initialize(
            &ctx.accounts.token_program.key(),
            token_mint_key,
            token_mint_key,
            token_mint_key,
            token_mint_key,
            name.clone(),
            symbol.clone(),
            uri.clone(),
        );

        invoke_signed(
            &token_metadata_ix,
            &[
                ctx.accounts.token_mint.to_account_info(),
            ],
            token_mint_signer,
        )?;

        // 7. Create and initialize the extra account meta list for transfer hooks
        let account_metas = vec![ExtraAccountMeta::new_with_pubkey(
            &ctx.accounts.token_manager.key(),
            false, // is_signer
            false, // is_writable
        )?];

        // Calculate account size for meta list
        let account_size = ExtraAccountMetaList::size_of(account_metas.len())?;
        let meta_list_lamports = rent.minimum_balance(account_size);

        // Create the account for the meta list
        let meta_list_seeds = &[
            b"extra-account-metas",
            token_mint_key.as_ref(),
            &[ctx.bumps.extra_account_meta_list],
        ];
        let meta_list_signer = &[&meta_list_seeds[..]];
        invoke_signed(
            &system_instruction::create_account(
                &ctx.accounts.signer.key(),
                &ctx.accounts.extra_account_meta_list.key(),
                meta_list_lamports,
                account_size as u64,
                ctx.program_id,
            ),
            &[
                ctx.accounts.signer.to_account_info(),
                ctx.accounts.extra_account_meta_list.to_account_info(),
                ctx.accounts.system_program.to_account_info(),
            ],
            meta_list_signer,
        )?;

        // Initialize the meta list data
        let mut data = ctx.accounts.extra_account_meta_list.try_borrow_mut_data()?;
        ExtraAccountMetaList::init::<ExecuteInstruction>(&mut data, &account_metas)?;

        // 9. Store the token in the token manager
        let current_index = ctx.accounts.token_manager.current_token_index.clone();
        ctx.accounts.token_manager.tokens.push(TokenShare {
            mint: *token_mint_key,
            isin: isin,
            index: current_index,
        });
        ctx.accounts.token_manager.current_token_index = current_index
        .checked_add(1)
        .ok_or(error!(TokenManagerError::IndexOverflow))?;

        Ok(())
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
                wallet: wallet,
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
                .position(|auth| auth.mint == token.mint && auth.wallet == wallet)
            {
                ctx.accounts.token_manager.whitelist.remove(*index);
                return Ok(());
            }
            return Err(error!(TokenManagerError::WalletNotFound));
        }
        Err(error!(TokenManagerError::TokenNotFound))
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
        seeds = [b"extra-account-metas", mint.key().as_ref()],
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

    #[interface(spl_transfer_hook_interface::execute)]
    pub fn transfer_hook(ctx: Context<TransferHook>) -> Result<()> {
        let mint_key = ctx.accounts.mint.key();
        let destination_owner = ctx.accounts.destination_token.owner;

        if let Some(_) = &ctx
            .accounts
            .token_manager
            .whitelist
            .iter()
            .find(|auth| auth.mint == mint_key && auth.wallet == destination_owner)
        {
            return Ok(());
        }

        Err(error!(TokenManagerError::TransferNotAllowed))
    }

    /// Structure for the mint_tokens instruction
    #[derive(Accounts)]
    #[instruction(token_index: u64)]
    pub struct MintToken<'info> {
        /// The wallet signing the transaction
        #[account(mut)]
        pub signer: Signer<'info>,

        /// Account storing token metadata and whitelist information
        #[account(
            mut,
            seeds = [b"token-manager", signer.key().as_ref()],
            bump,
        )]
        pub token_manager: Account<'info, TokenManager>,

        /// The token mint - with seeds derived from token-manager + index
        #[account(
            mut,
            seeds = [b"token-mint", token_manager.key().as_ref(), &token_index.to_le_bytes()],
            bump,
        )]
        pub token_mint: InterfaceAccount<'info, Mint>,

        /// The account receiving the tokens
        #[account(mut)]
        pub destination: InterfaceAccount<'info, TokenAccount>,

        /// The Token 2022 program
        pub token_program: Program<'info, Token2022>,
    }

    pub fn mint_tokens(ctx: Context<MintToken>, token_index: u64, amount: u64) -> Result<()> {
        // Verify the signer is the creator of the token manager
        if ctx.accounts.signer.key() != ctx.accounts.token_manager.creator {
            return Err(error!(TokenManagerError::Unauthorized));
        }

        let token_mint_bump = ctx.bumps.token_mint;
        let token_manager_key = ctx.accounts.token_manager.key();
        let token_mint_seeds = &[
            b"token-mint",
            token_manager_key.as_ref(),
            &token_index.to_le_bytes(),
            &[token_mint_bump],
        ];
        let token_mint_signer = &[&token_mint_seeds[..]];

        let cpi_accounts = MintTo {
            mint: ctx.accounts.token_mint.to_account_info(),
            to: ctx.accounts.destination.to_account_info(),
            authority: ctx.accounts.token_mint.to_account_info(),
        };

        mint_to(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                cpi_accounts,
                token_mint_signer,
            ),
            amount,
        )?;

        Ok(())
    }
}

// Calculate metadata space based on actual content
fn calculate_metadata_space(name: &String, symbol: &String, uri: &String) -> usize {
    // Base metadata header size (approximate)
    let header_size = 32;

    // Space for each field includes:
    // - Field type identifier (1 byte)
    // - Length prefix (typically 4 bytes)
    // - The string content itself
    // - Potential padding for alignment (up to 8 bytes worst case)

    let name_size = 1 + 4 + name.len() + 8;
    let symbol_size = 1 + 4 + symbol.len() + 8;
    let uri_size = 1 + 4 + uri.len() + 8;

    // Add some buffer for additional metadata fields that might be added
    // (e.g., standard fields like "decimals" or custom fields)
    let additional_fields_buffer = 256;

    header_size + name_size + symbol_size + uri_size + additional_fields_buffer
}

#[account]
#[derive(InitSpace)]
pub struct TokenShare {
    pub index: u64,
    #[max_len(12)]
    pub isin: String,
    pub mint: Pubkey,
}

#[account]
#[derive(InitSpace)]
pub struct Authorization {
    pub mint: Pubkey,
    pub wallet: Pubkey,
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
