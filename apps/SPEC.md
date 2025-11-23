This is the backend for a Jeopardy webgame with Jackbox-like player connections. The server manages game state, player connections, and real-time updates via WebSockets. The frontend provides an interactive UI for players to join games, select answers, and view scores.

# Flow

1. Host creates a room via POST /api/v1/rooms/create. They recieve a room code & token.
  a. The body of this POST request includes a game configuration object containing the categories and questions as JSON.
2. Host connects to WebSocket at /api/v1/rooms/:code/ws?token={host_token} to become the host.
  a. If the host gets disconnected, they can reconnect using the same token.
3. Players join the room by connecting to /api/v1/rooms/:code/ws?playerName={name}
4. Upon connection, server sends PlayerList to host and NewPlayer to the joining player.
  a. Server assigns each player a unique pid and token, which the player must store for reconnection.
  b. If a player disconnects, they can reconnect using `/rooms/:code/ws?playerID={pid}&token={token}`
5. Host displays list of connected players. Once everyone is in, they start the game by sending StartGame! message.
6. The game enters the "selection" state. Host displays a grid of questions and selects one by sending HostChoice:{categoryIndex, questionIndex}.
7. The game enters the "questionReading" state. Host reads the question, then sends HostReady! to open buzzing.
8. The game enters the "waitingForBuzz" state. Players can buzz in by sending Buzz! message. The server records the first buzz and notifies the host via Buzzed:{pid, name}.
9. The game enters the "answer" state. The host indicates whether the answer was correct by sending HostChecked:{correct:true/false}
  a. If correct:true, the server updates the player's score and returns to "selection" state (or "gameEnd" if no questions remain).
  b. If correct:false, the player is excluded from buzzing. If other players can still buzz, returns to "waitingForBuzz". Otherwise, returns to "selection" (or "gameEnd" if no questions remain).
10. The game continues until all questions are answered or the host ends the game with EndGame!

## Notes

- the client literally only cares about the playerlist notifications (to see their own score) and the buzz enable/disable messages. they only send buzz messages. the heartbeat messages will come later.

# WS Message Protocol
Witness:{pid}:{msg}					Server -> All
NewPlayer:{player as {pid}:{token}}		Server -> Player
PlayerList:{list as [{pid}:{name}]}		Server -> Host
GameState:{state as JSON}			Server -> Host (sent after every state transition)
StartGame!						Host   -> Server
EndGame!						Host   -> Server
HostChoice:{categoryIndex, questionIndex}	Host   -> Server (select a question)
HostReady!						Host   -> Server (done reading, open buzzing)
Buzz!							Player -> Server
HostChecked:{boolean correct}			Host   -> Server
Buzzed:{pid, name}				Server -> Host (notifies who buzzed)

## GameState Schema
```json
{
  "state": "selection" | "questionReading" | "waitingForBuzz" | "answer" | "gameEnd",
  "categories": [
    {
      "title": "Category Name",
      "questions": [
        { "question": "...", "answer": "...", "value": 100, "answered": false }
      ]
    }
  ],
  "players": [{ "pid": 1, "name": "Player 1", "score": 0 }],
  "currentQuestion": [categoryIndex, questionIndex] | null,
  "currentBuzzer": pid | null
}
```

## Game State Machine
The game logic is controlled by an XState state machine with the following states and transitions:

**States:**
- `selection` - Host selects a question from the grid
- `questionReading` - Host reads the question aloud
- `waitingForBuzz` - Players can buzz in
- `answer` - A player is answering
- `gameEnd` - Game is over (final state)

**Transitions:**
- selection → questionReading (HOST_CHOICE: host selects a question)
- questionReading → waitingForBuzz (HOST_READY: host done reading)
- waitingForBuzz → answer (PLAYER_BUZZ: player buzzes in)
- answer → selection (HOST_CORRECT: correct answer, questions remain)
- answer → gameEnd (HOST_CORRECT: correct answer, no questions remain)
- answer → waitingForBuzz (HOST_INCORRECT: wrong answer, other players can buzz)
- answer → selection (HOST_INCORRECT: all players buzzed incorrectly, questions remain)
- answer → gameEnd (HOST_INCORRECT: all players buzzed incorrectly, no questions remain)
// ! = everyone receives message as Witness:{pid}:{msg}
// *id = integer
// t_* = also an integer, a unix timestamp or timestamp delta
// t_lat is calculated as:
//   t_send_lat = time_recv(DoHeartBeat) - DoHeartbeat.t_sent
//   t_tot_lat = DoHeartbeat.t_sent - time_recv(GotHeartbeat)
//   t_lat (sent) = t_tot_lat - t_send_lat 

Frontend is sending these as JSON, so Join!:{pid}:{name} becomes
{
	“Join”: {
		pid: number,
		name: string,
}
}
// The exclamation point is just for documentation purposes
// and is not actually sent over websockets or in variant names

# API Endpoints
/api/v1
POST /rooms/create				Create room
GET /rooms/:code/ws?token&playerName&playerID 				WebSocket upgrade
