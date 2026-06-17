use crate::error::{AppError, AppResult};

/// Parse a decimal money string into integer cents without floating point.
pub fn parse_money_to_cents(input: &str, allow_zero: bool) -> AppResult<i64> {
    let trimmed = input.trim().replace(',', "");
    if trimmed.is_empty() {
        return if allow_zero {
            Ok(0)
        } else {
            Err(AppError::bad_request("Amount is required"))
        };
    }

    let (whole_part, frac_part) = match trimmed.split_once('.') {
        None => (trimmed.as_str(), ""),
        Some((whole, frac)) => {
            if frac.len() > 2 {
                return Err(AppError::bad_request(
                    "Amount must have at most 2 decimal places",
                ));
            }
            (whole, frac)
        }
    };

    let whole: i64 = whole_part
        .parse()
        .map_err(|_| AppError::bad_request("Amount must be a valid number"))?;

    let frac_cents: i64 = match frac_part.len() {
        0 => 0,
        1 => {
            frac_part
                .parse::<i64>()
                .map_err(|_| AppError::bad_request("Amount must be a valid number"))?
                * 10
        }
        2 => frac_part
            .parse::<i64>()
            .map_err(|_| AppError::bad_request("Amount must be a valid number"))?,
        _ => unreachable!("validated above"),
    };

    if whole < 0 || frac_cents < 0 {
        return Err(AppError::bad_request("Amount cannot be negative"));
    }

    let cents = whole
        .checked_mul(100)
        .and_then(|base| base.checked_add(frac_cents))
        .ok_or_else(|| AppError::bad_request("Amount is too large"))?;

    if cents == 0 && !allow_zero {
        return Err(AppError::bad_request("Amount must be greater than zero"));
    }
    if cents > 9_999_999_999 {
        return Err(AppError::bad_request("Amount is too large"));
    }

    Ok(cents)
}

#[cfg(test)]
mod tests {
    use super::parse_money_to_cents;

    #[test]
    fn parses_whole_and_decimal() {
        assert_eq!(parse_money_to_cents("26000", false).unwrap(), 2_600_000);
        assert_eq!(parse_money_to_cents("1,234.50", true).unwrap(), 123_450);
        assert_eq!(parse_money_to_cents("5.5", true).unwrap(), 550);
    }

    #[test]
    fn allows_zero_when_configured() {
        assert_eq!(parse_money_to_cents("", true).unwrap(), 0);
        assert_eq!(parse_money_to_cents("0", true).unwrap(), 0);
    }
}
