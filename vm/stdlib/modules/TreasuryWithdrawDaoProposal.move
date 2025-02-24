address 0x1 {
/// TreasuryWithdrawDaoProposal is a dao proposal for withdraw Token from Treasury.
module TreasuryWithdrawDaoProposal {
    use 0x1::Token::{Self,Token};
    use 0x1::Signer;
    use 0x1::Dao;
    use 0x1::Errors;
    use 0x1::Treasury;
    use 0x1::CoreAddresses;

    spec module {
        pragma verify = false; // break after enabling v2 compilation scheme
        pragma aborts_if_is_strict;
        pragma aborts_if_is_partial;
    }

    /// A wrapper of Token MintCapability.
    struct WrappedWithdrawCapability<TokenT> has key {
        cap: Treasury::WithdrawCapability<TokenT>,
    }

    /// WithdrawToken request.
    struct WithdrawToken has copy, drop, store {
        /// the receiver of withdraw tokens.
        receiver: address,
        /// how many tokens to mint.
        amount: u128,
        /// How long in milliseconds does it take for the token to be released
        period: u64,
    }

    const ERR_NOT_AUTHORIZED: u64 = 101;
    /// Only receiver can execute TreasuryWithdrawDaoProposal
    const ERR_NEED_RECEIVER_TO_EXECUTE: u64 = 102;

    /// Plugin method of the module.
    /// Should be called by token issuer.
    public fun plugin<TokenT: store>(signer: &signer, cap: Treasury::WithdrawCapability<TokenT>) {
        let token_issuer = Token::token_address<TokenT>();
        assert(Signer::address_of(signer) == token_issuer, Errors::requires_address(ERR_NOT_AUTHORIZED));
        move_to(signer, WrappedWithdrawCapability<TokenT> { cap: cap });
    }

    spec fun plugin {
        pragma aborts_if_is_partial = false;
        let sender = Signer::address_of(signer);
        aborts_if sender != Token::SPEC_TOKEN_TEST_ADDRESS();
        aborts_if !exists<Treasury::WithdrawCapability<TokenT>>(sender);
        aborts_if exists<WrappedWithdrawCapability<TokenT>>(sender);

        ensures !exists<Treasury::WithdrawCapability<TokenT>>(sender);
        ensures exists<WrappedWithdrawCapability<TokenT>>(sender);
    }


    /// Entrypoint for the proposal.
    public fun propose_withdraw<TokenT: copy + drop + store>(signer: &signer, receiver: address, amount: u128, period: u64, exec_delay: u64) {
        Dao::propose<TokenT, WithdrawToken>(
            signer,
            WithdrawToken { receiver, amount, period },
            exec_delay,
        );
    }
    spec fun propose_withdraw {
        use 0x1::Timestamp;
        use 0x1::CoreAddresses;
        pragma aborts_if_is_partial = false;

        // copy from Dao::propose spec.
        include Dao::AbortIfDaoConfigNotExist<TokenT>;
        include Dao::AbortIfDaoInfoNotExist<TokenT>;
        aborts_if !exists<Timestamp::CurrentTimeMilliseconds>(CoreAddresses::SPEC_GENESIS_ADDRESS());
        aborts_if exec_delay > 0 && exec_delay < Dao::spec_dao_config<TokenT>().min_action_delay;
        include Dao::CheckQuorumVotes<TokenT>;
        let sender = Signer::spec_address_of(signer);
        aborts_if exists<Dao::Proposal<TokenT, WithdrawToken>>(sender);
    }

    /// Once the proposal is agreed, anyone can call the method to make the proposal happen.
    public fun execute_withdraw_proposal<TokenT: copy + drop + store>(
        signer: &signer,
        proposer_address: address,
        proposal_id: u64,
    ) acquires WrappedWithdrawCapability {
        let WithdrawToken { receiver, amount, period } = Dao::extract_proposal_action<TokenT, WithdrawToken>(
            proposer_address,
            proposal_id,
        );
        assert(receiver == Signer::address_of(signer), Errors::requires_address(ERR_NEED_RECEIVER_TO_EXECUTE));
        let cap = borrow_global_mut<WrappedWithdrawCapability<TokenT>>(Token::token_address<TokenT>());
        let linear_cap = Treasury::issue_linear_withdraw_capability<TokenT>(&mut cap.cap, amount, period);
        Treasury::add_linear_withdraw_capability(signer, linear_cap);
    }

    spec fun execute_withdraw_proposal {
        use 0x1::Option;
        pragma aborts_if_is_partial = true;
        let expected_states = singleton_vector(6);
        include Dao::CheckProposalStates<TokenT, WithdrawToken>{expected_states};
        let proposal = global<Dao::Proposal<TokenT, WithdrawToken>>(proposer_address);
        aborts_if Option::is_none(proposal.action);
        aborts_if !exists<WrappedWithdrawCapability<TokenT>>(Token::SPEC_TOKEN_TEST_ADDRESS());
    }

    /// Provider a port for get block reward STC from Treasury, only genesis account can invoke this function.
    /// The TreasuryWithdrawCapability is locked in TreasuryWithdrawDaoProposal, and only can withdraw by DAO proposal.
    /// This approach is not graceful, but restricts the operation to genesis accounts only, so there are no security issues either.
    public fun withdraw_for_block_reward<TokenT: store>(signer: &signer, reward: u128):Token<TokenT> acquires WrappedWithdrawCapability  {
        CoreAddresses::assert_genesis_address(signer);
        let cap = borrow_global_mut<WrappedWithdrawCapability<TokenT>>(Signer::address_of(signer));
        Treasury::withdraw_with_capability(&mut cap.cap, reward)
    }
}
}