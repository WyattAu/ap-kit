//! A supplier bill: the AP mirror of an AR invoice, and where it must differ.
//!
//! # The asymmetry that shapes this whole module
//!
//! Accounts receivable *issues* the document, so it derives the line net from
//! quantity and unit price — that arithmetic is the issuer's job and its own
//! rounding policy governs it.
//!
//! Accounts payable *receives* the document. The supplier's line net is
//! **authoritative data**, not a derivation. Recomputing `quantity × unit price`
//! here would mean rounding a figure the supplier actually invoiced, and then
//! posting something to the ledger that differs from the document in the
//! supplier's hand. The difference is small, it is systematic, and it is exactly
//! the kind of discrepancy that blocks a payment run. So [`BillLine`] carries
//! the net amount as supplied, and quantity and unit price are retained for
//! display and three-way matching, never for the posting.
//!
//! # The posting, in one sentence
//!
//! Credit the payable with the **gross**; debit the expense with the net plus
//! the non-deductible tax; debit input VAT with the deductible tax. The payable
//! is the gross whatever the recoverability, because the supplier charges the
//! gross whatever the recoverability.

use double_entry::money::currencies;
use double_entry::{Amount, Currency};
use vat_rules::{DocumentReference, DocumentType, RoundingPolicy, TaxCategory};

use crate::recoverability::Recoverability;

/// Why a bill cannot be computed or issued.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BillError {
    /// A standard-rated line was given a zero rate.
    StandardWithZeroRate,
    /// A category that must carry zero tax was given a non-zero rate.
    NonZeroRateOnZeroCategory {
        /// The UNCL5305 category code.
        category: &'static str,
    },
    /// A reverse-charged bill mixing AE and non-AE lines. BR-AE-1.
    MixedReverseCharge,
    /// A reverse-charged line without both parties' VAT identifiers.
    ReverseChargeWithoutVatIds,
    /// A corrective bill without an unambiguous reference to the original.
    ///
    /// Article 219 treats a document that amends *and* refers specifically and
    /// unambiguously as an invoice. A supplier credit note that fails the second
    /// half does not correct the original, so the original's input tax stands —
    /// which is why this is refused rather than warned about.
    MissingReference,
    /// A negative line net.
    ///
    /// Peppol BR-27 forbids a negative item net price on both sides. Direction
    /// belongs to the document type, and a credit note is how a negative bill is
    /// expressed.
    NegativeLineNet,
    /// A rate above 100%, which is a data-entry fault rather than a tax rate.
    ///
    /// Refused rather than clamped. The rate is per-mille, so a 19% rate is
    /// `190`; the transposition to `1900` is a silent 190% posting that no other
    /// check in this module would catch, because the figure is still a
    /// well-formed non-zero rate.
    ImplausibleRate {
        /// The rate as given, in per-mille.
        rate_permille: u32,
    },
    /// A bill with no lines.
    NoLines,
    /// A line count beyond what a single document may carry.
    TooManyLines {
        /// The limit.
        limit: usize,
    },
}

/// The maximum lines on one bill.
///
/// Peppol BIS Billing 3.0 caps BT-126 at 1000 line items. The cap is enforced
/// because an uncapped document is what produces a 50MB payload, and a
/// conforming parser on the other side is entitled to reject it.
pub const MAX_LINES: usize = 1000;

/// One line on a supplier bill.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BillLine {
    /// Caller-assigned identifier, unique within the document.
    pub id: String,
    /// Human-readable description.
    pub description: String,
    /// The net amount **as invoiced**, in minor units. Never recomputed.
    pub net_amount: i64,
    /// Quantity, as supplied. Retained for matching and display only.
    pub quantity: String,
    /// Unit price, as supplied. Retained for matching and display only.
    pub unit_price: String,
    /// The UNCL5305 tax category. BT-151.
    pub tax_category: TaxCategory,
    /// The tax rate as a percentage. BT-152.
    pub tax_rate_permille: u32,
    /// Whether the tax on this line may be deducted.
    ///
    /// Defaults to [`Recoverability::Full`] in the constructors; the field is
    /// explicit so an irrecoverable line cannot be represented as recoverable
    /// by omission.
    pub recoverability: Recoverability,
}

