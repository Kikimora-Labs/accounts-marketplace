use crate::*;

pub const OFFER_DEPOSIT: Balance = 450_000_000_000_000_000_000_000;
pub const INIT_BET_PRICE: Balance = 500_000_000_000_000_000_000_000;

pub const INV_COMMISSION: u128 = 20;
pub const INV_REWARD_DECAY_MULT_100: u128 = 144;

pub const ERR_OFFER_DEPOSIT_NOT_ENOUGH: &str =
    "Attached deposit must be no less than OFFER_DEPOSIT";
pub const ERR_GAINER_SAME_AS_OFFER: &str = "Offered account cannot take profit";
pub const ERR_ALREADY_OFFERED: &str = "Bid is already offered";
pub const ERR_BET_FORFEIT_NOT_ENOUGH: &str =
    "Attached deposit must be no less than bet price plus forfeit";
pub const ERR_CLAIM_NOT_ENOUGH: &str = "Attached deposit must be no less than claim price";
pub const ERR_ALREADY_CLAIMED: &str = "Bid is already claimed";
pub const ERR_BID_NOT_FOUND: &str = "Bid is not found";
pub const ERR_BID_CLAIM_NOT_FOUND: &str = "Bid claim is not found";
pub const ERR_BET_ON_ACQUISITION: &str = "Bid is on acquisition";
pub const ERR_NOT_ON_ACQUISITION: &str = "Bid is not on acquisition";
pub const ERR_ACQUIRE_REJECTED: &str = "Do not have permission to acquire";

#[near_bindgen]
impl Contract {
    #[payable]
    pub fn offer(&mut self, profile_id: ValidAccountId) -> bool {
        assert!(
            env::attached_deposit() >= OFFER_DEPOSIT,
            "{}",
            ERR_OFFER_DEPOSIT_NOT_ENOUGH
        );
        assert_ne!(
            &env::predecessor_account_id(),
            profile_id.as_ref(),
            "{}",
            ERR_GAINER_SAME_AS_OFFER
        );

        // Create bid
        let mut bid = self.extract_bid_or_create(&env::predecessor_account_id());
        assert_eq!(bid.bets.len(), 0, "{}", ERR_ALREADY_OFFERED);
        bid.participants.insert(profile_id.as_ref());

        // Update profile
        let mut profile = self.extract_profile_or_create(profile_id.as_ref());
        profile.num_offers += 1;
        profile.participation.insert(&env::predecessor_account_id());
        self.save_profile_or_panic(profile_id.as_ref(), &profile);

        // Update top
        self.bet_and_update_leaders(
            profile_id.as_ref(),
            &env::predecessor_account_id(),
            &mut bid,
        );

        true
    }

    #[payable]
    pub fn bet(&mut self, bid_id: ValidAccountId) -> bool {
        let mut bid = self.extract_bid_or_panic(bid_id.as_ref());
        assert!(
            !bid.on_acquisition(&self.acquisition_time),
            "{}",
            ERR_BET_ON_ACQUISITION
        );
        let bet_price = bid.calculate_bet_price();
        let forfeit = bid.calculate_forfeit(&self.acquisition_time).unwrap_or(0);
        assert!(
            env::attached_deposit() >= bet_price + forfeit,
            "{}",
            ERR_BET_FORFEIT_NOT_ENOUGH
        );

        // TODO forfeit -> rewards

        // Update bid
        bid.claim_status = None;
        bid.participants.insert(&env::predecessor_account_id());

        // Update profile
        let mut profile = self.extract_profile_or_create(&env::predecessor_account_id());
        profile.num_bets += 1;
        profile.bets_volume += bet_price;
        profile.participation.insert(bid_id.as_ref());
        self.save_profile_or_panic(&env::predecessor_account_id(), &profile);

        // Update top
        self.bet_and_update_leaders(&env::predecessor_account_id(), bid_id.as_ref(), &mut bid);

        true
    }

