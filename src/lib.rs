use block::Block;
use ledger::Ledger;
use num_bigint::BigUint;
use rand::thread_rng;
use rsa::{pss::{SigningKey, VerifyingKey}, sha2::Sha256, RsaPublicKey};
use rsa::signature::Keypair;

pub mod block;
pub mod blockchain;
pub mod draw;
pub mod ledger;
pub mod transaction;

pub const TRANSACTION_FEE: u64 = 1;
pub const BLOCK_REWARD: u64 = 50;
pub const ROOT_AMOUNT: u64 = 300;

pub(crate) type Timeslot = u64;
pub(crate) type Address = VerifyingKey<Sha256>;

pub(crate) fn generate_keypair() -> (SigningKey<Sha256>, VerifyingKey<Sha256>) {
    let mut rng = thread_rng();

    #[cfg(not(feature = "small_key"))]
    const BITS: usize = 2048;
    #[cfg(feature = "small_key")]
    const BITS: usize = 1024;

    let signing_key = SigningKey::random(&mut rng, BITS).unwrap();
    let verifying_key = signing_key.verifying_key();
    (signing_key, verifying_key)
}

fn is_winner(ledger: &Ledger, block: &Block, wallet: &RsaPublicKey) -> bool {
    #[cfg(feature = "always_win")]
    return true;

    let balance = BigUint::from(ledger.get_balance(&wallet));
    let total_money = ledger.get_total_money_in_ledger();

    let max_hash = BigUint::from(2u64).pow(256);

    // the entire network has a total 10% chance of beating this at a given timeslot
    let hardness = BigUint::from(10421u64) * (BigUint::from(10u64).pow(73));

    // we must map the draw value which is in [0, 2^256] to [0, h + c(2^256 - h)] where h is hardness and c is the ratio of money we have
    // we can map this by multiplying the draw with (h + c(2^256 - h))/(2^256)
    // we can describe c as balance/total_money. Therefore we can multiply total_money to the hardness and write the multiplication factor as:
    let mult_factor =
        (hardness.clone() * total_money) + (balance * (max_hash.clone() - hardness.clone()));

    // We win if we have a good draw and a big enough fraction of the money
    block.draw.value.clone() * mult_factor > hardness * total_money * max_hash.clone()
}


#[cfg(test)]
mod tests {
    use crate::{blockchain::Blockchain, draw::Draw, transaction::Transaction};

    use super::*;

    #[test]
    fn test_draw_verify() {
        let (sk, vk) = generate_keypair();
        let (_, vk2) = generate_keypair();
        let draw = Draw::new(0, vk.clone(), &sk, [0; 32]);
        assert!(draw.verify());

        let draw = Draw::new(0, vk2.clone(), &sk, [0; 32]);
        assert!(!draw.verify());
    }

    #[test]
    fn test_transaction_verify() {
        let (sk, vk) = generate_keypair();

        let from = vk.clone();
        let to = generate_keypair().1;
        let amount = 50;
        let timeslot: Timeslot = 0;
        let transaction = Transaction::new(from.clone(), to.clone(), &sk, amount, timeslot);

        assert!(transaction.verify_signature());
    }

    #[test]
    fn test_block_verify() {
        let (sk, vk) = generate_keypair();

        let from = vk.clone();
        let to = generate_keypair().1;
        let amount = 50;
        let transaction = Transaction::new(from.clone(), to.clone(), &sk, amount, 0);
        let transactions = vec![transaction];

        // Create a block
        let block = Block::new(0, [0; 32], 0, vk.clone(), transactions.clone(), &sk);

        assert!(block.verify_signature());
    }

