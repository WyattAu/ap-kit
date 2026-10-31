# ap-kit

Accounts payable: the supplier-bill half of a tax ledger. Pairs with
[`invoice-kit`](https://crates.io/crates/invoice-kit) and shares its vocabulary
via [`vat-rules`](https://crates.io/crates/vat-rules).

## The one thing that makes AP different from AR

A sales invoice produces two outcomes from its tax: the tax is a liability, the
net is income. The tax figure is not a decision.

A purchase bill produces **three**, because the gross payable always includes the
full tax whether or not the business can claim it back:

- the **gross payable** owed to the supplier;
- the **deductible** input tax, which becomes an asset or reduces what is
  capitalised; and
- the **non-deductible** part, which is expense.

Directive 2006/112/EC Article 168 is where the third outcome comes from: it lists
circumstances in which the right to deduct does not arise at all; non-business
use, entertainment, gifts, and unmet invoicing requirements; and allows a
proportion where goods are used for both business and non-business purposes.

**Recoverability never changes the amount owed to the supplier.** A bill whose
input tax turns out to be irrecoverable is still paid in full; only the
accounting destination of the tax changes. A model that conflates the two
proposes paying the supplier less than they invoiced, which is a payable dispute
rather than an accounting entry.

## The other asymmetry: the line net is authoritative

Accounts receivable *issues* the document, so it derives each line net from
quantity and unit price. Accounts payable *receives* it. The supplier's line net
is data, not a derivation; recomputing `quantity × unit price` would mean
rounding a figure the supplier actually invoiced and then posting something that
differs from the document in their hand. So `BillLine` carries the net amount as
supplied, and quantity and unit price are retained for display and three-way
matching, never for the posting.

## The ledger identity

    credit   trade payables      net + tax          (always the gross)
    debit    cost / asset        net + non-deductible tax
    debit    input VAT           deductible tax

which is `expensed() + deductible_tax == payable()`, and every unit of tax is
exactly one of deductible or expense, never both.

`Split::capitalisable()` and `Split::expensed()` both contain the net, so they
are **not** complementary and must not be added together. That trap is pinned by
a test.

## Rounding

Input tax is rounded under the same four regimes as the sales side, via
`vat-rules::RoundingPolicy`; EN 16931 / Peppol per group, GST Act s9-90's
total-invoice and taxable-supply methods, and HMRC Notice 700 §17.5. A rate above
100% is refused, because the rate is per-mille and the transposition from `190` to
`1900` is a 190% posting that no other check would catch.

## Licence

MIT OR Apache-2.0.
