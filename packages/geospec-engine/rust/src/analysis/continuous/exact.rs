use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::{Signed, ToPrimitive, Zero};

use super::{bits, ContinuousError, ExactScalar, MAX_INTEGER_BITS};

pub(crate) fn rational(value: f64) -> Result<BigRational, ContinuousError> {
    if !value.is_finite() {
        return Err(ContinuousError::invalid(
            "Continuous-domain coordinates and thresholds must be finite.",
        ));
    }
    let value = BigRational::from_float(value).ok_or_else(|| {
        ContinuousError::outside(
            "dyadic-input",
            "The exact dyadic input could not be represented.",
        )
    })?;
    checked(value)
}

pub(super) fn scalar(value: &BigRational) -> Result<ExactScalar, ContinuousError> {
    check(value)?;
    let mut display = value.to_f64().ok_or_else(no_display)?;
    if !display.is_finite() {
        return Err(no_display());
    }
    if display == 0.0 {
        display = 0.0;
    }
    Ok(ExactScalar {
        numerator: value.numer().to_string(),
        denominator: value.denom().to_string(),
        display,
    })
}

pub(super) fn midpoint(
    minimum: &BigRational,
    maximum: &BigRational,
) -> Result<BigRational, ContinuousError> {
    let sum = add(minimum, maximum)?;
    checked(sum / BigInt::from(2))
}

pub(crate) fn add(left: &BigRational, right: &BigRational) -> Result<BigRational, ContinuousError> {
    preflight_add(left, right)?;
    checked(left + right)
}

pub(crate) fn subtract(
    left: &BigRational,
    right: &BigRational,
) -> Result<BigRational, ContinuousError> {
    preflight_add(left, right)?;
    checked(left - right)
}

pub(crate) fn multiply(
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

pub(super) fn checked(value: BigRational) -> Result<BigRational, ContinuousError> {
    check(&value)?;
    Ok(value)
}

pub(crate) fn divide(
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

fn check(value: &BigRational) -> Result<(), ContinuousError> {
    let required = super::rational_bits(value);
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

fn no_display() -> ContinuousError {
    ContinuousError::outside(
        "finite-binary64-display",
        "The exact result has no finite binary64 display value.",
    )
}

/// The shared 8192-bit continuous-domain arithmetic guard.
pub(crate) fn limit(required: u64) -> ContinuousError {
    ContinuousError::arithmetic(
        "Continuous-domain exact arithmetic",
        bits(required, MAX_INTEGER_BITS),
    )
}
