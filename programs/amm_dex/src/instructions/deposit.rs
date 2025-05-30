use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token::{mint_to, transfer, Mint, MintTo, Token, TokenAccount, Transfer},
};

use crate::{error::AmmError, state::Config};
use constant_product_curve::ConstantProduct;

#[derive(Accounts)]
pub struct Deposit<'info> {
    #[account(mut)]
    pub user: Signer<'info>,

    pub mint_x: Account<'info, Mint>,
    pub mint_y: Account<'info, Mint>,

    #[account(
        mut,
        associated_token::mint = mint_x,
        associated_token::authority = user
    )]
    pub user_x: Account<'info, TokenAccount>, // Box to move to heap

    #[account(
        mut, // Added mut since you transfer from this account
        associated_token::mint = mint_y,
        associated_token::authority = user
    )]
    pub user_y: Account<'info, TokenAccount>, // Box to move to heap

    #[account(
        mut,
        associated_token::mint = mint_x,
        associated_token::authority = config
    )]
    pub vault_x: Account<'info, TokenAccount>, // Box to move to heap

    #[account(
        mut,
        associated_token::mint = mint_y,
        associated_token::authority = config,
    )]
    pub vault_y: Account<'info, TokenAccount>, // Box to move to heap

    #[account(
        seeds = [b"config", config.seed.to_le_bytes().as_ref()], // Fixed seeds to match initialize
        bump = config.config_bump,
        has_one = mint_x,
        has_one = mint_y
    )]
    pub config: Account<'info, Config>,

    #[account(
        mut,
        seeds = [b"lp", config.key().as_ref()],
        bump = config.lp_bump
    )]
    pub mint_lp: Account<'info, Mint>, // Box to move to heap

    #[account(
        // init_if_needed, // avoiding these will create an ATA on client side
        // payer = user,
        mut,
        associated_token::mint = mint_lp,
        associated_token::authority = user
    )]
    pub user_lp: Account<'info, TokenAccount>, // Box to move to heap

    pub associated_token_program: Program<'info, AssociatedToken>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

impl<'info> Deposit<'info> {
    pub fn deposit(&mut self, amount: u64, max_x: u64, max_y: u64) -> Result<()> {
        require!(self.config.locked == false, AmmError::PoolLocked);

        require!(amount > 0, AmmError::InvalidAmount);

        let (x, y) =
            if self.mint_lp.supply == 0 && self.vault_x.amount == 0 && self.vault_y.amount == 0 {
                (max_x, max_y)
            } else {
                let amounts = ConstantProduct::xy_deposit_amounts_from_l(
                    self.vault_x.amount,
                    self.vault_y.amount,
                    self.mint_lp.supply,
                    amount,
                    6,
                )
                .unwrap();
                (amounts.x, amounts.y)
            };

        require!(x <= max_x && y <= max_y, AmmError::SlippageExceeded);

        self.deposit_tokens(true, x)?;
        self.deposit_tokens(false, y)?;
        self.mint_lp_tokens(amount)
    }

    pub fn deposit_tokens(&mut self, is_x: bool, amount: u64) -> Result<()> {
        let (from, to) = if is_x {
            (
                self.user_x.to_account_info(),
                self.vault_x.to_account_info(),
            )
        } else {
            (
                self.user_y.to_account_info(),
                self.vault_y.to_account_info(),
            )
        };

        let cpi_program = self.token_program.to_account_info();
        let cpi_accounts = Transfer {
            from,
            to,
            authority: self.user.to_account_info(),
        };

        let ctx = CpiContext::new(cpi_program, cpi_accounts);
        transfer(ctx, amount)
    }

    pub fn mint_lp_tokens(&mut self, amount: u64) -> Result<()> {
        let cpi_program = self.token_program.to_account_info();

        let cpi_accounts = MintTo {
            mint: self.mint_lp.to_account_info(),
            to: self.user_lp.to_account_info(),
            authority: self.config.to_account_info(),
        };

        let seeds = &[
            b"config",
            &self.config.seed.to_le_bytes()[..],
            &[self.config.config_bump],
        ];

        let signer_seeds = &[&seeds[..]];

        let ctx = CpiContext::new_with_signer(cpi_program, cpi_accounts, signer_seeds);
        mint_to(ctx, amount)?;
        Ok(())
    }
}