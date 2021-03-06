// Copyright (c) 2018-2020 MobileCoin Inc.

extern crate alloc;
use alloc::vec::Vec;
use bulletproofs::RangeProof;
use curve25519_dalek::{ristretto::CompressedRistretto, scalar::Scalar};
use merlin::Transcript;
use rand_core::{CryptoRng, RngCore};

pub mod error;
use crate::ring_signature::{Blinding, BP_GENERATORS, GENERATORS};
use error::Error;

/// The domain separation label should be unique for each application.
const DOMAIN_SEPARATOR_LABEL: &[u8] = b"range_proof";

/// Create an aggregated 64-bit rangeproof for a set of values.
///
/// Creates a proof that each secret value is in the range [0,2^64).
///
/// # Arguments
/// `values` - Secret values that we want to prove are in [0,2^64).
/// `serials` - Transaction output serial numbers.
///
pub fn generate_range_proofs<T: RngCore + CryptoRng>(
    values: &[u64],
    serials: &[Blinding],
    rng: &mut T,
) -> Result<(RangeProof, Vec<CompressedRistretto>), Error> {
    // Most of this comes directly from the example at
    // https://doc-internal.dalek.rs/bulletproofs/struct.RangeProof.html#example-1

    // Aggregated rangeproofs operate on sets of `m` values, where `m` must be a power of 2.
    // If the number of inputs is not a power of 2, pad them.
    let values_padded: Vec<u64> = resize_slice_to_pow2::<u64>(values)?;
    let serials_padded: Vec<Blinding> = resize_slice_to_pow2::<Blinding>(serials)?;
    let blindings: Vec<Scalar> = serials_padded.iter().map(|s| *s.as_ref()).collect();

    // Create a 64-bit RangeProof and corresponding commitments.
    RangeProof::prove_multiple_with_rng(
        &BP_GENERATORS,
        &GENERATORS,
        &mut Transcript::new(DOMAIN_SEPARATOR_LABEL),
        &values_padded,
        &blindings,
        64,
        rng,
    )
    .map_err(Error::from)
}

/// Verifies an aggregated 64-bit RangeProof for the given value commitments.
///
/// Proves that the corresponding values lie in the range [0,2^64).
///
/// # Arguments
/// `range_proof` - A RangeProof.
/// `commitments` - Commitments to secret values that lie in the range [0,2^64).
/// `rng` - Randomness.
pub fn check_range_proofs<T: RngCore + CryptoRng>(
    range_proof: &RangeProof,
    commitments: &[CompressedRistretto],
    rng: &mut T,
) -> Result<(), Error> {
    // The length of `commitments` must be a power of 2. If not, resize it.
    let resized_commitments = resize_slice_to_pow2::<CompressedRistretto>(commitments)?;
    range_proof
        .verify_multiple_with_rng(
            &BP_GENERATORS,
            &GENERATORS,
            &mut Transcript::new(DOMAIN_SEPARATOR_LABEL),
            &resized_commitments,
            64,
            rng,
        )
        .map_err(Error::from)
}

/// Return a vector which is the slice plus enough of the final element such that
/// the length of the vector is a power of two.
///
/// If the next power of two is greater than the type's maximum value, an Error is returned.
///
/// # Arguments
/// `slice` - (in) the slice with the data to use
fn resize_slice_to_pow2<T: Clone>(slice: &[T]) -> Result<Vec<T>, Error> {
    let len: usize = slice.len();
    if let Some(next_power_of_two) = len.checked_next_power_of_two() {
        let diff = next_power_of_two - len;
        let mut pow2_slice: Vec<T> = Vec::with_capacity(next_power_of_two);
        pow2_slice.extend_from_slice(slice);
        pow2_slice.resize(slice.len() + diff, slice[slice.len() - 1].clone());
        Ok(pow2_slice)
    } else {
        // The next power of two would exceed the maximum value of usize.
        Err(Error::ResizeError)
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use bulletproofs::PedersenGens;
    use rand::{rngs::StdRng, SeedableRng};
    use rand_core::RngCore;

    fn generate_and_check(vals: Vec<u64>, serial_scalars: Vec<Scalar>) {
        let mut rng: StdRng = SeedableRng::from_seed([1u8; 32]);
        let serials: Vec<Blinding> = serial_scalars.iter().map(|s| Blinding::from(*s)).collect();
        let (proof, commitments) = generate_range_proofs(&vals, &serials, &mut rng).unwrap();

        match check_range_proofs(&proof, &commitments, &mut rng) {
            Ok(_) => {} // This is expected.
            Err(e) => panic!("{:?}", e),
        }
    }

    #[test]
    fn test_pow2_number_of_inputs() {
        let mut rng: StdRng = SeedableRng::from_seed([1u8; 32]);
        let vals: Vec<u64> = (0..2).map(|_| rng.next_u64()).collect();
        let serial_scalars: Vec<Scalar> = vals.iter().map(|_| Scalar::random(&mut rng)).collect();
        generate_and_check(vals, serial_scalars);
    }

    #[test]
    fn test_not_pow2_number_of_inputs() {
        let mut rng: StdRng = SeedableRng::from_seed([1u8; 32]);
        let vals: Vec<u64> = (0..9).map(|_| rng.next_u64()).collect();
        let serial_scalars: Vec<Scalar> = vals.iter().map(|_| Scalar::random(&mut rng)).collect();
        generate_and_check(vals, serial_scalars);
    }

    #[test]
    // `check_range_proofs` should return an error if the commitments do not agree with the proof.
    fn test_wrong_commitments() {
        let mut rng: StdRng = SeedableRng::from_seed([1u8; 32]);

        let num_values: usize = 4;
        let values: Vec<u64> = (0..num_values).map(|_| rng.next_u64()).collect();
        let serial_scalars: Vec<Scalar> =
            (0..num_values).map(|_| Scalar::random(&mut rng)).collect();
        let serials: Vec<Blinding> = serial_scalars.iter().map(|s| Blinding::from(*s)).collect();
        let (proof, _commitments) = generate_range_proofs(&values, &serials, &mut rng).unwrap();

        // Create commitments that do not agree with the proof.
        let gen = PedersenGens::default();
        let wrong_commitments: Vec<CompressedRistretto> = serial_scalars
            .into_iter()
            .map(|serial| {
                let scalar_val = Scalar::from_bytes_mod_order([77u8; 32]);
                let commitment_point = gen.commit(scalar_val, serial);
                commitment_point.compress()
            })
            .collect();

        match check_range_proofs(&proof, &wrong_commitments, &mut rng) {
            Ok(_) => panic!(),
            Err(_e) => {} // This is expected.
        }
    }
}
