//! How much of the tax on a bill the business may actually deduct.
//!
//! # Why this type exists and why it is not in `vat-rules`
//!
//! On the sales side, every unit of tax a business charges is tax it owes. The
//! whole VAT amount becomes a liability. There is nothing to decide.
//!
//! On the purchase side that is false. Directive 2006/112/EC Article 168 lists
//! circumstances in which the right to deduct **does not arise at all**, or
//! arises only in part — non-business use under (a), entertainment and
//! representation costs under (b), gifts under (c), and costs whose invoicing
//! and documentation requirements are not met under (e). Where a car is used for
//! business and privately, only the business share of the input tax is
//! deductible.
//!
//! That means a purchase document does not produce two outcomes from its tax, it
//! produces **three**:
//!
//! - the **gross payable** — what is owed to the supplier, which always includes
//!   the full tax whatever its recoverability, because the supplier charges it
//!   either way;
//! - the **deductible** part, which becomes an asset or a reduction of the
//!   amount capitalised; and
//! - the **non-deductible** part, which is simply expense.
//!
//! A crate that models a bill as "net plus a tax amount" cannot represent that,
//! and the error is always in the same direction: it capitalises or expenses
//! money the business was never allowed to recover. So recoverability lives
//! here, on the AP side, where it is a decision.

/// Whether the tax on a line may be deducted, and if not, why.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Recoverability {
    /// Fully deductible input tax. The default, and Article 168's residual case.
    Full,
    /// Deductible in part, for goods or services used for both business and
    /// non-business purposes.
    ///
    /// The fraction is expressed as exact minor units over a denominator rather
    /// than as a float or a percentage, because the deduction is an amount that
    /// has to reconcile against a posted entry: a "67%" business use of a mixed
    /// car has to produce a deductible figure that leaves nothing unexplained.
    Proportion {
        /// Deductible share of the tax, as a numerator.
        numerator: i64,
        /// The denominator the share is taken over. Must be positive.
        denominator: i64,
    },
    /// Not deductible, and the reason is known. Recorded rather than inferred,
    /// because an auditor asks which limb of Article 168 applied.
    None(NonDeductibleReason),
}

/// The grounds on which input tax is not deductible.
///
/// Each variant names a limb of Directive 2006/112/EC Article 168 rather than
/// describing the expense, so the record answers "under which provision" rather
/// than "why was this one different".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NonDeductibleReason {
    /// Article 168(a) — costs relating to non-business use.
    NonBusinessUse,
    /// Article 168(b) — entertainment or representation expenses.
    Entertainment,
    /// Article 168(c) — goods or services used as gifts.
    Gift,
    /// Article 168(e) — invoicing or documentation requirements not met.
    ///
    /// The costly one in practice, and the one worth persisting: once an invoice
    /// is missing, the input tax is not deductible, and the expense is the whole
    /// gross amount. Nothing in the arithmetic reveals this, which is why it has
    /// to be a field.
    DocumentationNotMet,
    /// A car is not available to the business in full. Article 168(b) and
    /// Member State car rules.
    CarNotAvailableToBusiness,
}

impl NonDeductibleReason {
    /// The Article 168 limb this ground sits under.
    pub fn article(self) -> &'static str {
        match self {
            Self::NonBusinessUse => "168(a)",
            Self::Entertainment | Self::CarNotAvailableToBusiness => "168(b)",
            Self::Gift => "168(c)",
            Self::DocumentationNotMet => "168(e)",
        }
    }
}

impl Recoverability {
    /// The deductible share of `tax`, in minor units.
    ///
    /// Rounds **down**, and the reason is not conservatism: VAT Guidance Note 3
    /// treats any fraction of a deduction that cannot be claimed as part of the
    /// non-deductible cost. Rounding up over-claims input tax, which is the more
    /// expensive direction of the error and the one a deduction that is merely
    /// *argued* against looks like.
    pub fn deductible_of(&self, tax: i64) -> i64 {
        match self {
            Self::Full => tax,
            Self::Proportion {
                numerator,
                denominator,
            } => {
                if *denominator <= 0 {
                    0
                } else {
                    ((i128::from(tax) * i128::from(*numerator)) / i128::from(*denominator)) as i64
                }
            }
            Self::None(_) => 0,
        }
    }

    /// The non-deductible part, which the caller expenses.
    ///
    /// `tax - deductible`, and never computed independently: two independent
    /// calculations of a split is exactly how a one-cent difference appears
    /// between the deductible entry and the expense entry.
    pub fn non_deductible_of(&self, tax: i64) -> i64 {
        tax - self.deductible_of(tax)
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn full_recoverability_is_the_identity() {
        assert_eq!(Recoverability::Full.deductible_of(2100), 2100);
        assert_eq!(Recoverability::Full.non_deductible_of(2100), 0);
    }

    #[test]
    fn nothing_is_recoverable_when_the_ground_is_recorded() {
        for reason in [
            NonDeductibleReason::NonBusinessUse,
            NonDeductibleReason::Entertainment,
            NonDeductibleReason::Gift,
            NonDeductibleReason::DocumentationNotMet,
            NonDeductibleReason::CarNotAvailableToBusiness,
        ] {
            let r = Recoverability::None(reason);
            assert_eq!(r.deductible_of(2100), 0, "{}", reason.article());
            // The expense is the *gross* amount's tax, not a partial figure.
            assert_eq!(r.non_deductible_of(2100), 2100);
        }
    }

    #[test]
    fn a_proportion_splits_exactly_with_nothing_left_over() {
        // Two thirds of a two-and-ten input tax.
        let r = Recoverability::Proportion {
            numerator: 2,
            denominator: 3,
        };
        assert_eq!(r.deductible_of(2100), 1400);
        assert_eq!(r.non_deductible_of(2100), 700);
        assert_eq!(r.deductible_of(2100) + r.non_deductible_of(2100), 2100);
    }

    #[test]
    fn a_proportion_rounds_the_deduction_down() {
        // One third of 10 cents is 3.33: 3 is deductible, and the 0.33 that
        // cannot be claimed is non-deductible cost.
        let r = Recoverability::Proportion {
            numerator: 1,
            denominator: 3,
        };
        assert_eq!(r.deductible_of(10), 3);
        assert_eq!(r.non_deductible_of(10), 7);
    }

    #[test]
    fn a_proportion_can_claim_nothing_and_must_not_invert() {
        let r = Recoverability::Proportion {
            numerator: 0,
            denominator: 7,
        };
        assert_eq!(r.deductible_of(2100), 0);
        assert_eq!(r.non_deductible_of(2100), 2100);

        // A denominator that would invert the split is refused outright rather
        // than producing a negative deduction and a negative expense.
        let inverted = Recoverability::Proportion {
            numerator: 3,
            denominator: 0,
        };
        assert_eq!(inverted.deductible_of(2100), 0);
        assert_eq!(inverted.non_deductible_of(2100), 2100);
    }

    #[test]
    fn each_ground_names_its_article_limb() {
        for (reason, article) in [
            (NonDeductibleReason::NonBusinessUse, "168(a)"),
            (NonDeductibleReason::Entertainment, "168(b)"),
            (NonDeductibleReason::Gift, "168(c)"),
            (NonDeductibleReason::DocumentationNotMet, "168(e)"),
            (NonDeductibleReason::CarNotAvailableToBusiness, "168(b)"),
        ] {
            assert_eq!(reason.article(), article);
        }
    }
}
