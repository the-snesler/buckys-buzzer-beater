import type { Question } from "../types/game";

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
 *
 * The Jeopardy round must be valid, since the game can't be played without it. A
 * malformed Double Jeopardy or Final Jeopardy is left out with a warning instead, so
 * that files with incomplete optional rounds (common in scraped games) still load.
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
  /** Problems with optional rounds that were left out of the game */
  warnings: string[];
}

export class GameFileError extends Error {}

/**
 * Parses and validates the JSON of a game file. Throws a GameFileError if the Jeopardy
 * round can't be played.
 */
export function parseGameFile(json: unknown): ParsedGame {
  const game = isObject(json) ? json.game : undefined;
  if (!isObject(game)) {
    throw new GameFileError('Invalid format: expected a "game" object');
  }

  if (!Array.isArray(game.single) || game.single.length === 0) {
    throw new GameFileError('Invalid format: expected a non-empty "game.single" list of categories');
  }
  let single: GameCategory[];
  try {
    single = parseRound(game.single);
  } catch (err) {
    if (err instanceof GameFileError) {
      throw new GameFileError(`Jeopardy round: ${err.message}`);
    }
    throw err;
  }

  const warnings: string[] = [];

  let double: GameCategory[] = [];
  try {
    double = parseDouble(game.double);
  } catch (err) {
    if (!(err instanceof GameFileError)) throw err;
    warnings.push(`Double Jeopardy skipped: ${err.message}`);
  }

  let final: FinalClue | null = null;
  try {
    final = parseFinal(game.final);
  } catch (err) {
    if (!(err instanceof GameFileError)) throw err;
    warnings.push(`Final Jeopardy skipped: ${err.message}`);
  }

  return { single, double, final, warnings };
}

/** A clue as it's written in a game file */
export interface GameFileClue {
  value: number;
  clue: string;
  solution: string;
  dailyDouble?: boolean;
}

export interface GameFileCategory {
  category: string;
  clues: GameFileClue[];
}

/** The JSON of a game file, in the format described at the top of this file */
export interface GameFile {
  game: {
    single: GameFileCategory[];
    double?: GameFileCategory[];
    final?: { category: string; clue: string; solution: string };
  };
}

/**
 * Writes a game as a game file, which parseGameFile reads back as the same game. A game
 * without Double Jeopardy or Final Jeopardy leaves that section out, and only clues that
 * hide a Daily Double say so.
 */
export function toGameFile(game: Pick<ParsedGame, "single" | "double" | "final">): GameFile {
  const toFileRound = (categories: GameCategory[]): GameFileCategory[] =>
    categories.map((category) => ({
      category: category.title,
      clues: category.questions.map(({ question, answer, value, dailyDouble }) => ({
        value,
        clue: question,
        solution: answer,
        ...(dailyDouble ? { dailyDouble: true } : {}),
      })),
    }));

  return {
    game: {
      single: toFileRound(game.single),
      ...(game.double.length > 0 ? { double: toFileRound(game.double) } : {}),
      ...(game.final
        ? {
            final: {
              category: game.final.category,
              clue: game.final.question,
              solution: game.final.answer,
            },
          }
        : {}),
    },
  };
}

/** Something that stops one round of a game from being played */
export interface GameProblem {
  round: "single" | "double" | "final";
  /** Such as "Category 2, clue 3 has no solution" */
  message: string;
}

/**
 * What stops a game from being played with every round it has: at most one problem per
 * round.
 *
 * Unlike parseGameFile, which leaves out a broken Double Jeopardy or Final Jeopardy so a
 * scraped game still loads, this counts them as problems. It's for the Game Builder,
 * whose author added those rounds on purpose.
 */
export function findGameProblems(
  game: Pick<ParsedGame, "single" | "double" | "final">
): GameProblem[] {
  const { single, double, final } = toGameFile(game).game;
  const problems: GameProblem[] = [];
  const check = (round: GameProblem["round"], parse: () => unknown) => {
    try {
      parse();
    } catch (err) {
      if (!(err instanceof GameFileError)) throw err;
      problems.push({ round, message: err.message });
    }
  };

  check("single", () => {
    if (single.length === 0) throw new GameFileError("it has no categories");
    parseRound(single);
  });
  if (double) check("double", () => parseRound(double));
  if (final) check("final", () => parseFinalClue(final));
  return problems;
}

