// Copyright (c) 2018-2020 MobileCoin Inc.

//! Utilities for converting between `mobilecoind` and `mobilecoind_api` data types.

use crate::{
    payments::{Outlay, TxProposal},
    utxo_store::UnspentTxOut,
};

use common::HashMap;
use mobilecoind_api::{self, ConversionError};
use protobuf::RepeatedField;
use std::{convert::TryFrom, iter::FromIterator};
use transaction::{
    account_keys::PublicAddress,
    ring_signature::KeyImage,
    tx::{Tx, TxOut},
};

impl From<&UnspentTxOut> for mobilecoind_api::UnspentTxOut {
    fn from(src: &UnspentTxOut) -> Self {
        let mut dst = Self::new();

        dst.set_tx_out((&src.tx_out).into());
        dst.set_subaddress_index(src.subaddress_index);
        dst.set_key_image((&src.key_image).into());
        dst.set_value(src.value);
        dst.set_attempted_spend_height(src.attempted_spend_height);
        dst.set_attempted_spend_tombstone(src.attempted_spend_tombstone);

        dst
    }
}

impl TryFrom<&mobilecoind_api::UnspentTxOut> for UnspentTxOut {
    type Error = ConversionError;

    fn try_from(src: &mobilecoind_api::UnspentTxOut) -> Result<Self, Self::Error> {
        let tx_out = TxOut::try_from(src.get_tx_out())?;
        let subaddress_index = src.subaddress_index;
        let key_image = KeyImage::try_from(src.get_key_image())?;
        let value = src.value;
        let attempted_spend_height = src.attempted_spend_height;
        let attempted_spend_tombstone = src.attempted_spend_tombstone;

        Ok(Self {
            tx_out,
            subaddress_index,
            key_image,
            value,
            attempted_spend_height,
            attempted_spend_tombstone,
        })
    }
}

impl From<&Outlay> for mobilecoind_api::Outlay {
    fn from(src: &Outlay) -> Self {
        let mut dst = Self::new();

        dst.set_value(src.value);
        dst.set_receiver((&src.receiver).into());

        dst
    }
}

impl TryFrom<&mobilecoind_api::Outlay> for Outlay {
    type Error = ConversionError;

    fn try_from(src: &mobilecoind_api::Outlay) -> Result<Self, Self::Error> {
        let value = src.value;
        let receiver = PublicAddress::try_from(src.get_receiver())?;

        Ok(Self { value, receiver })
    }
}

impl From<&TxProposal> for mobilecoind_api::TxProposal {
    fn from(src: &TxProposal) -> mobilecoind_api::TxProposal {
        let mut dst = mobilecoind_api::TxProposal::new();

        dst.set_input_list(RepeatedField::from_vec(
            src.utxos.iter().map(|utxo| utxo.into()).collect(),
        ));
        dst.set_outlay_list(RepeatedField::from_vec(
            src.outlays.iter().map(|outlay| outlay.into()).collect(),
        ));
        dst.set_tx((&src.tx).into());
        dst.set_fee(src.tx.prefix.fee);
        dst.set_outlay_index_to_tx_out_index(std::collections::HashMap::from_iter(
            src.outlay_index_to_tx_out_index
                .iter()
                .map(|(key, val)| (*key as u64, *val as u64)),
        ));

        dst
    }
}

impl TryFrom<&mobilecoind_api::TxProposal> for TxProposal {
    type Error = ConversionError;

