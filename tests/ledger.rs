//! The AP posting, executed against a real `double-entry` ledger.
//!
//! A split that adds up in isolation proves arithmetic, not accounting. These
//! tests post the entries and require the ledger to accept them, because a
//! journal that balances by luck still balances, and the question is whether the
//! *accounts* are right.

#![allow(clippy::expect_used, clippy::panic)]

use ap_kit::{Bill, BillLine, NonDeductibleReason, Recoverability};
use double_entry::money::currencies;
use double_entry::{Account, AccountId, AccountType, JournalEntry, Ledger, Line};
use vat_rules::{DocumentReference, RoundingPolicy, TaxCategory};

fn chart() -> Ledger {
    let mut ledger = Ledger::new();
    for (id, name, kind) in [
        ("2000", "Trade payables", AccountType::Liability),
        ("3200", "Input VAT", AccountType::Asset),
        ("4000", "Purchases and cost of sales", AccountType::Expense),
        ("7000", "Non-deductible costs", AccountType::Expense),
    ] {
        ledger.add_account(Account::new(id, name, kind));
    }
    ledger
}

fn bill() -> Bill {
    Bill::draft("2026-07-01", currencies::EUR, RoundingPolicy::En16931Group)
        .from_supplier("DE7654321")
        .with_line(BillLine {
            id: "1".into(),
            description: "Raw materials".into(),
            net_amount: 10_000,
            quantity: "10".into(),
            unit_price: "10.00".into(),
            tax_category: TaxCategory::Standard,
            tax_rate_permille: 190,
            recoverability: Recoverability::Full,
        })
}

#[test]
fn a_fully_deductible_bill_posts_as_payable_versus_input_vat() {
    let mut ledger = chart();
    let bill = bill();
    let split = bill.split();
    assert_eq!(split.payable(), 11_900);

    ledger
        .post(
            &JournalEntry::draft(
                "JE-1",
                "2026-07-01",
                "2026-07",
                vec![
                    // Credit the supplier for the gross.
                    Line::debit(
                        "4000",
                        double_entry::Amount::minor(split.expensed(), currencies::EUR),
                    ),
                    Line::debit(
                        "3200",
                        double_entry::Amount::minor(split.deductible_tax, currencies::EUR),
                    ),
                    Line::credit(
                        "2000",
                        double_entry::Amount::minor(split.payable(), currencies::EUR),
                    ),
                ],
            ),
            0,
        )
        .expect("the bill's three entries balance by construction");

    assert_eq!(
        ledger.balance(&AccountId::new("2000")),
        -11_900,
        "payable is a credit"
    );
    assert_eq!(
        ledger.balance(&AccountId::new("3200")),
        1_900,
        "input VAT is a debit"
    );
    assert_eq!(
        ledger.balance(&AccountId::new("4000")),
        10_000,
        "cost excludes VAT"
    );
    assert_eq!(
        ledger.balance(&AccountId::new("7000")),
        0,
        "nothing irrecoverable here"
    );
}

#[test]
fn an_irrecoverable_bill_expenses_the_whole_gross_and_deducts_nothing() {
    let mut ledger = chart();
    let mut bill = bill();
    if let Some(line) = bill.lines.first_mut() {
        line.recoverability = Recoverability::None(NonDeductibleReason::DocumentationNotMet);
    }
    let split = bill.split();

    assert_eq!(
        split.payable(),
        11_900,
        "the supplier is still owed the gross"
    );
    assert_eq!(split.deductible_tax, 0);
    assert_eq!(split.non_deductible_tax, 1_900);

    ledger
        .post(
            &JournalEntry::draft(
                "JE-1",
                "2026-07-01",
                "2026-07",
                vec![
                    Line::debit(
                        "4000",
                        double_entry::Amount::minor(split.net, currencies::EUR),
                    ),
                    Line::debit(
                        "7000",
                        double_entry::Amount::minor(split.non_deductible_tax, currencies::EUR),
                    ),
                    Line::credit(
                        "2000",
                        double_entry::Amount::minor(split.payable(), currencies::EUR),
                    ),
                ],
            ),
            0,
        )
        .expect("the non-deductible bill still balances");

    assert_eq!(
        ledger.balance(&AccountId::new("3200")),
        0,
        "nothing to deduct"
    );
    assert_eq!(
        ledger.balance(&AccountId::new("7000")),
        1_900,
        "the tax became expense"
    );
    assert_eq!(ledger.balance(&AccountId::new("4000")), 10_000);
}

