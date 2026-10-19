import { useEffect, useState } from 'react';
import { useLocation, useNavigate } from 'react-router-dom';
import {
  defaultCategoryTitle,
  draftFromGame,
  draftHasContent,
  finalHasContent,
  loadBuilderDraft,
  roundHasContent,
  saveBuilderDraft,
  type BuilderDraft,
} from '../lib/builderDraft';
import {
  countRoundDailyDoubles,
  findGameProblems,
  parseGameFile,
  toGameFile,
  type FinalClue,
  type GameCategory,
  type GameQuestion,
} from '../lib/gameFile';
import type { BuilderLocationState, LobbyLocationState } from '../types/navigation';

type Round = 'single' | 'double';
type Tab = Round | 'final';

/** The clue values of a new category in each round */
const DEFAULT_VALUES: Record<Round, number[]> = {
  single: [200, 400, 600, 800, 1000],
  double: [400, 800, 1200, 1600, 2000],
};

/** How many Daily Doubles TV hides in each round. More is allowed, just unusual. */
const TV_DAILY_DOUBLES: Record<Round, number> = { single: 1, double: 2 };

const ROUND_NAMES: Record<Tab, string> = {
  single: 'Jeopardy',
  double: 'Double Jeopardy',
  final: 'Final Jeopardy',
};

function createEmptyCategory(round: Round, index: number): GameCategory {
  return {
    title: defaultCategoryTitle(index),
    questions: DEFAULT_VALUES[round].map((value) => ({
      question: '',
      answer: '',
      value,
      answered: false,
      dailyDouble: false,
    })),
  };
}

function createEmptyRound(round: Round): GameCategory[] {
  return Array.from({ length: 6 }, (_, i) => createEmptyCategory(round, i));
}

function createEmptyDraft(): BuilderDraft {
  return { single: createEmptyRound('single'), double: null, final: null };
}

