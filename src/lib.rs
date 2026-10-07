//! Accounts payable: the supplier-bill half of a tax ledger.
//!
//! # The one thing that makes AP different from AR
//!
//! A sales invoice produces two outcomes from its tax: the tax is a liability,
//! and the net is income. The tax figure is not a decision.
//!
//! A purchase bill produces **three**, because the gross payable always includes
//! the full tax regardless of whether the business may claim it back:
//!
//! - the **gross payable** owed to the supplier;
//! - the **deductible** input tax, which becomes an asset or reduces what is
//!   capitalised; and
//! - the **non-deductible** part, which is expense.
//!
//! Every arithmetic rule in [`bill`] follows from that split. Notably, changing
//! recoverability **never changes the amount owed to the supplier** — a bill
//! whose input tax turns out to be irrecoverable is still paid in full, and only
//! the accounting destination of the tax changes. A model that conflates the two
//! gets the payable wrong, which is the error that reaches the supplier.

pub mod recoverability;

pub use recoverability::{NonDeductibleReason, Recoverability};

// Re-exported, not re-declared: a consumer that depends on both crates must see
// one `TaxCategory` and one `DocumentType`, so a category chosen on a purchase
// side can be handed to sales code and compared. Two structurally identical
// enums would be a silent incompatibility.
pub use vat_rules::{
    DocumentReference, DocumentType, RoundingPolicy, TaxCategory, TieMode,
    is_all_or_none_reverse_charge,
};

pub mod bill;

pub use bill::{Bill, BillError, BillLine};
