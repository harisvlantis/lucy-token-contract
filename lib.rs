use anchor_lang::prelude::*;
use anchor_spl::token::{self, Mint, Token, TokenAccount, Transfer, Burn};

#[program]
mod lucy_token {
    use super::*;

    pub fn initialize(ctx: Context<Initialize>, total_supply: u64, launch_time: u64) -> Result<()> {
        let mint = &mut ctx.accounts.mint;
        mint.decimals = 9;
        mint.supply = total_supply;
        mint.mint_authority = COption::Some(ctx.accounts.authority.key());
        let control = &mut ctx.accounts.control;
        control.launch_time = launch_time;
        control.fee_percentage = 40; // Default 40% Fee για τον πρώτο 1 μήνα
        control.vesting_start = launch_time;
        control.trading_paused = false;
        control.dynamic_fee_start = launch_time;
        control.fee_wallet = Pubkey::from_str("E5EErpBbcLJBBAJsiuvyAk5RttUCKDnpxSUN36uii8sH").unwrap();
        control.last_withdraw = launch_time;
        Ok(())
    }

    pub fn auto_withdraw(ctx: Context<AutoWithdraw>, current_time: u64) -> Result<()> {
        let control = &mut ctx.accounts.control;
        let time_since_last_withdraw = current_time - control.last_withdraw;
        require!(time_since_last_withdraw >= (7 * 24 * 60 * 60), ErrorCode::WithdrawCooldown);

        let fee_balance = ctx.accounts.fee_wallet.amount;
        require!(fee_balance > 0, ErrorCode::NoFeesToWithdraw);

        let cpi_accounts = Transfer {
            from: ctx.accounts.fee_wallet.to_account_info(),
            to: ctx.accounts.authority.to_account_info(),
            authority: ctx.accounts.authority.to_account_info(),
        };
        let cpi_program = ctx.accounts.token_program.to_account_info();
        let cpi_ctx = CpiContext::new(cpi_program, cpi_accounts);
        token::transfer(cpi_ctx, fee_balance)?;

        control.last_withdraw = current_time;
        emit!(WithdrawEvent {
            amount: fee_balance,
            timestamp: current_time,
        });
        Ok(())
    }

    pub fn update_fee_wallet(ctx: Context<UpdateFeeWallet>, new_wallet: Pubkey) -> Result<()> {
        let control = &mut ctx.accounts.control;
        control.fee_wallet = new_wallet;
        emit!(FeeWalletUpdated {
            new_wallet,
        });
        Ok(())
    }

    pub fn transfer(ctx: Context<TransferTokens>, amount: u64, current_time: u64) -> Result<()> {
        let control = &ctx.accounts.control;
        require!(!control.trading_paused, ErrorCode::TradingPaused);
        require!(current_time > ctx.accounts.from.creation_time + (48 * 60 * 60), ErrorCode::TimeLockActive);
        
        let elapsed_months = (current_time - control.dynamic_fee_start) / (30 * 24 * 60 * 60);
        let fee_percentage = if elapsed_months == 0 {
            40
        } else if elapsed_months == 1 {
            20
        } else if elapsed_months == 2 {
            10
        } else {
            1 // Μόνιμο 1% Fee μετά τον 3ο μήνα
        };
        
        let fee_amount = if ctx.accounts.exempted_accounts.load()?.contains(&ctx.accounts.from.key()) {
            0
        } else {
            amount * fee_percentage / 100
        };
        let transfer_amount = amount - fee_amount;

        if fee_amount > 0 {
            let cpi_accounts_fee = Transfer {
                from: ctx.accounts.from.to_account_info(),
                to: ctx.accounts.control.fee_wallet.to_account_info(),
                authority: ctx.accounts.authority.to_account_info(),
            };
            let cpi_program = ctx.accounts.token_program.to_account_info();
            let cpi_ctx_fee = CpiContext::new(cpi_program, cpi_accounts_fee);
            token::transfer(cpi_ctx_fee, fee_amount)?;
        }

        let cpi_accounts = Transfer {
            from: ctx.accounts.from.to_account_info(),
            to: ctx.accounts.to.to_account_info(),
            authority: ctx.accounts.authority.to_account_info(),
        };
        let cpi_program = ctx.accounts.token_program.to_account_info();
        let cpi_ctx = CpiContext::new(cpi_program, cpi_accounts);
        token::transfer(cpi_ctx, transfer_amount)
    }
}

#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(init, payer = authority, mint::decimals = 9, mint::authority = authority)]
    pub mint: Account<'info, Mint>,
    #[account(init, payer = authority, space = 8 + 48)]
    pub control: Account<'info, LaunchControl>,
    #[account(mut)]
    pub authority: Signer<'info>,
    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
    pub rent: Sysvar<'info, Rent>,
}

#[derive(Accounts)]
pub struct AutoWithdraw<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,
    #[account(mut)]
    pub control: Account<'info, LaunchControl>,
    #[account(mut)]
    pub fee_wallet: Account<'info, TokenAccount>,
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct UpdateFeeWallet<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,
    #[account(mut)]
    pub control: Account<'info, LaunchControl>,
}

#[account]
pub struct LaunchControl {
    pub launch_time: u64,
    pub fee_percentage: u64,
    pub vesting_start: u64,
    pub trading_paused: bool,
    pub dynamic_fee_start: u64,
    pub fee_wallet: Pubkey,
    pub last_withdraw: u64,
}

#[event]
pub struct WithdrawEvent {
    pub amount: u64,
    pub timestamp: u64,
}

#[event]
pub struct FeeWalletUpdated {
    pub new_wallet: Pubkey,
}

#[error_code]
pub enum ErrorCode {
    #[msg("Invalid fee percentage. Must be 0-100.")]
    InvalidFeePercentage,
    #[msg("Trading is currently paused.")]
    TradingPaused,
    #[msg("Wallet must wait 48 hours before selling.")]
    TimeLockActive,
    #[msg("No fees available to withdraw.")]
    NoFeesToWithdraw,
    #[msg("Withdraw cooldown active, try again later.")]
    WithdrawCooldown,
}