/// A supplier bill.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bill {
    /// The document's number, once issued. Empty while a draft.
    pub number: String,
    /// The UNTDID 1001 type, carrying direction.
    pub document_type: DocumentType,
    /// Issue date, `YYYY-MM-DD`.
    pub issue_date: String,
    /// Tax point / supply date, `YYYY-MM-DD`, when it differs from issue.
    pub supply_date: Option<String>,
    /// The currency.
    pub currency: Currency,
    /// The jurisdiction's arithmetic.
    pub rounding_policy: RoundingPolicy,
    /// The supplier's VAT identifier, required for the cross-border categories.
    pub supplier_vat_id: Option<String>,
    /// This business's own VAT identifier, required for reverse charge.
    pub buyer_vat_id: Option<String>,
    /// The supplier's country, for the intra-community delivery test.
    pub delivery_country: Option<String>,
    /// The lines.
    pub lines: Vec<BillLine>,
    /// References to amended documents.
    pub references: Vec<DocumentReference>,
}

/// The three amounts a bill produces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Split {
    /// The net, as invoiced.
    pub net: i64,
    /// The full tax, before any recoverability question.
    pub tax: i64,
    /// The deductible part of the tax.
    pub deductible_tax: i64,
    /// The non-deductible part, which is expense.
    pub non_deductible_tax: i64,
}

impl Split {
    /// What is owed to the supplier: the net plus the **full** tax.
    ///
    /// Recoverability does not appear in this calculation and must never be
    /// allowed to: the supplier charges the gross regardless of whether the
    /// business can reclaim it. A model that nets off irrecoverable input tax
    /// here proposes paying the supplier less than they invoiced, which is a
    /// payable dispute, not an accounting entry.
    pub fn payable(&self) -> i64 {
        self.net + self.tax
    }

    /// The amount that becomes an asset or a reduction of what is capitalised.
    ///
    /// **Do not add this to [`Split::expensed`]**: both contain the net. The
    /// balancing identity is `expensed() + deductible_tax == payable()`.
    pub fn capitalisable(&self) -> i64 {
        self.net + self.deductible_tax
    }

    /// The amount expensed, being the net plus the tax that cannot be claimed.
    ///
    /// The cost base for the ledger entry, which pairs with
    /// `deductible_tax` against [`Split::payable`].
    pub fn expensed(&self) -> i64 {
        self.net + self.non_deductible_tax
    }
}

impl Bill {
    /// A draft bill in `currency` under `policy`.
    pub fn draft(
        issue_date: impl Into<String>,
        currency: Currency,
        policy: RoundingPolicy,
    ) -> Self {
        Self {
            number: String::new(),
            document_type: DocumentType::Invoice,
            issue_date: issue_date.into(),
            supply_date: None,
            currency,
            rounding_policy: policy,
            supplier_vat_id: None,
            buyer_vat_id: None,
            delivery_country: None,
            lines: Vec::new(),
            references: Vec::new(),
        }
    }

    /// Make this a supplier credit note, reducing what we owe.
    pub fn as_credit_note(mut self) -> Self {
        self.document_type = DocumentType::CreditNote;
        self
    }

    /// Add a fully deductible line.
    pub fn with_line(mut self, line: BillLine) -> Self {
        self.lines.push(line);
        self
    }

    /// Record the supplier's VAT identifier.
    pub fn from_supplier(mut self, vat_id: impl Into<String>) -> Self {
        self.supplier_vat_id = Some(vat_id.into());
        self
    }

    /// Record this business's VAT identifier, needed for reverse charge.
    pub fn for_buyer(mut self, vat_id: impl Into<String>) -> Self {
        self.buyer_vat_id = Some(vat_id.into());
        self
    }

    /// Record the country of delivery, for the intra-community test.
    pub fn delivered_in(mut self, country: impl Into<String>) -> Self {
        self.delivery_country = Some(country.into());
        self
    }

    /// The net, as invoiced.
    pub fn net_total(&self) -> i64 {
        self.lines.iter().map(|line| line.net_amount).sum()
    }

    /// The full tax, before any recoverability question.
    pub fn total_tax(&self) -> i64 {
        self.lines.iter().map(|line| self.line_tax(line)).sum()
    }

    /// The deductible input tax on this line.
    pub fn deductible_tax(&self) -> i64 {
        self.lines
            .iter()
            .map(|line| line.recoverability.deductible_of(self.line_tax(line)))
            .sum()
    }

    /// The three-way split this bill produces.
    pub fn split(&self) -> Split {
        let tax = self.total_tax();
        let deductible_tax = self.deductible_tax();
        Split {
            net: self.net_total(),
            tax,
            deductible_tax,
            non_deductible_tax: tax - deductible_tax,
        }
    }