#[test]
fn a_partly_deductible_bill_splits_the_tax_across_three_accounts() {
    let mut ledger = chart();
    let mut bill = bill();
    // Two thirds of a mixed-use vehicle's input tax.
    if let Some(line) = bill.lines.first_mut() {
        line.recoverability = Recoverability::Proportion {
            numerator: 2,
            denominator: 3,
        };
    }
    let split = bill.split();

    assert_eq!(split.payable(), 11_900);
    assert_eq!(split.deductible_tax, 1_266, "1900 * 2 / 3, truncated");
    assert_eq!(split.non_deductible_tax, 634);

    ledger
        .post(
            &JournalEntry::draft(
                "JE-1",
                "2026-07-01",
                "2026-07",
                vec![
                    Line::debit(
                        "4000",
                        double_entry::Amount::minor(split.net, currencies::EUR),
                    ),
                    Line::debit(
                        "3200",
                        double_entry::Amount::minor(split.deductible_tax, currencies::EUR),
                    ),
                    Line::debit(
                        "7000",
                        double_entry::Amount::minor(split.non_deductible_tax, currencies::EUR),
                    ),
                    Line::credit(
                        "2000",
                        double_entry::Amount::minor(split.payable(), currencies::EUR),
                    ),
                ],
            ),
            0,
        )
        .expect("the partial bill balances");

    let vat_and_cost: i64 =
        ledger.balance(&AccountId::new("3200")) + ledger.balance(&AccountId::new("7000"));
    assert_eq!(
        vat_and_cost, 1_900,
        "the two tax destinations together are exactly the tax, and nothing more"
    );
}

#[test]
fn a_supplier_credit_note_reverses_the_deduction_and_the_payable() {
    let mut ledger = chart();
    // Post the original bill first: a reversal asserted in isolation proves only
    // that the credit note balances, not that it discharges anything.
    let original = bill();
    let original_split = original.split();
    let version = ledger
        .post(
            &JournalEntry::draft(
                "JE-1",
                "2026-07-01",
                "2026-07",
                vec![
                    Line::debit(
                        "4000",
                        double_entry::Amount::minor(original_split.net, currencies::EUR),
                    ),
                    Line::debit(
                        "3200",
                        double_entry::Amount::minor(original_split.deductible_tax, currencies::EUR),
                    ),
                    Line::credit(
                        "2000",
                        double_entry::Amount::minor(original_split.payable(), currencies::EUR),
                    ),
                ],
            ),
            0,
        )
        .expect("the original bill posts");
    assert_eq!(ledger.balance(&AccountId::new("2000")), -11_900);

    let mut credit = bill().as_credit_note();
    credit.references = vec![DocumentReference::new("BILL-2026-0001", "2026-07-01")];
    assert_eq!(
        credit.validate(),
        Ok(()),
        "an unambiguous reference is enough"
    );
    let split = credit.split();
    assert_eq!(split.payable(), 11_900);

    // The mirror of the original entry: credit the tax back and debit the payable.
    ledger
        .post(
            &JournalEntry::draft(
                "JE-2",
                "2026-07-02",
                "2026-07",
                vec![
                    Line::debit(
                        "2000",
                        double_entry::Amount::minor(split.payable(), currencies::EUR),
                    ),
                    Line::credit(
                        "3200",
                        double_entry::Amount::minor(split.deductible_tax, currencies::EUR),
                    ),
                    Line::credit(
                        "4000",
                        double_entry::Amount::minor(split.expensed(), currencies::EUR),
                    ),
                ],
            ),
            version,
        )
        .expect("the credit note balances against the ledger's current version");

    assert_eq!(
        ledger.balance(&AccountId::new("2000")),
        0,
        "the payable is discharged"
    );
    assert_eq!(
        ledger.balance(&AccountId::new("3200")),
        0,
        "the deduction is reversed"
    );
    assert_eq!(ledger.balance(&AccountId::new("4000")), 0);
}

#[test]
fn every_currency_minor_unit_is_honoured() {
    // JPY has no minor unit; a bill in JPY must not be rounded to cents.
    let bill = Bill::draft("2026-07-01", currencies::JPY, RoundingPolicy::En16931Group).with_line(
        BillLine {
            id: "1".into(),
            description: "Goods".into(),
            net_amount: 10_000,
            quantity: "1".into(),
            unit_price: "10000".into(),
            tax_category: TaxCategory::Standard,
            tax_rate_permille: 100,
            recoverability: Recoverability::Full,
        },
    );
    let split = bill.split();
    assert_eq!(split.tax, 1_000, "10% of 10000 yen");
    assert_eq!(split.payable(), 11_000);
    assert_eq!(bill.payable().units, 11_000);
    assert_eq!(bill.payable().currency, currencies::JPY);
}

#[test]
fn a_line_count_of_one_thousand_is_still_accepted() {
    let mut bill = bill();
    bill.lines.clear();
    for index in 0..1000 {
        bill.lines.push(BillLine {
            id: index.to_string(),
            description: "line".into(),
            net_amount: 1,
            quantity: "1".into(),
            unit_price: "0.01".into(),
            tax_category: TaxCategory::ZeroRated,
            tax_rate_permille: 0,
            recoverability: Recoverability::Full,
        });
    }
    assert_eq!(bill.validate(), Ok(()), "the cap is inclusive");
    assert_eq!(bill.net_total(), 1000);
}
