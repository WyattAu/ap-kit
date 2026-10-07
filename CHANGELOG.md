# Changelog

All notable changes to this project are documented here. Format: [Keep a
Changelog](https://keepachangelog.com/) — versions follow [semver](https://semver.org).

## [Unreleased]

## [0.1.1] - 2026-10-06

### Added

- Re-exports of `TaxCategory`, `DocumentType`, `DocumentReference`,
  `RoundingPolicy`, `TieMode` and `is_all_or_none_reverse_charge`.

  Found by the estate consumer suite, which is what it is for: a consumer
  depending on both `invoice-kit` and `ap-kit` could not name a category
  through either path as a single type, so the extraction was only half done. Two
  structurally identical enums would have been a silent incompatibility rather
  than a compile error.

## [0.1.0] - 2026-10-06

Initial release: the accounts payable half of a tax ledger, mirroring
`invoice-kit` on the vocabulary (`vat-rules`) and on the ledger
(`double-entry`).

### Added

- `Recoverability` — full, proportional, or none with the Article 168 limb
  recorded. Proportions truncate, because VAT Guidance Note 3 treats the
  unclaimable fraction as non-deductible cost, and rounding up over-claims input
  tax.
- `Bill`, `BillLine` and `Split`, producing the three-way split between gross
  payable, deductible input tax and expense. `Split::payable()` is the gross
  whatever the recoverability.
- Validation: negative line nets, empty bills, the 1000-line Peppol BIS Billing
  3.0 cap, standard-rated lines at zero, zero-rated categories at a non-zero
  rate, BR-AE-1 reverse-charge all-or-none, cross-border VAT identifier
  requirements, Article 219's unambiguous reference, and rates above 100%.
- Six ledger integration tests posting to a real `double-entry` chart, including
  the original-then-reversal pair for a supplier credit note.