    #[test]
    fn test_ledger() {
        let (sk, vk) = generate_keypair();
        let (_, vk2) = generate_keypair();
        let (_, vk3) = generate_keypair();

        let from = vk.clone();
        let from_rsa: RsaPublicKey = from.clone().into();
        let to = vk2.clone();
        let to_rsa: RsaPublicKey = to.clone().into();
        let transaction = Transaction::new(from.clone(), to.clone(), &sk, 50, 0);

        let mut ledger = Ledger::new();
        ledger.reward_winner(from.as_ref(), 102);
        assert!(ledger.process_transaction(&transaction));

        assert_eq!(ledger.get_balance(&from_rsa), 51);
        assert_eq!(ledger.get_balance(&to_rsa), 50);

        let transaction = Transaction::new(from.clone(), to.clone(), &sk, 50, 1);
        assert!(ledger.process_transaction(&transaction));

        assert_eq!(ledger.get_balance(&from_rsa), 0);
        assert_eq!(ledger.get_balance(&to_rsa), 100);

        ledger.rollback_transaction(&transaction);
        assert_eq!(ledger.get_balance(&from_rsa), 51);
        assert_eq!(ledger.get_balance(&to_rsa), 50);

        assert!(ledger.process_transaction(&transaction));
        assert!(!ledger.process_transaction(&transaction));
        ledger.rollback_transaction(&transaction);

        // ensure that the both have enough balance
        ledger.reward_winner(&from_rsa, 100);
        ledger.reward_winner(&vk3.clone().into(), 100);

        let transaction = Transaction::new(vk3.clone(), from.clone(), &sk, 50, 2);

        assert!(!ledger.process_transaction(&transaction)); // invalid signature
    }

    #[cfg(feature = "always_win")]
    #[test]
    fn test_blockchain_rollback() {
        let (sk1, vk1) = generate_keypair();
        let (sk2, vk2) = generate_keypair();
        let (_, vk3) = generate_keypair();
        let (_, vk4) = generate_keypair();

        // _b1_1 refers to branch 1, depth 1

        let mut blockchain = Blockchain::start(
            vec![
                vk1.clone().into(),
                vk2.clone().into(),
                vk3.clone().into(),
                vk4.clone().into(),
            ],
            &sk1,
        );

        assert!(blockchain.verify_chain());

        let transaction_b1_1 = Transaction::new(vk1.clone(), vk3.clone(), &sk1, 10, 0);
        let transaction_b1_2 = Transaction::new(vk1.clone(), vk3.clone(), &sk1, 10, 1);

        let transaction_b2_1 = Transaction::new(vk1.clone(), vk4.clone(), &sk1, 20, 0);

        let block_b1_1 = Block::new(
            1,
            blockchain.best_path_head.0,
            1,
            vk2.clone(),
            vec![transaction_b1_1],
            &sk2,
        );
        assert!(block_b1_1.verify_signature());
        let block_b2_1 = Block::new(
            1,
            blockchain.best_path_head.0,
            1,
            vk2.clone(),
            vec![transaction_b2_1],
            &sk2,
        );
        assert!(block_b2_1.verify_signature());
        let block_b1_2 = Block::new(
            2,
            block_b1_1.hash,
            2,
            vk2.clone(),
            vec![transaction_b1_2],
            &sk2,
        );
        assert!(block_b1_2.verify_signature());

        blockchain.blocks.push(HashMap::new());

        assert!(blockchain.add_block(block_b2_1.clone())); // this is always true, since we increase the depth
        if blockchain.add_block(block_b1_1.clone()) {
            // in case of a rollback
            assert_eq!(
                *blockchain.ledger.map.get(&vk1.clone().into()).unwrap(),
                ROOT_AMOUNT - 10 - TRANSACTION_FEE
            )
        } else {
            // in case of no rollback so still b2_1 state
            assert_eq!(
                *blockchain.ledger.map.get(&vk1.clone().into()).unwrap(),
                ROOT_AMOUNT - 20 - TRANSACTION_FEE
            )
        }

        assert!(blockchain.add_block(block_b1_2.clone())); // this will always be true, it may or may not cause a rollback
                                                           // so now the ledger follows b1_2,
                                                           // if we then add b2_2 and b2_3 there must be a rollback
        let transaction_b2_2 = Transaction::new(vk1.clone(), vk4.clone(), &sk1, 20, 1);
        let block_b2_2 = Block::new(
            2,
            block_b2_1.hash,
            2,
            vk2.clone(),
            vec![transaction_b2_2],
            &sk2,
        );
        let transaction_b2_3 = Transaction::new(vk1.clone(), vk4.clone(), &sk1, 20, 2);
        let block_b2_3 = Block::new(
            3,
            block_b2_2.hash,
            3,
            vk2.clone(),
            vec![transaction_b2_3],
            &sk2,
        );
        blockchain.add_block(block_b2_2);

        assert!(blockchain.verify_chain());

        assert!(blockchain.add_block(block_b2_3));

        // now we check the ledgers state
        assert_eq!(
            *blockchain.ledger.map.get(&vk1.clone().into()).unwrap(),
            ROOT_AMOUNT - 60 - 3 * TRANSACTION_FEE
        );
        assert_eq!(
            *blockchain.ledger.map.get(&vk4.clone().into()).unwrap(),
            ROOT_AMOUNT + 60
        );

        assert!(blockchain.verify_chain());
    }

