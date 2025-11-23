import { useState } from 'react';
import { useNavigate } from 'react-router-dom';
import type { Category } from '../lib/api';

const DEFAULT_VALUES = [200, 400, 600, 800, 1000];

interface Clue {
  value: number;
  clue: string;
  solution: string;
}

interface GameCategory {
  title: string;
  clues: Clue[];
}

function createEmptyCategory(index: number): GameCategory {
  return {
    title: `Category ${index + 1}`,
    clues: DEFAULT_VALUES.map((value) => ({
      value,
      clue: '',
      solution: '',
    })),
  };
}

function createEmptyGame(): GameCategory[] {
  return Array.from({ length: 6 }, (_, i) => createEmptyCategory(i));
}

export default function GameBuilder() {
  const navigate = useNavigate();
  const [categories, setCategories] = useState<GameCategory[]>(createEmptyGame);

  const updateCategoryTitle = (catIndex: number, title: string) => {
    setCategories((prev) =>
      prev.map((cat, i) => (i === catIndex ? { ...cat, title } : cat))
    );
  };

  const updateClue = (
    catIndex: number,
    clueIndex: number,
    field: 'clue' | 'solution',
    value: string
  ) => {
    setCategories((prev) =>
      prev.map((cat, i) =>
        i === catIndex
          ? {
              ...cat,
              clues: cat.clues.map((c, j) =>
                j === clueIndex ? { ...c, [field]: value } : c
              ),
            }
          : cat
      )
    );
  };

  const addCategory = () => {
    setCategories((prev) => [...prev, createEmptyCategory(prev.length)]);
  };

  const removeCategory = (index: number) => {
    if (categories.length <= 1) return;
    setCategories((prev) => prev.filter((_, i) => i !== index));
  };

  const downloadJson = () => {
    const gameData = {
      game: {
        single: categories.map((cat) => ({
          category: cat.title,
          clues: cat.clues.map((c) => ({
            value: c.value,
            clue: c.clue,
            solution: c.solution,
          })),
        })),
      },
    };

    const blob = new Blob([JSON.stringify(gameData, null, 2)], {
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
    const transformed: Category[] = categories.map((cat) => ({
      title: cat.title,
      questions: cat.clues.map((c) => ({
        question: c.clue,
        answer: c.solution,
        value: c.value,
        answered: false,
      })),
    }));

    navigate('/', { state: { categories: transformed, fromBuilder: true } });
  };

  const isValid = categories.every(
    (cat) =>
      cat.title.trim() &&
      cat.clues.every((c) => c.clue.trim() && c.solution.trim())
  );

  return (
    <div className="min-h-screen bg-gray-900 py-8 px-4">
      <div className="max-w-6xl mx-auto">
        <div className="flex items-center justify-between mb-8">
          <h1 className="text-3xl font-bold text-white">Game Builder</h1>
          <div className="flex gap-4">
            <button
              onClick={() => navigate('/')}
              className="px-4 py-2 bg-gray-700 text-white rounded hover:bg-gray-600"
            >
              Back to Lobby
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
          <p className="text-yellow-400 mb-4">
            Fill in all category titles, clues, and solutions to use this game.
          </p>
        )}

        <div className="space-y-6">
          {categories.map((category, catIndex) => (
            <div
              key={catIndex}
              className="bg-gray-800 rounded-lg p-6 border border-gray-700"
            >
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
                {category.clues.map((clue, clueIndex) => (
                  <div
                    key={clueIndex}
                    className="grid grid-cols-[80px_1fr_1fr] gap-3 items-start"
                  >
                    <div className="px-3 py-2 bg-yellow-600 text-white rounded text-center font-bold">
                      ${clue.value}
                    </div>
                    <input
                      type="text"
                      value={clue.clue}
                      onChange={(e) =>
                        updateClue(catIndex, clueIndex, 'clue', e.target.value)
                      }
                      placeholder="Enter clue..."
                      className="px-3 py-2 bg-gray-700 text-white rounded"
                    />
                    <input
                      type="text"
                      value={clue.solution}
                      onChange={(e) =>
                        updateClue(catIndex, clueIndex, 'solution', e.target.value)
                      }
                      placeholder="Enter answer..."
                      className="px-3 py-2 bg-gray-700 text-white rounded"
                    />
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
      </div>
    </div>
  );
}