export default function GameBuilder() {
  const navigate = useNavigate();
  const location = useLocation();
  // The game the lobby asked to edit, else the draft saved in this tab, else a new game
  const [initial] = useState<BuilderDraft>(() => {
    const requested = (location.state as BuilderLocationState | null)?.game;
    return requested ? draftFromGame(requested) : loadBuilderDraft() ?? createEmptyDraft();
  });
  const [tab, setTab] = useState<Tab>('single');
  const [single, setSingle] = useState<GameCategory[]>(initial.single);
  /** Null while the game has no Double Jeopardy */
  const [double, setDouble] = useState<GameCategory[] | null>(initial.double);
  /** Null while the game has no Final Jeopardy */
  const [final, setFinal] = useState<FinalClue | null>(initial.final);

  // Keep the draft, so leaving the builder, going back or reloading doesn't lose it
  useEffect(() => {
    saveBuilderDraft({ single, double, final });
  }, [single, double, final]);

  // The draft now holds any game the lobby passed in, so a reload shouldn't load it again
  useEffect(() => {
    if (location.state) {
      window.history.replaceState({}, document.title);
    }
  }, [location.state]);

  const loadDraft = (draft: BuilderDraft) => {
    setSingle(draft.single);
    setDouble(draft.double);
    setFinal(draft.final);
    setTab('single');
  };

  const newGame = () => {
    if (
      draftHasContent({ single, double, final }) &&
      !window.confirm('Start a new game? Everything in this one will be cleared.')
    ) {
      return;
    }
    loadDraft(createEmptyDraft());
  };

  const game = { single, double: double ?? [], final };
  const problems = findGameProblems(game);
  const isValid = problems.length === 0;

  const removeDouble = () => {
    if (
      double &&
      roundHasContent(double) &&
      !window.confirm('Remove Double Jeopardy and every clue in it?')
    ) {
      return;
    }
    setDouble(null);
  };

  const removeFinal = () => {
    if (
      final &&
      finalHasContent(final) &&
      !window.confirm('Remove Final Jeopardy and its clue?')
    ) {
      return;
    }
    setFinal(null);
  };

  const downloadJson = () => {
    const blob = new Blob([JSON.stringify(toGameFile(game), null, 2)], {
      type: 'application/json',
    });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = 'game.json';
    a.click();
    URL.revokeObjectURL(url);
  };

  const useGame = () => {
    // Read back through the game file format, so the lobby gets exactly what it would
    // from a downloaded copy of this game
    const state: LobbyLocationState = { game: parseGameFile(toGameFile(game)), fromBuilder: true };
    navigate('/', { state });
  };

  const tabStatus = (t: Tab): string | null => {
    if (t === 'double' && !double) return 'off';
    if (t === 'final' && !final) return 'off';
    return null;
  };
  const tabHasProblem = (t: Tab) => problems.some((p) => p.round === t);

  return (
    <div className="min-h-screen bg-gray-900 py-8 px-4">
      <div className="max-w-6xl mx-auto">
        <div className="flex flex-wrap items-center justify-between gap-4 mb-8">
          <h1 className="text-3xl font-bold text-white">Game Builder</h1>
          <div className="flex flex-wrap gap-2 sm:gap-4">
            <button
              onClick={() => navigate('/')}
              className="px-4 py-2 bg-gray-700 text-white rounded hover:bg-gray-600"
            >
              Back to Lobby
            </button>
            <button
              onClick={newGame}
              className="px-4 py-2 bg-gray-700 text-white rounded hover:bg-gray-600"
            >
              New Game
            </button>
            <button
              onClick={downloadJson}
              className="px-4 py-2 bg-blue-600 text-white rounded hover:bg-blue-700"
            >
              Download JSON
            </button>
            <button
              onClick={useGame}
              disabled={!isValid}
              className="px-4 py-2 bg-green-600 text-white rounded hover:bg-green-700 disabled:opacity-50 disabled:cursor-not-allowed"
            >
              Use Game
            </button>
          </div>
        </div>

        {!isValid && (
          <div className="text-yellow-400 mb-4">
            <p>Before you can use this game:</p>
            <ul className="mt-1 text-sm text-yellow-300/80 list-disc list-inside">
              {problems.map(({ round, message }) => (
                <li key={round}>
                  {ROUND_NAMES[round]}: {message}
                </li>
              ))}
            </ul>
          </div>
        )}

        <div role="tablist" className="flex gap-1 border-b border-gray-700 mb-6">
          {(['single', 'double', 'final'] as const).map((t) => (
            <button
              key={t}
              role="tab"
              aria-selected={tab === t}
              onClick={() => setTab(t)}
              className={`px-4 py-2 rounded-t font-semibold flex items-center gap-2 ${
                tab === t
                  ? 'bg-gray-800 text-white border border-b-0 border-gray-700'
                  : 'text-gray-400 hover:text-white'
              }`}
            >
              {ROUND_NAMES[t]}
              {tabStatus(t) && (
                <span className="text-xs font-normal text-gray-500">({tabStatus(t)})</span>
              )}
              {tabHasProblem(t) && (
                <span
                  className="w-2 h-2 rounded-full bg-yellow-400"
                  title="Something here still needs filling in"
                />
              )}
            </button>
          ))}
        </div>

        {tab === 'single' && (
          <RoundEditor round="single" categories={single} onChange={setSingle} />
        )}

        {tab === 'double' &&
          (double ? (
            <>
              <RoundHeader
                text="Double Jeopardy is played after Jeopardy, with its own board."
                removeLabel="Remove Double Jeopardy"
                onRemove={removeDouble}
              />
              <RoundEditor
                round="double"
                categories={double}
                onChange={(update) => setDouble((prev) => prev && update(prev))}
              />
            </>
          ) : (
            <OptionalRoundOff
              text="This game has no Double Jeopardy round. It goes straight from Jeopardy to Final Jeopardy, or ends."
              addLabel="Add Double Jeopardy"
              onAdd={() => setDouble(createEmptyRound('double'))}
            />
          ))}

        {tab === 'final' &&
          (final ? (
            <>
              <RoundHeader
                text="Everyone wagers on the category, then answers the clue in writing."
                removeLabel="Remove Final Jeopardy"
                onRemove={removeFinal}
              />
              <FinalEditor final={final} onChange={setFinal} />
            </>
          ) : (
            <OptionalRoundOff
              text="This game has no Final Jeopardy. It ends after the last round."
              addLabel="Add Final Jeopardy"
              onAdd={() => setFinal({ category: '', question: '', answer: '' })}
            />
          ))}
      </div>
    </div>
  );
}

function OptionalRoundOff({
  text,
  addLabel,
  onAdd,
}: {
  text: string;
  addLabel: string;
  onAdd: () => void;
}) {
  return (
    <div className="bg-gray-800 rounded-lg p-8 border border-gray-700 text-center">
      <p className="text-gray-300 mb-4">{text}</p>
      <button
        onClick={onAdd}
        className="px-4 py-2 bg-blue-600 text-white rounded hover:bg-blue-700"
      >
        + {addLabel}
      </button>
    </div>
  );
}

function RoundHeader({
  text,
  removeLabel,
  onRemove,
}: {
  text: string;
  removeLabel: string;
  onRemove: () => void;
}) {
  return (
    <div className="flex flex-wrap items-center justify-between gap-4 mb-4">
      <p className="text-gray-400">{text}</p>
      <button
        onClick={onRemove}
        className="px-3 py-2 text-red-300 border border-red-700 rounded hover:bg-red-900/40"
      >
        {removeLabel}
      </button>
    </div>
  );
}

