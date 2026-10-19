import type { FinalState, GameState } from "../types/game";

/** Whether every Final Jeopardy response has been revealed and judged */
export function allJudged(final: FinalState): boolean {
  return final.revealIndex >= final.entries.length;
}

/**
 * The game state as the display may see it.
 *
 * The server sends the host everything about Final Jeopardy, since the host reads the
 * clue, judges the responses and may peek at the wagers. The display is shown to the
 * players, so before forwarding the game state to it, this removes:
 * - the clue, until the host reveals it
 * - the correct response, until every response has been judged
 * - each player's response, until the host reveals it
 * - each player's wager, until their response has been judged
 */
export function displayGameState(gameState: GameState | null): GameState | null {
  const final = gameState?.final;
  if (!gameState || !final) return gameState;

  const showAnswer =
    gameState.state === "gameEnd" || (gameState.state === "finalReveal" && allJudged(final));
  return {
    ...gameState,
    final: {
      category: final.category,
      question: gameState.state === "finalWager" ? "" : final.question,
      answer: showAnswer ? final.answer : "",
      entries: final.entries.map((entry) => ({
        pid: entry.pid,
        wager: entry.correct !== null ? entry.wager : null,
        answer: entry.revealed ? entry.answer : null,
        correct: entry.correct,
        revealed: entry.revealed,
      })),
      revealIndex: final.revealIndex,
    },
  };
}