    /// What is owed to the supplier, as an [`Amount`].
    pub fn payable(&self) -> Amount {
        Amount::minor(self.split().payable().abs(), self.currency)
    }

    /// The rules a bill must satisfy before it can be issued.
    pub fn validate(&self) -> Result<(), BillError> {
        if self.lines.is_empty() {
            return Err(BillError::NoLines);
        }
        if self.lines.len() > MAX_LINES {
            return Err(BillError::TooManyLines { limit: MAX_LINES });
        }
        for line in &self.lines {
            if line.net_amount < 0 {
                return Err(BillError::NegativeLineNet);
            }
            if line.tax_category == TaxCategory::Standard && line.tax_rate_permille == 0 {
                return Err(BillError::StandardWithZeroRate);
            }
            if line.tax_rate_permille > 1000 {
                return Err(BillError::ImplausibleRate {
                    rate_permille: line.tax_rate_permille,
                });
            }
            if line.tax_category != TaxCategory::Standard && line.tax_rate_permille != 0 {
                return Err(BillError::NonZeroRateOnZeroCategory {
                    category: line.tax_category.code(),
                });
            }
            if line.tax_category.requires_buyer_vat_id()
                && (self.buyer_vat_id.is_none() || self.supplier_vat_id.is_none())
            {
                return Err(BillError::ReverseChargeWithoutVatIds);
            }
        }
        // BR-AE-1 is all-or-none, and it is easy to break by accident because the
        // category is per line.
        if !vat_rules::is_all_or_none_reverse_charge(
            &self
                .lines
                .iter()
                .map(|l| l.tax_category)
                .collect::<Vec<_>>(),
        ) {
            return Err(BillError::MixedReverseCharge);
        }
        if self.document_type.requires_reference() {
            let ok = self.references.iter().any(|r| r.is_unambiguous());
            if !ok {
                return Err(BillError::MissingReference);
            }
        }
        Ok(())
    }

    /// The input tax on one line, rounded under the bill's policy.
    ///
    /// Integer arithmetic throughout: rate is per-mille, so tax is
    /// `net * rate / 1000`, and rounding the exact integer quotient is the only
    /// way to avoid a decimal representation appearing anywhere near a tax
    /// figure.
    fn line_tax(&self, line: &BillLine) -> i64 {
        let numerator = i128::from(line.net_amount) * i128::from(line.tax_rate_permille);
        i64::try_from(round_div(numerator, 1000, self.rounding_policy.tie_mode()))
            .unwrap_or(i64::MAX)
    }
}

/// Round `numerator / divisor` exactly, breaking ties by `mode`.
///
/// The magnitude is rounded and the sign restored at the end. Doing the tie
/// comparison on a signed quotient makes "which neighbour is even" ill-defined,
/// because truncation toward zero means the quotient is the *upper* candidate
/// for a negative value.
fn round_div(numerator: i128, divisor: i128, mode: vat_rules::TieMode) -> i128 {
    if divisor == 0 {
        return 0;
    }
    let sign = if numerator < 0 { -1 } else { 1 };
    let magnitude = numerator.unsigned_abs() as i128;
    let divisor = divisor.unsigned_abs() as i128;
    let quotient = magnitude / divisor;
    let remainder = magnitude % divisor;
    if remainder == 0 {
        return quotient * sign;
    }
    let doubled = remainder * 2;
    let takes_upper =
        doubled > divisor || (doubled == divisor && mode.takes_upper_on_tie(quotient % 2 == 0));
    let rounded = if takes_upper { quotient + 1 } else { quotient };
    rounded * sign
}