    #[cfg(feature = "heavy_test")]
    #[test]
    fn test_stake() {
        // this tests that staking works well
        let (_, vk1) = generate_keypair();
        let (_, vk2) = generate_keypair();
        let (_, vk3) = generate_keypair();

        for i in (1..=30).rev() {
            let (sk, vk) = generate_keypair();
            let mut blockchain = Blockchain::start(
                vec![
                    vk.clone().into(),
                    vk1.clone().into(),
                    vk2.clone().into(),
                    vk3.clone().into(),
                ],
                &sk,
            );
            let mut block = Block::new(
                0,
                blockchain.best_path_head.0,
                1,
                vk.clone().into(),
                Vec::new(),
                &sk,
            );
            let mut tries_vec = Vec::new();
            print!("{i} tries: ");
            for _ in 0..10 {
                block.increment_timeslot();
                block.set_draw(&sk);

                *blockchain.ledger.map.get_mut(&vk.clone().into()).unwrap() = 10 * i;
                let mut has_won = blockchain.stake(&block, &vk.clone().into());
                let mut tries = 0;
                while !has_won {
                    block.increment_timeslot();
                    block.set_draw(&sk);
                    has_won = blockchain.stake(&block, &vk.clone().into());
                    tries += 1;
                }

                print!("{} ", tries);
                tries_vec.push(tries);
            }
            println!(
                " Mean: {}",
                tries_vec.iter().sum::<i64>() as f64 / (tries_vec.len() as f64)
            );
            block.sign_and_rehash(&sk);
            assert!(blockchain.add_block(block));
        }
    }

    #[cfg(feature = "always_win")]
    #[test]
    fn test_orphanage() {
        let (sk1, vk1) = generate_keypair();
        let (sk2, vk2) = generate_keypair();
        let (_, vk3) = generate_keypair();
        let (_, vk4) = generate_keypair();

        // _b1_1 refers to branch 1, depth 1

        let mut blockchain = Blockchain::start(
            vec![
                vk1.clone().into(),
                vk2.clone().into(),
                vk3.clone().into(),
                vk4.clone().into(),
            ],
            &sk1,
        );

        let transaction_b1_1 = Transaction::new(vk1.clone(), vk3.clone(), &sk1, 10, 0);

        let transaction_b2_1 = Transaction::new(vk1.clone(), vk4.clone(), &sk1, 20, 0);
        let transaction_b2_2 = Transaction::new(vk1.clone(), vk4.clone(), &sk1, 20, 1);

        let block_b1_1 = Block::new(
            1,
            blockchain.best_path_head.0,
            1,
            vk2.clone(),
            vec![transaction_b1_1],
            &sk2,
        );

        let block_b2_1 = Block::new(
            1,
            blockchain.best_path_head.0,
            1,
            vk2.clone(),
            vec![transaction_b2_1],
            &sk2,
        );

        // this will be added first so it is an orphan
        let block_b2_2 = Block::new(
            2,
            block_b2_1.hash,
            2,
            vk2.clone(),
            vec![transaction_b2_2],
            &sk2,
        );

        assert!(blockchain.verify_chain());

        assert!(blockchain.add_block(block_b1_1));
        assert!(blockchain.orphans.is_empty());

        assert!(blockchain.verify_chain());

        assert!(!blockchain.add_block(block_b2_2));
        assert_eq!(blockchain.orphans.len(), 1);

        assert!(blockchain.verify_chain());

        assert!(blockchain.add_block(block_b2_1));
        assert!(blockchain.orphans.is_empty());
        assert_eq!(
            blockchain.ledger.get_balance(&vk1.clone().into()),
            ROOT_AMOUNT - 40 - 2 * TRANSACTION_FEE
        );
        assert!(blockchain.verify_chain());
    }

