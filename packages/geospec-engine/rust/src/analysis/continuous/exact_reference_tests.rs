//! Differential cover for the exact-arithmetic identity short-circuits.
//!
//! [`reference`] is the general-operator arithmetic that [`super::exact`] used
//! before the identity short-circuits were added: every operation performs its
//! numeric preflight and then defers to the `BigRational` operator. The tests
//! below require the shipped implementation to return the identical value or
//! the identical typed refusal for every operand pair in the grid, so a
//! short-circuit cannot change a result, invent a value or lose a refusal.

use num_bigint::BigInt;
use num_integer::Integer;
use num_rational::BigRational;
use num_traits::{One, Signed, Zero};

use super::{bits, exact, rational_bits, ContinuousError, MAX_INTEGER_BITS};

/// The unoptimized exact arithmetic the short-circuits must agree with.
mod reference {
    use super::*;

    pub(super) fn add(
        left: &BigRational,
        right: &BigRational,
    ) -> Result<BigRational, ContinuousError> {
        preflight_add(left, right)?;
        checked(left + right)
    }

    pub(super) fn subtract(
        left: &BigRational,
        right: &BigRational,
    ) -> Result<BigRational, ContinuousError> {
        preflight_add(left, right)?;
        checked(left - right)
    }

    pub(super) fn multiply(
        left: &BigRational,
        right: &BigRational,
    ) -> Result<BigRational, ContinuousError> {
        let numerator_bits = left.numer().bits().saturating_add(right.numer().bits());
        let denominator_bits = left.denom().bits().saturating_add(right.denom().bits());
        if numerator_bits > MAX_INTEGER_BITS || denominator_bits > MAX_INTEGER_BITS {
            return Err(limit(numerator_bits.max(denominator_bits)));
        }
        checked(left * right)
    }

    pub(super) fn divide(
        left: &BigRational,
        right: &BigRational,
    ) -> Result<BigRational, ContinuousError> {
        check(left)?;
        check(right)?;
        if right.is_zero() {
            return Err(ContinuousError::outside(
                "nonzero-divisor",
                "Exact division requires a nonzero divisor.",
            ));
        }
        let required = left
            .numer()
            .bits()
            .saturating_add(right.denom().bits())
            .max(left.denom().bits().saturating_add(right.numer().bits()));
        if required > MAX_INTEGER_BITS {
            return Err(limit(required));
        }
        checked(left / right)
    }

    fn preflight_add(left: &BigRational, right: &BigRational) -> Result<(), ContinuousError> {
        let numerator_bits = left
            .numer()
            .bits()
            .saturating_add(right.denom().bits())
            .max(right.numer().bits().saturating_add(left.denom().bits()))
            .saturating_add(1);
        let denominator_bits = left.denom().bits().saturating_add(right.denom().bits());
        if numerator_bits > MAX_INTEGER_BITS || denominator_bits > MAX_INTEGER_BITS {
            return Err(limit(numerator_bits.max(denominator_bits)));
        }
        Ok(())
    }

    fn checked(value: BigRational) -> Result<BigRational, ContinuousError> {
        check(&value)?;
        Ok(value)
    }

    fn check(value: &BigRational) -> Result<(), ContinuousError> {
        let required = rational_bits(value);
        if required > MAX_INTEGER_BITS {
            return Err(limit(required));
        }
        if value.denom().is_zero() || value.denom().is_negative() {
            return Err(ContinuousError::invalid(
                "Exact continuous-domain values require a positive denominator.",
            ));
        }
        Ok(())
    }

    fn limit(required: u64) -> ContinuousError {
        ContinuousError::arithmetic(
            "Continuous-domain exact arithmetic",
            bits(required, MAX_INTEGER_BITS),
        )
    }
}

/// Neutral elements, zeros, equal operands, whole numbers and fractions, each
/// at widths that straddle limb boundaries and the 8192-bit arithmetic guard.
fn grid() -> Vec<BigRational> {
    let mut values = vec![
        BigRational::zero(),
        BigRational::one(),
        -BigRational::one(),
        BigRational::new(1.into(), 2.into()),
        BigRational::new((-3).into(), 2.into()),
    ];
    for bits in [0usize, 1, 31, 63, 64, 127, 1023, 4095, 8190, 8191] {
        let n = (BigInt::one() << bits) + BigInt::one();
        values.extend([
            BigRational::from_integer(n.clone()),
            BigRational::from_integer(-&n),
            BigRational::new(3.into(), n.clone()),
            BigRational::new(n.clone(), 3.into()),
            BigRational::new(-n.clone(), 3.into()),
        ]);
    }
    values
}

#[test]
fn identity_short_circuits_preserve_every_exact_value_and_limit_refusal() {
    let values = grid();
    let mut refusals = 0usize;
    for left in &values {
        for right in &values {
            for (shipped, expected) in [
                (exact::add(left, right), reference::add(left, right)),
                (
                    exact::subtract(left, right),
                    reference::subtract(left, right),
                ),
                (
                    exact::multiply(left, right),
                    reference::multiply(left, right),
                ),
                (exact::divide(left, right), reference::divide(left, right)),
            ] {
                refusals += usize::from(expected.is_err());
                assert_eq!(shipped, expected, "left={left} right={right}");
                // A short-circuit that skipped normalization would return an
                // uncanonical rational that still compares equal by value.
                if let Ok(value) = shipped {
                    assert!(
                        value.denom().is_positive(),
                        "{value} has no positive denominator"
                    );
                    assert_eq!(
                        *value.numer(),
                        value.numer() / value.numer().gcd(value.denom()),
                        "{value} is not in lowest terms"
                    );
                }
            }
        }
    }
    assert!(
        refusals > 0,
        "the grid must exercise arithmetic-limit refusals, not only successes"
    );
}