    fn try_from(src: &mobilecoind_api::TxProposal) -> Result<Self, Self::Error> {
        if src.fee != src.get_tx().get_prefix().fee {
            return Err(ConversionError::FeeMismatch);
        }

        let utxos = src
            .get_input_list()
            .iter()
            .map(UnspentTxOut::try_from)
            .collect::<Result<Vec<UnspentTxOut>, ConversionError>>()?;

        let outlays = src
            .get_outlay_list()
            .iter()
            .map(Outlay::try_from)
            .collect::<Result<Vec<Outlay>, ConversionError>>()?;

        let tx = Tx::try_from(src.get_tx())?;

        let outlay_index_to_tx_out_index = HashMap::from_iter(
            src.get_outlay_index_to_tx_out_index()
                .iter()
                .map(|(key, val)| (*key as usize, *val as usize)),
        );

        // Check that none of the indices are out of bound.
        if outlay_index_to_tx_out_index.len() != outlays.len() {
            return Err(ConversionError::IndexOutOfBounds);
        }

        for (outlay_index, tx_out_index) in outlay_index_to_tx_out_index.iter() {
            if *outlay_index >= outlays.len() || *tx_out_index >= tx.prefix.outputs.len() {
                return Err(ConversionError::IndexOutOfBounds);
            }
        }

        Ok(Self {
            utxos,
            outlays,
            tx,
            outlay_index_to_tx_out_index,
        })
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use keys::{FromRandom, RistrettoPublic};
    use ledger_db::Ledger;
    use rand::{rngs::StdRng, SeedableRng};
    use transaction::{account_keys::AccountKey, amount::Amount, ring_signature::Blinding};
    use transaction_test_utils::{create_ledger, create_transaction, initialize_ledger};

    #[test]
    fn test_unspent_tx_out_conversion() {
        let mut rng: StdRng = SeedableRng::from_seed([1u8; 32]);

        // Rust -> Proto
        let tx_out = TxOut {
            amount: Amount::new(
                1u64 << 13,
                Blinding::from(9u64),
                &RistrettoPublic::from_random(&mut rng),
            )
            .unwrap(),
            target_key: RistrettoPublic::from_random(&mut rng).into(),
            public_key: RistrettoPublic::from_random(&mut rng).into(),
            e_account_hint: (&[0u8; 128]).into(),
        };

        let subaddress_index = 123;
        let key_image = KeyImage::from(456);
        let value = 789;
        let attempted_spend_height = 1000;
        let attempted_spend_tombstone = 1234;

        let rust = UnspentTxOut {
            tx_out: tx_out.clone(),
            subaddress_index,
            key_image: key_image.clone(),
            value,
            attempted_spend_height,
            attempted_spend_tombstone,
        };

        let proto = mobilecoind_api::UnspentTxOut::from(&rust);

        assert_eq!(tx_out, TxOut::try_from(proto.get_tx_out()).unwrap());
        assert_eq!(subaddress_index, proto.subaddress_index);
        assert_eq!(
            key_image,
            KeyImage::try_from(proto.get_key_image()).unwrap()
        );
        assert_eq!(value, proto.value);
        assert_eq!(attempted_spend_height, proto.attempted_spend_height);
        assert_eq!(attempted_spend_tombstone, proto.attempted_spend_tombstone);

        // Proto -> Rust
        assert_eq!(rust, UnspentTxOut::try_from(&proto).unwrap());
    }

    #[test]
    fn test_outlay_conversion() {
        let mut rng: StdRng = SeedableRng::from_seed([1u8; 32]);
        let public_addr = AccountKey::random(&mut rng).default_subaddress();

        // Rust -> Proto
        let rust = Outlay {
            receiver: public_addr.clone(),
            value: 1234,
        };
        let proto = mobilecoind_api::Outlay::from(&rust);

        assert_eq!(proto.value, rust.value);
        assert_eq!(
            PublicAddress::try_from(proto.get_receiver()).unwrap(),
            public_addr
        );

        // Proto -> Rust
        assert_eq!(rust, Outlay::try_from(&proto).unwrap());
    }

    #[test]
    fn test_tx_proposal_conversion() {
        let mut rng: StdRng = SeedableRng::from_seed([1u8; 32]);

        let tx = {
            let mut ledger = create_ledger();
            let sender = AccountKey::random(&mut rng);
            let recipient = AccountKey::random(&mut rng);
            initialize_ledger(&mut ledger, 1, &sender, &mut rng);

            let block_contents = ledger.get_block_contents(0).unwrap();
            let tx_out = block_contents.outputs[0].clone();

            create_transaction(
                &mut ledger,
                &tx_out,
                &sender,
                &recipient.default_subaddress(),
                10,
                &mut rng,
            )
        };

        let utxo = {
            let tx_out = TxOut {
                amount: Amount::new(
                    1u64 << 13,
                    Blinding::from(9u64),
                    &RistrettoPublic::from_random(&mut rng),
                )
                .unwrap(),
                target_key: RistrettoPublic::from_random(&mut rng).into(),
                public_key: RistrettoPublic::from_random(&mut rng).into(),
                e_account_hint: (&[0u8; 128]).into(),
            };

            let subaddress_index = 123;
            let key_image = KeyImage::from(456);
            let value = 789;
            let attempted_spend_height = 1000;
            let attempted_spend_tombstone = 1234;

            UnspentTxOut {
                tx_out: tx_out.clone(),
                subaddress_index,
                key_image: key_image.clone(),
                value,
                attempted_spend_height,
                attempted_spend_tombstone,
            }
        };

        let outlay = {
            let public_addr = AccountKey::random(&mut rng).default_subaddress();
            Outlay {
                receiver: public_addr.clone(),
                value: 1234,
            }
        };

        let outlay_index_to_tx_out_index = HashMap::from_iter(vec![(0, 0)]);

        // Rust -> Proto
        let rust = TxProposal {
            utxos: vec![utxo],
            outlays: vec![outlay],
            tx,
            outlay_index_to_tx_out_index,
        };

        let proto = mobilecoind_api::TxProposal::from(&rust);

        assert_eq!(
            rust.utxos,
            vec![UnspentTxOut::try_from(&proto.get_input_list()[0]).unwrap()],
        );

        assert_eq!(
            rust.outlays,
            vec![Outlay::try_from(&proto.get_outlay_list()[0]).unwrap()],
        );

        assert_eq!(proto.get_outlay_index_to_tx_out_index().len(), 1);
        assert_eq!(proto.get_outlay_index_to_tx_out_index().get(&0), Some(&0));

        assert_eq!(rust.tx, Tx::try_from(proto.get_tx()).unwrap());

        // Proto -> Rust
        assert_eq!(rust, TxProposal::try_from(&proto).unwrap());
    }
}
