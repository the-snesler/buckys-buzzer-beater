const ROUND_NAMES = ["Jeopardy!", "Double Jeopardy!"];

/** The name of a zero-based round, such as "Double Jeopardy!" */
export function roundName(round: number): string {
  return ROUND_NAMES[round] ?? `Round ${round + 1}`;
}