function RoundEditor({
  round,
  categories,
  onChange,
}: {
  round: Round;
  categories: GameCategory[];
  onChange: (update: (prev: GameCategory[]) => GameCategory[]) => void;
}) {
  const dailyDoubles = countRoundDailyDoubles(categories);
  const usual = TV_DAILY_DOUBLES[round];

  const updateCategoryTitle = (catIndex: number, title: string) => {
    onChange((prev) => prev.map((cat, i) => (i === catIndex ? { ...cat, title } : cat)));
  };

  const updateClue = (catIndex: number, clueIndex: number, change: Partial<GameQuestion>) => {
    onChange((prev) =>
      prev.map((cat, i) =>
        i === catIndex
          ? {
              ...cat,
              questions: cat.questions.map((q, j) => (j === clueIndex ? { ...q, ...change } : q)),
            }
          : cat
      )
    );
  };

  const addCategory = () => {
    onChange((prev) => [...prev, createEmptyCategory(round, prev.length)]);
  };

  const removeCategory = (index: number) => {
    onChange((prev) => (prev.length <= 1 ? prev : prev.filter((_, i) => i !== index)));
  };

  return (
    <>
      <p className="text-sm text-gray-400 mb-4">
        {dailyDoubles === 0
          ? `No Daily Doubles. Tick "Daily Double" on a clue to hide one there, or let the lobby place ${usual} at random.`
          : `${dailyDoubles} Daily Double${dailyDoubles === 1 ? '' : 's'} in this round.`}
        {dailyDoubles > usual && (
          <span className="text-yellow-300/80">
            {' '}
            That's more than usual: TV hides {usual} in {ROUND_NAMES[round]}.
          </span>
        )}
      </p>

      <div className="space-y-6">
        {categories.map((category, catIndex) => (
          <div key={catIndex} className="bg-gray-800 rounded-lg p-6 border border-gray-700">
            <div className="flex items-center gap-4 mb-4">
              <input
                type="text"
                value={category.title}
                onChange={(e) => updateCategoryTitle(catIndex, e.target.value)}
                placeholder="Category Title"
                className="flex-1 px-4 py-2 bg-gray-700 text-white rounded text-lg font-semibold"
              />
              <button
                onClick={() => removeCategory(catIndex)}
                disabled={categories.length <= 1}
                className="px-3 py-2 bg-red-600 text-white rounded hover:bg-red-700 disabled:opacity-50 disabled:cursor-not-allowed"
              >
                Remove
              </button>
            </div>

            <div className="space-y-3">
              {category.questions.map((clue, clueIndex) => (
                <div
                  key={clueIndex}
                  className="grid grid-cols-[80px_1fr_1fr_auto] gap-3 items-start"
                >
                  <div
                    className={`px-3 py-2 text-white rounded text-center font-bold ${
                      clue.dailyDouble ? 'bg-blue-600' : 'bg-yellow-600'
                    }`}
                  >
                    ${clue.value}
                  </div>
                  <input
                    type="text"
                    value={clue.question}
                    onChange={(e) => updateClue(catIndex, clueIndex, { question: e.target.value })}
                    placeholder="Enter clue..."
                    className="px-3 py-2 bg-gray-700 text-white rounded"
                  />
                  <input
                    type="text"
                    value={clue.answer}
                    onChange={(e) => updateClue(catIndex, clueIndex, { answer: e.target.value })}
                    placeholder="Enter answer..."
                    className="px-3 py-2 bg-gray-700 text-white rounded"
                  />
                  <label className="flex items-center gap-2 py-2 text-sm text-gray-300 whitespace-nowrap cursor-pointer">
                    <input
                      type="checkbox"
                      checked={clue.dailyDouble}
                      onChange={(e) =>
                        updateClue(catIndex, clueIndex, { dailyDouble: e.target.checked })
                      }
                      className="w-4 h-4 accent-blue-600"
                    />
                    Daily Double
                  </label>
                </div>
              ))}
            </div>
          </div>
        ))}
      </div>

      <button
        onClick={addCategory}
        className="mt-6 w-full py-3 bg-gray-700 text-white rounded hover:bg-gray-600 border-2 border-dashed border-gray-600"
      >
        + Add Category
      </button>
    </>
  );
}

function FinalEditor({
  final,
  onChange,
}: {
  final: FinalClue;
  onChange: (final: FinalClue) => void;
}) {
  return (
    <div className="bg-gray-800 rounded-lg p-6 border border-gray-700 space-y-4">
      <label className="block">
        <span className="block text-sm text-gray-400 mb-1">Category</span>
        <input
          type="text"
          value={final.category}
          onChange={(e) => onChange({ ...final, category: e.target.value })}
          placeholder="Shown while players wager"
          className="w-full px-4 py-2 bg-gray-700 text-white rounded text-lg font-semibold"
        />
      </label>
      <label className="block">
        <span className="block text-sm text-gray-400 mb-1">Clue</span>
        <textarea
          value={final.question}
          onChange={(e) => onChange({ ...final, question: e.target.value })}
          placeholder="Enter clue..."
          rows={3}
          className="w-full px-3 py-2 bg-gray-700 text-white rounded"
        />
      </label>
      <label className="block">
        <span className="block text-sm text-gray-400 mb-1">Solution</span>
        <input
          type="text"
          value={final.answer}
          onChange={(e) => onChange({ ...final, answer: e.target.value })}
          placeholder="Enter answer..."
          className="w-full px-3 py-2 bg-gray-700 text-white rounded"
        />
      </label>
    </div>
  );
}
