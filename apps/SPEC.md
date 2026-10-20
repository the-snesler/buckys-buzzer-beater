This is the backend for a Jeopardy webgame with Jackbox-like player connections. The server manages game state, player connections, and real-time updates via WebSockets. The frontend provides an interactive UI for players to join games, select answers, and view scores.

A game has a Jeopardy round, an optional Double Jeopardy round with its own board, and an optional Final Jeopardy. Any clue can hide a Daily Double. The server is the authority on scores and wagers, and each client only gets what its role is allowed to see (see [Privacy](#privacy)).

# Flow

1. Host creates a room via POST /api/v1/rooms/create. They receive a room code & token.
  a. The body of this POST request is a `CreateRoomRequest` holding the boards, the Final Jeopardy clue, and options (see [Creating a room](#creating-a-room)).
2. Host connects to WebSocket at /api/v1/rooms/:code/ws?token={host_token} to become the host.
  a. If the host gets disconnected, they can reconnect using the same token. Once the game has started, the server sends them the current GameState straight away.
3. Players join the room by connecting to /api/v1/rooms/:code/ws?playerName={name}
4. Upon connection, server sends PlayerList to host and NewPlayer to the joining player.
  a. Server assigns each player a unique pid and token, which the player must store for reconnection.
  b. If a player disconnects, they can reconnect using `/rooms/:code/ws?playerID={pid}&token={token}`
  c. Players can join at any point, including mid-game, with a score of $0.
5. Host displays list of connected players. Once everyone is in, they start the game by sending StartGame.
6. The game enters the `selection` state of the first round. The host chooses who picks first with HostSetPicker, then picks the clue that player chooses with HostChoice.
7. A normal clue enters `questionReading`. The host reads the clue, then sends HostReady to open buzzing.
8. The game enters `waitingForBuzz`. Players buzz in by sending Buzz. The fastest reaction time within a short window wins, and the host is told via PlayerBuzzed.
9. The game enters `answer`. The host judges the response with HostChecked.
  a. If correct, the player gains the clue's value, becomes the picker, and the game shows the answer (`answerReveal`).
  b. If incorrect, the player loses the clue's value and can't buzz again on this clue. If anyone else can still buzz, buzzing reopens (`waitingForBuzz`). Otherwise the game shows the answer.
10. A Daily Double skips steps 7–9: the picker wagers on it (`dailyDoubleWager`), then answers it alone (`answer`), winning or losing their wager.
11. From `answerReveal`, HostContinue goes back to `selection`. When a board is cleared, or the host ends the round early with HostNextRound, the game moves to Double Jeopardy, then Final Jeopardy, skipping whichever the game doesn't have.
12. In Final Jeopardy, everyone wagers on the category (`finalWager`), writes a response to the clue (`finalAnswer`), and then the host reveals and judges each response in turn (`finalReveal`).
13. The game ends (`gameEnd`) after Final Jeopardy, after the last round if there is no Final Jeopardy, or whenever the host sends EndGame. The player with the highest score wins.

## Notes

- Players never receive GameState, which holds every clue's answer. They get PublicState (the same for everyone) and their own PlayerState.
- Commands from the wrong role (a player sending a host command or the reverse) are ignored and logged. So are commands that aren't allowed in the current state, so a double-click or a stale command can't skip part of the game.
- Rooms are deleted after 30 minutes without any commands.

# WS Message Protocol

Every message is a JSON object whose `type` field names the message, with its fields alongside, e.g. `{"type": "HostChoice", "categoryIndex": 2, "questionIndex": 3}`. Field names below are as sent over the wire.

## Commands (client -> server)

| Command | Sent by | Allowed in | What it does |
|---|---|---|---|
| `StartGame` | Host | `start` | Starts the first round with clues left, with no picker yet |
| `EndGame` | Host | any state but `gameEnd` | Ends the game and decides the winner |
| `HostSetPicker {pid}` | Host | `selection` | Makes `pid` the picker. Ignored if `pid` isn't in the room. |
| `HostChoice {categoryIndex, questionIndex}` | Host | `selection` | Chooses a clue on the current board. Ignored while there is no picker, or if the clue is missing or already played. |
| `HostReady` | Host | `questionReading` | Opens buzzing. Also sends a Witness (see below). |
| `Buzz {reactionMs}` | Player | `waitingForBuzz` | Buzzes in. `reactionMs` is the time between the buzzer enabling and the press, measured on the phone. Ignored from a player who already buzzed on this clue. |
| `HostChecked {correct}` | Host | `answer`, `finalReveal` | Judges the response being answered, or in Final Jeopardy the response just revealed |
| `HostSkip` | Host | `questionReading`, `waitingForBuzz`, `dailyDoubleWager`, and `answer` on a Daily Double | Skips the clue with no score change |
| `HostContinue` | Host | `answerReveal`, `finalReveal` | Moves on from the answer, or ends the game once every Final Jeopardy response is judged |
| `HostNextRound {round}` | Host | `selection` | Ends round `round` early. Ignored unless `round` is the current round. |
| `SubmitWager {amount}` | Player | `dailyDoubleWager`, `finalWager` | Wagers on the Daily Double or on Final Jeopardy |
| `SubmitAnswer {answer}` | Player | `finalAnswer` | Sends or replaces a Final Jeopardy response |
| `HostRevealFinalClue` | Host | `finalWager` | Closes the wagers and shows everyone the clue |
| `HostLockFinalAnswers` | Host | `finalAnswer` | Closes the responses |
| `HostRevealFinalResponse {pid}` | Host | `finalReveal` | Shows everyone `pid`'s response. Ignored unless `pid` is next and not yet revealed. |
| `Heartbeat {hbid, tDohbRecv}` | Player | any | Answers a DoHeartbeat, see [Latency](#latency) |
| `LatencyOfHeartbeat {hbid, tLat}` | Player | any | Reports a measured latency, see [Latency](#latency) |

## Events (server -> client)

| Event | Sent to | When |
|---|---|---|
| `PlayerList {players}` | Host | When the host connects and whenever a player joins. Each player is `{pid, name, score, buzzed}`. |
| `NewPlayer {pid, token}` | The joining player | Once, when they first join, with their own token. No one is ever sent another player's token. |
| `GameState {...}` | Host | After every change once the game has started, and when the host reconnects. See [GameState](#gamestate-host-only). |
| `PublicState {state, round, picker}` | Every player | Once the game has started: after every change, and when a player joins or reconnects. `round` is zero-based; `picker` is a pid or null. |
| `PlayerState {...}` | One player | After every change, and when they join or reconnect. See [PlayerState](#playerstate). |
| `Rejected {reason}` | One player | When their wager or response was refused, with a reason to show them |
| `PlayerBuzzed {pid, name}` | Host | When the buzz window closes and `pid` wins the buzz |
| `Witness {msg}` | Every player | On HostReady, a copy of the PublicState wrapped in a Witness, delayed by 500 ms minus each player's measured latency. The web client currently ignores it. |
| `DoHeartbeat {hbid, t_sent}` / `GotHeartbeat {hbid}` | One player | See [Latency](#latency) |

A broadcast sends GameState to the host, PublicState to every player, then each player their own PlayerState, so a player's PlayerState arrives after the PublicState it goes with. The only later message is the delayed Witness after HostReady. A Final Jeopardy wager or response only changes what its player and the host see, so it's sent only to them.

Buzzers are enabled by the PlayerState (`canBuzz: true`) that HostReady's broadcast sends straight away, not by the Witness. Buzzing is kept fair by `reactionMs`: the first Buzz opens a 250 ms window, and when it closes the lowest reaction time wins, with ties going to whichever buzz arrived first.

## PlayerState

```jsonc
{
  "type": "PlayerState",
  "pid": 1,
  "buzzed": false,          // has buzzed on this clue
  "score": 400,
  "canBuzz": false,         // in waitingForBuzz and hasn't buzzed yet
  "wagerRange": { "min": 5, "max": 2000 } | null,  // set while this player must wager
  "wager": 1000 | null,     // their wager on the Daily Double or Final Jeopardy
  "prompt": "..." | null,   // what they're wagering or answering on, see below
  "canAnswer": false,       // may write a Final Jeopardy response now
  "answer": "..." | null,   // their own Final Jeopardy response
  "correct": true | null    // how the host judged their own Final Jeopardy response
}
```

The wager fields are only filled in when they concern this player:
- **Daily Double** (`dailyDoubleWager`, `answer`, `answerReveal`): only for the player who chose it. `wagerRange` is set until they wager, `wager` once they have, and `prompt` is the clue's category. Everyone else gets nulls.
- **Final Jeopardy** (`finalWager`, `finalAnswer`, `finalReveal`): for every player in it. `wagerRange` is set in `finalWager` for a player with a score above $0 (see [Wagers](#wagers)). `prompt` is the category in `finalWager` and the clue after that. `canAnswer` is true in `finalAnswer`. `answer` and `correct` are their own response and result. A player who joined after the responses were locked gets nulls.
- **Game over**: if the Final Jeopardy responses were locked, players keep their own Final Jeopardy fields. A game ended earlier shows nothing, since the clue may never have been revealed.

## GameState (host only)

```jsonc
{
  "type": "GameState",
  "state": "selection",
  "categories": [           // the current round's board
    {
      "title": "Category Name",
      "questions": [
        { "question": "...", "answer": "...", "value": 200, "answered": false }
      ]
    }
  ],
  "round": 0,               // zero-based index of the current round
  "roundCount": 2,          // 1, or 2 with Double Jeopardy
  "players": [{ "pid": 1, "name": "Player 1", "score": 0, "buzzed": false }],
  "currentQuestion": [categoryIndex, questionIndex] | null,
  "currentBuzzer": pid | null,   // who is answering
  "picker": pid | null,          // who chooses the next clue; null until the host sets it, and in Final Jeopardy
  "ddWager": 1000 | null,        // the wager on the Daily Double being played, once made
  "hasFinal": true,              // whether the game ends with Final Jeopardy
  "final": {                     // null until Final Jeopardy starts
    "category": "...",
    "question": "...",           // the clue
    "answer": "...",             // the correct response
    "entries": [                 // by pid until the responses are locked, then in reveal order
      { "pid": 1, "wager": 0 | null, "answer": "..." | null, "correct": true | null, "revealed": false }
    ],
    "revealIndex": 0             // index into entries of the next response to reveal or judge; entries.length once all are judged
  },
  "winner": pid | null           // set at game end; null on a tie or if the room has no players
}
```

Daily Double locations are never sent to anyone, host included. The server reads them when the room is created and keeps them to itself.

## Game State Machine
The game logic is a state machine on the server (`game/room.rs`) with the following states and transitions.

**States:**
- `start` - Players are joining
- `selection` - The picker chooses a clue from the board, and the host selects it
- `questionReading` - Host reads the clue aloud
- `dailyDoubleWager` - The picker chose a Daily Double and is deciding how much to wager
- `waitingForBuzz` - Players can buzz in
- `answer` - A player is answering
- `answerReveal` - Everyone sees the correct response
- `finalWager` - Final Jeopardy: everyone sees the category and wagers
- `finalAnswer` - Final Jeopardy: everyone sees the clue and writes a response
- `finalReveal` - Final Jeopardy: the host reveals and judges each response in turn
- `gameEnd` - Game is over (final state)

**Transitions:**
- start → selection (StartGame; the first round with clues left, which is Double Jeopardy if the Jeopardy board is empty)
- start → finalWager (StartGame, no round has clues, and the game has Final Jeopardy)
- start → gameEnd (StartGame, no round has clues, and no Final Jeopardy)
- selection → selection (HostSetPicker)
- selection → questionReading (HostChoice on a normal clue)
- selection → dailyDoubleWager (HostChoice on a Daily Double)
- selection → *next round* (HostNextRound, see below)
- questionReading → waitingForBuzz (HostReady)
- waitingForBuzz → answer (the buzz window closes; the fastest reaction time answers)
- dailyDoubleWager → answer (SubmitWager in range from the picker; the picker answers)
- answer → answerReveal (HostChecked correct)
- answer → waitingForBuzz (HostChecked incorrect on a normal clue, and someone hasn't buzzed yet)
- answer → answerReveal (HostChecked incorrect on a Daily Double, or nobody is left to buzz)
- questionReading, waitingForBuzz, dailyDoubleWager, answer on a Daily Double → answerReveal (HostSkip; the clue is used up)
- answerReveal → selection (HostContinue, clues left on the board)
- answerReveal → *next round* (HostContinue, board cleared)
- *next round* → selection of the next round with clues left; else finalWager if the game has Final Jeopardy; else gameEnd
- finalWager → finalAnswer (HostRevealFinalClue)
- finalAnswer → finalReveal (HostLockFinalAnswers)
- finalReveal → finalReveal (HostRevealFinalResponse, then HostChecked, for each player in reveal order)
- finalReveal → gameEnd (HostContinue once every response is judged)
- any state but gameEnd → gameEnd (EndGame)

## Rules

### Rounds
- Round 0 is Jeopardy, and round 1 is Double Jeopardy if the game has one. A round with no clues left to play is skipped.
- Scoring: a correct response adds the clue's value, or the wager on a Daily Double; an incorrect one subtracts it. Scores can go below $0.

### Picker
- The picker is the player who chooses the next clue. It's shown on the host, the display, and the picker's phone.
- When the game starts, there is no picker, and HostChoice is ignored until the host sends HostSetPicker. This holds even if StartGame skips an empty Jeopardy board and starts in Double Jeopardy. The host can change the picker at any time during `selection`.
- A correct response makes the responder the picker. After an incorrect response, a skip, or nobody answering, the picker stays the same.
- When the game moves on to Double Jeopardy from Jeopardy, the picker is the connected player with the lowest score (ties to the lowest pid), or the lowest scorer of all if nobody is connected. The host can override it.
- A picker who disconnects stays the picker until the host chooses someone else.
- Final Jeopardy clears the picker.

### Wagers
| | Who | Accepted range |
|---|---|---|
| Daily Double | The picker who chose it | $5 up to their score, or up to the most valuable clue on the board if that's more, and never less than $5 |
| Final Jeopardy, score > $0 | Every player in it | $0 up to their score |
| Final Jeopardy, score ≤ $0 | Every player in it | Exactly $1 (the $1 rule) |

- A wager outside its range, or a Daily Double wager from anyone but the picker, gets a Rejected with the reason.
- Only the picker answers a Daily Double: nobody buzzes, and an incorrect response doesn't reopen buzzing. The host can skip it while the picker wagers or answers.
- A player with nothing to wager in Final Jeopardy is entered at $1 automatically, gets no `wagerRange`, and any other amount is rejected. Since their wager is already in, the host sees them as "✓ wagered" from the start.
- Final Jeopardy wagers can be changed until the clue is revealed. Anyone who hasn't wagered by then wagers $0 (or $1 under the $1 rule).
- Final Jeopardy responses are trimmed and cut off at 200 characters, and can be changed until the host locks them. Anyone who hasn't responded by then gets an empty response.

### Final Jeopardy
- Everyone in the room when it starts plays it. Players who join during `finalWager` or `finalAnswer` play too, for $1. Players who join after the responses are locked only watch.
- Reveal order: when the responses are locked, the players are ordered by score, lowest first (as on TV), with ties going to the lowest pid, the player who joined first.
- For each player in that order, the host sends HostRevealFinalResponse (everyone sees the response), then HostChecked (their score moves by their wager). A response must be revealed before it's judged, and a repeated reveal or judgement is ignored, so nobody can be skipped.
- The winner is decided once every response is judged and the host sends HostContinue: the single highest score wins. A tie, or a room with no players, has no winner.

## Privacy
- **Players** never receive GameState. They get PublicState, which is the same for everyone, and a PlayerState holding only their own wager and response. They are never sent a clue's correct response, another player's wager or response, or another player's token.
- **The host** gets the full GameState, including every correct response and every Final Jeopardy wager and response, so they can read and judge. The host UI shows only who has wagered or responded, and hides the Final Jeopardy wagers and responses themselves until the host clicks "peek".
- **The display** is a window the host opens on the projector. The host forwards it `displayGameState(gameState)` (`web/src/lib/finalJeopardy.ts`) rather than GameState itself, which removes the Final Jeopardy clue until it's revealed, the correct response until every response is judged, each response until it's revealed, and each wager until its response is judged. The display shows a board clue's correct response only in `answerReveal`.
- **Daily Doubles** are hidden from everyone, host included, until a picker chooses one.

## Latency

The server can ask a player to measure its latency, which it uses to delay Witness messages:
- `DoHeartbeat {hbid, t_sent}` is sent to each player when something calls `GET /rooms/:code/cpr`.
- The player replies `Heartbeat {hbid, tDohbRecv}` with the time it got the DoHeartbeat, and the server answers `GotHeartbeat {hbid}`.
- The player then reports `LatencyOfHeartbeat {hbid, tLat}`, where `tLat` is the time from receiving the DoHeartbeat to receiving the GotHeartbeat. The server combines it with the DoHeartbeat's timestamps into a latency, and keeps an average of the last few.

# Creating a room

`POST /api/v1/rooms/create` with a `CreateRoomRequest`. Every field is optional, so older clients still work.

```jsonc
{
  "categories": [ { "title": "...", "questions": [ { "question": "...", "answer": "...", "value": 200, "dailyDouble": true } ] } ],  // the Jeopardy board
  "doubleCategories": [ ... ],   // the Double Jeopardy board; omitted or [] = no Double Jeopardy
  "final": { "category": "...", "question": "...", "answer": "..." },  // omitted, null, or a blank question = no Final Jeopardy
  "randomDailyDoubles": false    // hide Daily Doubles at random
}
```

- `answered` and `dailyDouble` default to false.
- With `randomDailyDoubles`, the server hides 1 Daily Double in Jeopardy and 2 in Double Jeopardy, in each round whose board has none of its own. They never go in the top row, and each goes in a different category.
- The response is `201 Created` with `{"room_code": "KXQWMB", "host_token": "<uuid>"}`. Room codes are 6 letters from A–Z without I or O.

The web app builds this from a game file (`web/src/lib/gameFile.ts`), which the lobby loads and the Game Builder writes:

```jsonc
{
  "game": {
    "single": [ { "category": "...", "clues": [ { "value": 200, "clue": "...", "solution": "...", "dailyDouble": true } ] } ],
    "double": [ /* same shape; omitted or [] = no Double Jeopardy */ ],
    "final":  { "category": "...", "clue": "...", "solution": "..." }  // omitted or empty clue = no Final Jeopardy
  }
}
```

Validation (`parseGameFile`):
- Every category needs a non-empty name and at least one clue. Every clue needs non-empty `clue` and `solution` text, and a `value` that is a whole number of dollars, $0 or more. `dailyDouble`, if present, must be `true` or `false`.
- A broken Jeopardy round rejects the whole file.
- A broken Double Jeopardy or Final Jeopardy is skipped with a warning, so files with incomplete optional rounds (common in scraped games) still load. A Final Jeopardy with a clue also needs a category and a solution.
- The Game Builder is stricter: it won't use a game until every round it has is complete.

# API Endpoints
/api/v1
POST /rooms/create				Create room
GET /rooms/:code/ws?token&playerName&playerID 				WebSocket upgrade
GET /rooms/:code/cpr				Ask every player in the room for a heartbeat

Outside /api/v1:
GET /health					Health check
Anything else serves the web app
