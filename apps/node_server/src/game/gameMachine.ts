import { setup, assign } from "xstate";

// Types for game data
export interface Question {
  question: string;
  answer: string;
  value: number;
  answered: boolean;
}

export interface Category {
  title: string;
  questions: Question[];
}

export interface PlayerState {
  pid: number;
  name: string;
  score: number;
}

export interface GameContext {
  categories: Category[];
  players: PlayerState[];
  currentQuestion: [number, number] | null; // [categoryIndex, questionIndex]
  currentBuzzer: number | null;
  excludedPlayers: number[]; // players who answered incorrectly for current question
}

// Input type for initializing the machine
export interface GameInput {
  categories: Category[];
  players: PlayerState[];
}

// Events
export type GameEvent =
  | { type: "HOST_CHOICE"; categoryIndex: number; questionIndex: number }
  | { type: "HOST_READY" }
  | { type: "PLAYER_BUZZ"; pid: number }
  | { type: "HOST_CORRECT" }
  | { type: "HOST_INCORRECT" }
  | { type: "ADD_PLAYER"; pid: number; name: string }
  | { type: "REMOVE_PLAYER"; pid: number }
  | { type: "UPDATE_PLAYER_SCORE"; pid: number; score: number };

// Helper to check if questions remain
function hasQuestionsRemaining(categories: Category[]): boolean {
  return categories.some((cat) =>
    cat.questions.some((q) => !q.answered)
  );
}

// Helper to get eligible players who can still buzz
export function getEligiblePlayers(
  players: PlayerState[],
  excludedPlayers: number[]
): PlayerState[] {
  return players.filter((p) => !excludedPlayers.includes(p.pid));
}

// The game state machine
export const gameMachine = setup({
  types: {
    context: {} as GameContext,
    events: {} as GameEvent,
    input: {} as GameInput,
  },
  guards: {
    questionsRemain: ({ context }) => hasQuestionsRemaining(context.categories),
    noQuestionsRemain: ({ context }) =>
      !hasQuestionsRemaining(context.categories),
    playersCanStillBuzz: ({ context }) => {
      const eligible = getEligiblePlayers(
        context.players,
        context.excludedPlayers
      );
      console.log("Eligible players for buzzing:", eligible);
      return eligible.length > 0;
    },
    allPlayersBuzzed: ({ context }) => {
      const eligible = getEligiblePlayers(
        context.players,
        context.excludedPlayers
      );
      return eligible.length === 0;
    },
  },
  actions: {
    selectQuestion: assign({
      currentQuestion: ({ event }) => {
        if (event.type !== "HOST_CHOICE") return null;
        return [event.categoryIndex, event.questionIndex] as [number, number];
      },
      excludedPlayers: () => [], // Reset excluded players for new question
      currentBuzzer: () => null,
    }),
    recordBuzz: assign({
      currentBuzzer: ({ event }) => {
        if (event.type !== "PLAYER_BUZZ") return null;
        return event.pid;
      },
    }),
    awardPoints: assign({
      players: ({ context }) => {
        console.log(
          "Awarding points to current buzzer:",
          context.currentBuzzer,
          context.currentQuestion
        );
        if (
          context.currentBuzzer === null ||
          context.currentQuestion === null
        ) {
          return context.players;
        }
        const [catIdx, qIdx] = context.currentQuestion;
        const pointValue =
          context.categories[catIdx]?.questions[qIdx]?.value ?? 0;
        return context.players.map((p) =>
          p.pid === context.currentBuzzer
            ? { ...p, score: p.score + pointValue }
            : p
        );
      },
      categories: ({ context }) => {
        if (context.currentQuestion === null) return context.categories;
        const [catIdx, qIdx] = context.currentQuestion;
        return context.categories.map((cat, ci) =>
          ci === catIdx
            ? {
                ...cat,
                questions: cat.questions.map((q, qi) =>
                  qi === qIdx ? { ...q, answered: true } : q
                ),
              }
            : cat
        );
      },
    }),
    markQuestionAnswered: assign({
      categories: ({ context }) => {
        if (context.currentQuestion === null) return context.categories;
        const [catIdx, qIdx] = context.currentQuestion;
        return context.categories.map((cat, ci) =>
          ci === catIdx
            ? {
                ...cat,
                questions: cat.questions.map((q, qi) =>
                  qi === qIdx ? { ...q, answered: true } : q
                ),
              }
            : cat
        );
      },
    }),
    excludeCurrentBuzzer: assign({
      excludedPlayers: ({ context }) => {
        if (context.currentBuzzer === null) return context.excludedPlayers;
        return [...context.excludedPlayers, context.currentBuzzer];
      },
    }),
    clearCurrentBuzzer: assign({
      currentBuzzer: () => null,
    }),
    clearCurrentQuestion: assign({
      currentQuestion: () => null,
      currentBuzzer: () => null,
      excludedPlayers: () => [],
    }),
    addPlayer: assign({
      players: ({ context, event }) => {
        if (event.type !== "ADD_PLAYER") return context.players;
        return [
          ...context.players,
          { pid: event.pid, name: event.name, score: 0 },
        ];
      },
    }),
    removePlayer: assign({
      players: ({ context, event }) => {
        if (event.type !== "REMOVE_PLAYER") return context.players;
        return context.players.filter((p) => p.pid !== event.pid);
      },
    }),
  },
}).createMachine({
  id: "jeopardyGame",
  initial: "selection",
  context: ({ input }) => ({
    categories: input.categories,
    players: input.players,
    currentQuestion: null,
    currentBuzzer: null,
    excludedPlayers: [],
  }),
  states: {
    selection: {
      on: {
        HOST_CHOICE: {
          target: "questionReading",
          actions: "selectQuestion",
        },
        ADD_PLAYER: {
          actions: "addPlayer",
        },
        REMOVE_PLAYER: {
          actions: "removePlayer",
        },
      },
    },
    questionReading: {
      on: {
        HOST_READY: {
          target: "waitingForBuzz",
        },
      },
    },
    waitingForBuzz: {
      on: {
        PLAYER_BUZZ: {
          target: "answer",
          actions: ["recordBuzz", "excludeCurrentBuzzer"],
        },
      },
    },
    answer: {
      on: {
        HOST_CORRECT: [
          {
            target: "gameEnd",
            guard: "noQuestionsRemain",
            actions: ["awardPoints", "clearCurrentQuestion"],
          },
          {
            target: "selection",
            guard: "questionsRemain",
            actions: ["awardPoints", "clearCurrentQuestion"],
          },
        ],
        HOST_INCORRECT: [
          {
            target: "waitingForBuzz",
            guard: "playersCanStillBuzz",
            actions: ["clearCurrentBuzzer"]
          },
          {
            target: "gameEnd",
            guard: "noQuestionsRemain",
            actions: ["markQuestionAnswered", "clearCurrentQuestion"],
          },
          {
            target: "selection",
            actions: ["markQuestionAnswered", "clearCurrentQuestion"],
          },
        ],
      },
    },
    gameEnd: {
      type: "final",
    },
  },
});

// Export a type for the serialized game state sent to host
export interface GameStateSnapshot {
  state: string;
  categories: Category[];
  players: PlayerState[];
  currentQuestion: [number, number] | null;
  currentBuzzer: number | null;
}

export function createGameStateSnapshot(
  state: string,
  context: GameContext
): GameStateSnapshot {
  return {
    state,
    categories: context.categories,
    players: context.players,
    currentQuestion: context.currentQuestion,
    currentBuzzer: context.currentBuzzer,
  };
}