    #[test]
    fn test_illegal_genesis_block() {
        let (sk1, vk1) = generate_keypair();
        let (_, vk2) = generate_keypair();
        let (_, vk3) = generate_keypair();
        let (_, vk4) = generate_keypair();

        let mut blockchain = Blockchain::start(
            vec![
                vk1.clone().into(),
                vk2.clone().into(),
                vk3.clone().into(),
                vk4.clone().into(),
            ],
            &sk1,
        );

        assert!(blockchain.verify_chain());

        let zero_map = blockchain.blocks.get_mut(0).unwrap();
        assert_eq!(zero_map.len(), 1);
        let genesis_block = zero_map.get_mut(&blockchain.best_path_head.0).unwrap();
        genesis_block.depth = 1;

        assert!(!blockchain.verify_chain());
    }

    #[test]
    fn test_illegal_transaction() {
        let (sk1, vk1) = generate_keypair();
        let (_, vk2) = generate_keypair();
        let (_, vk3) = generate_keypair();
        let (_, vk4) = generate_keypair();

        let mut blockchain = Blockchain::start(
            vec![
                vk1.clone().into(),
                vk2.clone().into(),
                vk3.clone().into(),
                vk4.clone().into(),
            ],
            &sk1,
        );

        assert!(blockchain.verify_chain());

        let zero_map = blockchain.blocks.get_mut(0).unwrap();
        assert_eq!(zero_map.len(), 1);
        let genesis_block = zero_map.get_mut(&blockchain.best_path_head.0).unwrap();
        genesis_block.transactions = vec![Transaction::new(vk1.clone(), vk1, &sk1, 4, 0)];
        assert!(!blockchain.verify_chain());
    }

    #[cfg(feature = "always_win")]
    #[test]
    fn test_illegal_ledger() {
        let (sk1, vk1) = generate_keypair();
        let (_, vk2) = generate_keypair();
        let (_, vk3) = generate_keypair();
        let (_, vk4) = generate_keypair();

        let mut blockchain = Blockchain::start(vec![
            vk1.clone().into(),
            vk2.clone().into(),
            vk3.clone().into(),
            vk4.clone().into(),
        ], &sk1);

        let mut block = Block::new(1, blockchain.best_path_head.0, 1, vk1.clone(), Vec::new(), &sk1);
        loop {
            if blockchain.stake(&block, vk1.as_ref()) {
                break;
            } else { 
                block.increment_timeslot();
            }
        }

        assert!(blockchain.add_block(block));
    
        assert!(blockchain.verify_chain());
        blockchain.ledger.reward_winner(vk1.as_ref(), 50);
        assert!(!blockchain.verify_chain());
    }

    #[cfg(not(feature = "always_win"))]
    #[test]
    fn test_illegal_block() {
        use crate::blockchain::Blockchain;

        let (sk1, vk1) = generate_keypair();
        let (_, vk2) = generate_keypair();
        let (_, vk3) = generate_keypair();
        let (_, vk4) = generate_keypair();

        let mut blockchain = Blockchain::start(vec![
            vk1.clone().into(),
            vk2.clone().into(),
            vk3.clone().into(),
            vk4.clone().into(),
        ], &sk1);

        let mut block = Block::new(1, blockchain.best_path_head.0, 1, vk1.clone(), Vec::new(), &sk1);
        loop {
            if blockchain.stake(&block, vk1.as_ref()) {
                block.increment_timeslot();
            } else { 
                break;
            }
        }

        assert!(blockchain.verify_chain());
        blockchain.add_block(block);
        assert!(!blockchain.verify_chain());
    }
}