/**
 * The categories of a round, as sent to the server. The server keeps the Daily Double
 * flags to itself, so they never come back in the game state.
 */
export function toServerCategories(categories: GameCategory[]): GameCategory[] {
  return categories.map((category) => ({
    title: category.title,
    questions: category.questions.map(({ question, answer, value, answered, dailyDouble }) => ({
      question,
      answer,
      value,
      answered,
      dailyDouble,
    })),
  }));
}

/** How many Daily Doubles the game file hides across its rounds */
export function countDailyDoubles(game: ParsedGame): number {
  return [...game.single, ...game.double]
    .flatMap((category) => category.questions)
    .filter((question) => question.dailyDouble).length;
}

/**
 * A one-line summary of what the game contains, such as
 * "6 categories + Double Jeopardy + Final Jeopardy, 3 Daily Doubles"
 */
export function describeGame(game: ParsedGame): string {
  const categories = `${game.single.length} ${game.single.length === 1 ? "category" : "categories"}`;
  const withDouble = game.double.length > 0 ? `${categories} + Double Jeopardy` : categories;
  const rounds = game.final ? `${withDouble} + Final Jeopardy` : withDouble;
  const dailyDoubles = countDailyDoubles(game);
  if (dailyDoubles === 0) return rounds;
  return `${rounds}, ${dailyDoubles} Daily Double${dailyDoubles === 1 ? "" : "s"}`;
}

function parseDouble(double: unknown): GameCategory[] {
  if (double === undefined || double === null) {
    return [];
  }
  if (!Array.isArray(double)) {
    throw new GameFileError('"game.double" is not a list of categories');
  }
  return parseRound(double);
}

/** Parses a round's categories. Error messages count categories and clues from 1. */
function parseRound(round: unknown[]): GameCategory[] {
  return round.map((category, catIdx) => {
    const where = `Category ${catIdx + 1}`;
    if (!isObject(category)) {
      throw new GameFileError(`${where} is not a category`);
    }
    if (!isNonEmptyString(category.category)) {
      throw new GameFileError(`${where} has no name`);
    }
    if (!Array.isArray(category.clues) || category.clues.length === 0) {
      throw new GameFileError(`${where} has no clues`);
    }

    return {
      title: category.category,
      questions: category.clues.map((clue, clueIdx) =>
        parseClue(clue, `${where}, clue ${clueIdx + 1}`)
      ),
    };
  });
}

function parseClue(clue: unknown, where: string): GameQuestion {
  if (!isObject(clue)) {
    throw new GameFileError(`${where} is not a clue`);
  }
  if (!isNonEmptyString(clue.clue)) {
    throw new GameFileError(`${where} has no clue text`);
  }
  if (!isNonEmptyString(clue.solution)) {
    throw new GameFileError(`${where} has no solution`);
  }
  if (typeof clue.value !== "number" || !Number.isInteger(clue.value) || clue.value < 0) {
    throw new GameFileError(`${where} has an invalid value (it must be a whole number of dollars)`);
  }
  if (clue.dailyDouble !== undefined && typeof clue.dailyDouble !== "boolean") {
    throw new GameFileError(`${where} has an invalid "dailyDouble" (it must be true or false)`);
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
  // A Final Jeopardy without a clue means the game has none
  if (
    isObject(final) &&
    (final.clue === undefined || (typeof final.clue === "string" && final.clue.trim() === ""))
  ) {
    return null;
  }
  return parseFinalClue(final);
}

/** Parses a Final Jeopardy that must have a clue */
function parseFinalClue(final: unknown): FinalClue {
  if (!isObject(final)) {
    throw new GameFileError('"game.final" is not an object');
  }
  if (final.clue === undefined || (typeof final.clue === "string" && final.clue.trim() === "")) {
    throw new GameFileError("it has no clue");
  }
  if (!isNonEmptyString(final.clue)) {
    throw new GameFileError("the clue is not text");
  }
  if (!isNonEmptyString(final.category)) {
    throw new GameFileError("it has no category");
  }
  if (!isNonEmptyString(final.solution)) {
    throw new GameFileError("it has no solution");
  }

  return { category: final.category, question: final.clue, answer: final.solution };
}

function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isNonEmptyString(value: unknown): value is string {
  return typeof value === "string" && value.trim() !== "";
}
