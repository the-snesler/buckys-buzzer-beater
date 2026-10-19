import type { Category, Question } from "../types/game";

/**
 * Parsing and validation of game files:
 *
 * ```jsonc
 * {
 *   "game": {
 *     "single": [{ "category": "...", "clues": [{ "value": 200, "clue": "...", "solution": "...", "dailyDouble": true }] }],
 *     "double": [ ... ],  // same shape; omitted or [] = no Double Jeopardy
 *     "final": { "category": "...", "clue": "...", "solution": "..." }  // omitted or empty clue = no Final Jeopardy
 *   }
 * }
 * ```
 */

/** A clue as the game is set up, including whether it hides a Daily Double */
export interface GameQuestion extends Question {
  dailyDouble: boolean;
}

export interface GameCategory {
  title: string;
  questions: GameQuestion[];
}

export interface FinalClue {
  category: string;
  question: string;
  answer: string;
}

export interface ParsedGame {
  single: GameCategory[];
  /** Empty if the game has no Double Jeopardy */
  double: GameCategory[];
  final: FinalClue | null;
}

export class GameFileError extends Error {}

/** Parses and validates the JSON of a game file. Throws a GameFileError if it's invalid. */
export function parseGameFile(json: unknown): ParsedGame {
  const game = isObject(json) ? json.game : undefined;
  if (!isObject(game)) {
    throw new GameFileError('Invalid format: expected a "game" object');
  }

  if (!Array.isArray(game.single) || game.single.length === 0) {
    throw new GameFileError("Invalid format: expected a non-empty game.single array");
  }
  const single = parseRound(game.single, "game.single");

  let double: GameCategory[] = [];
  if (game.double !== undefined && game.double !== null) {
    if (!Array.isArray(game.double)) {
      throw new GameFileError("Invalid format: game.double must be an array");
    }
    double = parseRound(game.double, "game.double");
  }

  return { single, double, final: parseFinal(game.final) };
}

/** Builds a game from categories that have no Daily Doubles, such as the Game Builder's */
export function gameFromCategories(categories: Category[]): ParsedGame {
  return {
    single: categories.map((category) => ({
      title: category.title,
      questions: category.questions.map((question) => ({ ...question, dailyDouble: false })),
    })),
    double: [],
    final: null,
  };
}

/**
 * The categories of a round, as sent to the server. Daily Doubles aren't played yet, so
 * their flags stay here.
 */
export function toServerCategories(categories: GameCategory[]): Category[] {
  return categories.map((category) => ({
    title: category.title,
    questions: category.questions.map(({ question, answer, value, answered }) => ({
      question,
      answer,
      value,
      answered,
    })),
  }));
}

/** A one-line summary of what the game contains, such as "6 categories + Double Jeopardy" */
export function describeGame(game: ParsedGame): string {
  const categories = `${game.single.length} ${game.single.length === 1 ? "category" : "categories"}`;
  return game.double.length > 0 ? `${categories} + Double Jeopardy` : categories;
}

function parseRound(round: unknown[], path: string): GameCategory[] {
  return round.map((category, catIdx) => {
    const categoryPath = `${path}[${catIdx}]`;
    if (
      !isObject(category) ||
      !isNonEmptyString(category.category) ||
      !Array.isArray(category.clues) ||
      category.clues.length === 0
    ) {
      throw new GameFileError(
        `Invalid category at ${categoryPath}: expected a "category" name and a non-empty "clues" array`
      );
    }

    return {
      title: category.category,
      questions: category.clues.map((clue, clueIdx) =>
        parseClue(clue, `${categoryPath}.clues[${clueIdx}]`)
      ),
    };
  });
}

function parseClue(clue: unknown, path: string): GameQuestion {
  if (!isObject(clue) || !isNonEmptyString(clue.clue) || !isNonEmptyString(clue.solution)) {
    throw new GameFileError(`Invalid clue at ${path}: expected a "clue" and a "solution"`);
  }
  if (typeof clue.value !== "number" || !Number.isInteger(clue.value) || clue.value < 0) {
    throw new GameFileError(`Invalid clue at ${path}: "value" must be a whole number of dollars`);
  }
  if (clue.dailyDouble !== undefined && typeof clue.dailyDouble !== "boolean") {
    throw new GameFileError(`Invalid clue at ${path}: "dailyDouble" must be true or false`);
  }

  return {
    question: clue.clue,
    answer: clue.solution,
    value: clue.value,
    answered: false,
    dailyDouble: clue.dailyDouble ?? false,
  };
}

function parseFinal(final: unknown): FinalClue | null {
  if (final === undefined || final === null) {
    return null;
  }
  if (!isObject(final)) {
    throw new GameFileError("Invalid format: game.final must be an object");
  }
  // A Final Jeopardy without a clue means the game has none
  if (final.clue === undefined || final.clue === "") {
    return null;
  }
  if (
    !isNonEmptyString(final.category) ||
    !isNonEmptyString(final.clue) ||
    !isNonEmptyString(final.solution)
  ) {
    throw new GameFileError(
      'Invalid Final Jeopardy: expected a "category", a "clue" and a "solution"'
    );
  }

  return { category: final.category, question: final.clue, answer: final.solution };
}

function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isNonEmptyString(value: unknown): value is string {
  return typeof value === "string" && value.trim() !== "";
}
