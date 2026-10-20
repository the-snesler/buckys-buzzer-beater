import type { FinalClue, GameCategory, ParsedGame } from "./gameFile";

/**
 * The game being built in the Game Builder. It's kept in sessionStorage, so leaving the
 * builder, going back, or reloading doesn't lose it.
 */
export interface BuilderDraft {
  single: GameCategory[];
  /** Null while the game has no Double Jeopardy */
  double: GameCategory[] | null;
  /** Null while the game has no Final Jeopardy */
  final: FinalClue | null;
}

const STORAGE_KEY = "game_builder_draft";

/** The title a new category gets, such as "Category 3" */
export function defaultCategoryTitle(index: number): string {
  return `Category ${index + 1}`;
}

/** Whether anything in a round has been typed or ticked, including a renamed category */
export function roundHasContent(categories: GameCategory[]): boolean {
  return categories.some(
    (cat, i) =>
      cat.title !== defaultCategoryTitle(i) ||
      cat.questions.some((q) => q.question.trim() || q.answer.trim() || q.dailyDouble)
  );
}

/** Whether anything has been typed into Final Jeopardy */
export function finalHasContent(final: FinalClue): boolean {
  return Boolean(final.category.trim() || final.question.trim() || final.answer.trim());
}

/** Whether a draft has anything in it that would be lost by replacing it */
export function draftHasContent(draft: BuilderDraft): boolean {
  return (
    roundHasContent(draft.single) ||
    (draft.double !== null && roundHasContent(draft.double)) ||
    (draft.final !== null && finalHasContent(draft.final))
  );
}

/** A draft to edit an existing game, such as one opened from a file */
export function draftFromGame(game: Pick<ParsedGame, "single" | "double" | "final">): BuilderDraft {
  return {
    single: game.single,
    double: game.double.length > 0 ? game.double : null,
    final: game.final,
  };
}

/** The draft saved in this tab, if there is one */
export function loadBuilderDraft(): BuilderDraft | null {
  try {
    const saved: unknown = JSON.parse(sessionStorage.getItem(STORAGE_KEY) ?? "null");
    return isDraft(saved) ? saved : null;
  } catch {
    return null;
  }
}

/** Saves the draft for this tab, or forgets it if there's nothing in it */
export function saveBuilderDraft(draft: BuilderDraft): void {
  try {
    if (draftHasContent(draft)) {
      sessionStorage.setItem(STORAGE_KEY, JSON.stringify(draft));
    } else {
      sessionStorage.removeItem(STORAGE_KEY);
    }
  } catch {
    // Storage may be full or disabled; the builder still works without it
  }
}

function isDraft(value: unknown): value is BuilderDraft {
  if (typeof value !== "object" || value === null) return false;
  const { single, double, final } = value as Record<string, unknown>;
  return (
    isCategories(single) &&
    (double === null || isCategories(double)) &&
    (final === null || isFinal(final))
  );
}

function isCategories(value: unknown): value is GameCategory[] {
  return (
    Array.isArray(value) &&
    value.every(
      (cat) =>
        typeof cat?.title === "string" &&
        Array.isArray(cat.questions) &&
        cat.questions.every(
          (q: Record<string, unknown> | null) =>
            typeof q?.question === "string" &&
            typeof q.answer === "string" &&
            typeof q.value === "number" &&
            typeof q.answered === "boolean" &&
            typeof q.dailyDouble === "boolean"
        )
    )
  );
}

function isFinal(value: unknown): value is FinalClue {
  if (typeof value !== "object" || value === null) return false;
  const { category, question, answer } = value as Record<string, unknown>;
  return typeof category === "string" && typeof question === "string" && typeof answer === "string";
}