    #[payable]
    pub fn claim(&mut self, bid_id: ValidAccountId) -> bool {
        let mut bid = self.extract_bid_or_panic(bid_id.as_ref());
        assert!(bid.claim_status.is_none(), "{}", ERR_ALREADY_CLAIMED);
        let claim_price = bid.force_calculate_claim_price();
        assert!(
            env::attached_deposit() >= claim_price,
            "{}",
            ERR_CLAIM_NOT_ENOUGH
        );

        // Update bid
        bid.claim_status = Some((env::predecessor_account_id(), env::block_timestamp()));
        bid.participants.insert(&env::predecessor_account_id());
        self.save_bid_or_panic(bid_id.as_ref(), &bid);

        // Update profile
        let mut profile = self.extract_profile_or_create(&env::predecessor_account_id());
        profile.num_claims += 1;
        profile.participation.insert(bid_id.as_ref());
        self.save_profile_or_panic(&env::predecessor_account_id(), &profile);

        // Update top
        self.top_claims.insert(&(claim_price, bid_id.into()), &());

        true
    }

    pub fn finalize(&mut self, bid_id: ValidAccountId) -> bool {
        let mut bid = self.extract_bid_or_panic(bid_id.as_ref());
        assert!(
            bid.on_acquisition(&self.acquisition_time),
            "{}",
            ERR_NOT_ON_ACQUISITION
        );
        let profile_id = if let Some((profile_id, _)) = &bid.claim_status {
            profile_id
        } else {
            unreachable!()
        };

        // Update profile
        let mut profile = self.extract_profile_or_create(&profile_id);
        profile.acquisitions.insert(bid_id.as_ref());
        self.save_profile_or_panic(&profile_id, &profile);

        // Update rewards
        self.update_final_rewards(bid_id.as_ref(), &bid);

        // Update top
        self.top_bets
            .remove(&(bid.calculate_bet_price(), bid_id.clone().into()));
        self.top_claims
            .remove(&(bid.force_calculate_claim_price(), bid_id.into()));

        // Clear Bid completely from storage
        self.clear_bid(&mut bid);

        true
    }

    pub fn acquire(&mut self, bid_id: ValidAccountId, new_public_key: Base58PublicKey) -> bool {
        // Update profile
        let mut profile = self.extract_profile_or_create(&env::predecessor_account_id());
        assert!(
            profile.acquisitions.remove(bid_id.as_ref()),
            "{}",
            ERR_ACQUIRE_REJECTED
        );
        profile.num_acquisitions += 1;
        self.save_profile_or_panic(&env::predecessor_account_id(), &profile);

        // Update keys
        Promise::new(env::current_account_id())
            .add_full_access_key(new_public_key.into())
            .then({
                // This Promise may fail by design
                Promise::new(env::current_account_id()).delete_key(env::signer_account_pk())
            });

        true
    }
}

impl Contract {
    fn bet_and_update_leaders(&mut self, profile_id: &ProfileId, bid_id: &BidId, bid: &mut Bid) {
        let mut bet_price = bid.calculate_bet_price();
        if bid.bets.len() == 0 {
            // Offer
            self.update_commission(OFFER_DEPOSIT);
        } else {
            // Bet
            self.top_bets
                .remove(&(bet_price, bid_id.clone()))
                .expect(ERR_BID_NOT_FOUND);
            self.top_claims.remove(&(bet_price * 2, bid_id.clone()));
            self.update_commission(bet_price / INV_COMMISSION);
            let mut paid = bet_price - bet_price / INV_COMMISSION;
            for i in (0..bid.bets.len()).rev() {
                self.update_reward(
                    &bid.bets.get(i).unwrap(),
                    &(paid / INV_REWARD_DECAY_MULT_100 * 100),
                );
                paid -= paid / INV_REWARD_DECAY_MULT_100 * 100;
            }
            self.update_reward(&bid.bets.get(0).unwrap(), &paid);
            bet_price = bet_price * 6 / 5;
        }

        bid.bets.push(&profile_id);
        self.top_bets.insert(&(bet_price, bid_id.clone()), &());
        self.save_bid_or_panic(bid_id, bid);
    }

    fn update_final_rewards(&mut self, bid_id: &BidId, bid: &Bid) {
        for profile_id in bid.participants.iter() {
            let mut profile = self.extract_profile_or_create(&profile_id);
            profile.participation.remove(bid_id);
            self.save_profile_or_panic(&profile_id, &profile);
        }
        // TODO implement
    }

    fn update_commission(&mut self, value: Balance) {
        self.total_commission += value;
    }
}