/** Formats a whole number of dollars, such as "$400", "$0" or "-$400" */
export function formatMoney(amount: number): string {
  return amount < 0 ? `-$${Math.abs(amount)}` : `$${amount}`;
}