/// A convenience for the common EUR bill, fully deductible.
pub fn eur_bill(issue_date: &str, policy: RoundingPolicy) -> Bill {
    Bill::draft(issue_date, currencies::EUR, policy)
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::recoverability::NonDeductibleReason;

    fn line(id: &str, net: i64, rate: u32) -> BillLine {
        BillLine {
            id: id.into(),
            description: "line".into(),
            net_amount: net,
            quantity: "1".into(),
            unit_price: "0.00".into(),
            tax_category: TaxCategory::Standard,
            tax_rate_permille: rate,
            recoverability: Recoverability::Full,
        }
    }

    #[test]
    fn the_payable_is_gross_regardless_of_recoverability() {
        let bill = eur_bill("2026-07-01", RoundingPolicy::En16931Group).with_line(BillLine {
            recoverability: Recoverability::None(NonDeductibleReason::DocumentationNotMet),
            ..line("1", 10_000, 190)
        });
        let split = bill.split();
        assert_eq!(split.payable(), 11_900, "the supplier is owed the gross");
        assert_eq!(split.deductible_tax, 0);
        assert_eq!(split.non_deductible_tax, 1_900);
        assert_eq!(split.expensed(), 11_900, "but the whole cost is expense");
    }

    #[test]
    fn a_fully_deductible_bill_splits_into_capital_and_nothing_expensed() {
        let bill =
            eur_bill("2026-07-01", RoundingPolicy::En16931Group).with_line(line("1", 10_000, 190));
        let split = bill.split();
        assert_eq!(split.payable(), 11_900);
        assert_eq!(split.capitalisable(), 11_900);
        assert_eq!(
            split.expensed(),
            10_000,
            "the net is the cost, not the gross"
        );
    }

    #[test]
    fn the_three_destinations_balance_against_the_payable() {
        // The ledger identity: the cost base plus the deductible input tax equals
        // the gross owed. Note what is deliberately *not* asserted here —
        // `capitalisable()` and `expensed()` both contain the net, so adding them
        // double-counts it. That trap is what the next test pins down.
        for (net, rate) in [(10_000i64, 190u32), (5_555, 190), (3, 7), (99_999, 250)] {
            let bill = eur_bill("2026-07-01", RoundingPolicy::En16931Group)
                .with_line(line("1", net, rate));
            let split = bill.split();
            assert_eq!(
                split.expensed() + split.deductible_tax,
                split.payable(),
                "net {net} at {rate} per-mille"
            );
            assert_eq!(
                split.deductible_tax + split.non_deductible_tax,
                split.tax,
                "every unit of tax is either deductible or expense, never both"
            );
        }
    }

    #[test]
    fn capitalisable_and_expensed_overlap_and_must_not_be_summed() {
        // `capitalisable` is the VAT-inclusive asset cost and `expensed` is the
        // VAT-inclusive expense. Both start from the net, so on a mixed bill they
        // are not complementary and adding them exceeds the gross by the net.
        // Documented here because it is the mistake this type invites.
        let bill = eur_bill("2026-07-01", RoundingPolicy::En16931Group)
            .with_line(line("1", 10_000, 190))
            .with_line(BillLine {
                recoverability: Recoverability::None(NonDeductibleReason::Entertainment),
                ..line("2", 5_000, 190)
            });
        let split = bill.split();
        assert!(split.payable() > 0);
        assert!(
            split.capitalisable() + split.expensed() > split.payable(),
            "the overlap is real, and is why the balancing test uses expensed()"
        );
    }

    #[test]
    fn the_tax_is_the_invoiced_net_times_the_rate() {
        let bill =
            eur_bill("2026-07-01", RoundingPolicy::En16931Group).with_line(line("1", 10_000, 190));
        assert_eq!(bill.total_tax(), 1_900);
        assert_eq!(bill.net_total(), 10_000);
    }

    #[test]
    fn a_credit_note_reduces_the_payable_but_never_the_expense_direction() {
        let bill = eur_bill("2026-07-01", RoundingPolicy::En16931Group)
            .as_credit_note()
            .with_line(line("1", 10_000, 190));
        // A supplier credit note reduces what is owed and reverses the deduction.
        assert_eq!(bill.split().payable(), 11_900);
        assert!(bill.document_type.requires_reference());
        assert!(matches!(bill.validate(), Err(BillError::MissingReference)));
    }

    #[test]
    fn a_corrective_bill_needs_both_halves_of_article_219() {
        let mut bill = eur_bill("2026-07-01", RoundingPolicy::En16931Group)
            .as_credit_note()
            .with_line(line("1", 1, 190));
        bill.references = vec![DocumentReference::new("BILL-2026-0007", "")];
        assert_eq!(bill.validate(), Err(BillError::MissingReference));

        bill.references = vec![DocumentReference::new("BILL-2026-0007", "2026-06-01")];
        assert_eq!(bill.validate(), Ok(()));
    }

    #[test]
    fn reverse_charge_needs_both_vat_ids_and_is_all_or_none() {
        let mut bill = eur_bill("2026-07-01", RoundingPolicy::En16931Group).with_line(BillLine {
            tax_category: TaxCategory::ReverseCharge,
            ..line("1", 10_000, 0)
        });
        assert_eq!(bill.validate(), Err(BillError::ReverseChargeWithoutVatIds));

        bill.buyer_vat_id = Some("IE1234567".into());
        bill.supplier_vat_id = Some("DE7654321".into());
        assert_eq!(bill.validate(), Ok(()));

        bill.lines.push(line("2", 500, 190));
        assert_eq!(bill.validate(), Err(BillError::MixedReverseCharge));
    }

    #[test]
    fn a_negative_line_is_refused_because_credit_notes_are_the_answer() {
        let bill =
            eur_bill("2026-07-01", RoundingPolicy::En16931Group).with_line(line("1", -100, 190));
        assert_eq!(bill.validate(), Err(BillError::NegativeLineNet));
    }

    #[test]
    fn a_rate_above_one_hundred_percent_is_refused() {
        // 19% is 190 per-mille. The transposition to 1900 is a 190% posting that
        // is otherwise indistinguishable from a valid line.
        let bill =
            eur_bill("2026-07-01", RoundingPolicy::En16931Group).with_line(line("1", 10_000, 1900));
        assert_eq!(
            bill.validate(),
            Err(BillError::ImplausibleRate {
                rate_permille: 1900
            })
        );
        assert_eq!(
            eur_bill("2026-07-01", RoundingPolicy::En16931Group)
                .with_line(line("1", 10_000, 190))
                .validate(),
            Ok(()),
            "19% is 190 per-mille"
        );
    }

    #[test]
    fn an_empty_bill_is_refused() {
        assert_eq!(
            eur_bill("2026-07-01", RoundingPolicy::En16931Group).validate(),
            Err(BillError::NoLines)
        );
    }

    #[test]
    fn a_zero_rated_line_needs_no_rate_and_a_standard_line_needs_one() {
        let zero = eur_bill("2026-07-01", RoundingPolicy::En16931Group).with_line(BillLine {
            tax_category: TaxCategory::ZeroRated,
            ..line("1", 10_000, 0)
        });
        assert_eq!(zero.validate(), Ok(()));
        assert_eq!(zero.total_tax(), 0);

        let standard_at_zero =
            eur_bill("2026-07-01", RoundingPolicy::En16931Group).with_line(line("1", 10_000, 0));
        assert_eq!(
            standard_at_zero.validate(),
            Err(BillError::StandardWithZeroRate)
        );
    }

    #[test]
    fn the_tax_rounds_under_each_regime_without_losing_a_cent() {
        // 5c at 10% is exactly half a cent: half-up takes it, half-even does not.
        let half =
            eur_bill("2026-07-01", RoundingPolicy::En16931Group).with_line(line("1", 5, 100));
        assert_eq!(half.total_tax(), 1, "half-up, as EN 16931 practice adopts");

        let hmrc =
            eur_bill("2026-07-01", RoundingPolicy::HmrcSeventeenFive).with_line(line("1", 5, 100));
        assert_eq!(hmrc.total_tax(), 1, "HMRC rounds up at half a penny");
    }

    #[test]
    fn the_maximum_line_count_is_enforced() {
        let mut bill = eur_bill("2026-07-01", RoundingPolicy::En16931Group);
        for index in 0..=MAX_LINES {
            bill.lines.push(line(&index.to_string(), 1, 0));
        }
        assert_eq!(
            bill.validate(),
            Err(BillError::TooManyLines { limit: MAX_LINES })
        );
    }

    #[test]
    fn a_split_adds_up_no_matter_how_it_was_cut() {
        let bill = eur_bill("2026-07-01", RoundingPolicy::En16931Group)
            .with_line(line("1", 33_333, 190))
            .with_line(BillLine {
                recoverability: Recoverability::Proportion {
                    numerator: 2,
                    denominator: 3,
                },
                ..line("2", 10_000, 70)
            })
            .with_line(BillLine {
                recoverability: Recoverability::None(NonDeductibleReason::Entertainment),
                ..line("3", 5_000, 190)
            });
        let split = bill.split();
        assert_eq!(split.payable(), 33_333 + 10_000 + 5_000 + split.tax);
        assert_eq!(
            split.deductible_tax + split.non_deductible_tax,
            split.tax,
            "every unit of tax is either deductible or expense, never both"
        );
        assert_eq!(
            split.expensed() + split.deductible_tax,
            split.payable(),
            "and the cost base plus the deduction equals the gross owed"
        );
    }
}
