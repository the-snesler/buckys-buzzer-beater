This is the backend for a Jeopardy webgame with Jackbox-like player connections. The server manages game state, player connections, and real-time updates via WebSockets. The frontend provides an interactive UI for players to join games, select answers, and view scores.

# Flow

1. Host creates a room via POST /api/v1/rooms/create. They recieve a room code & token.
  a. TODO: in the body of this POST request, the host will include a game configuration object containing the question set as JSON.
2. Host connects to WebSocket at /api/v1/rooms/:code/ws?token={host_token} to become the host.
  a. If the host gets disconnected, they can reconnect using the same token.
3. Players join the room by connecting to /api/v1/rooms/:code/ws?playerName={name}
4. Upon connection, server sends PlayerList to host and NewPlayer to the joining player.
  a. Server assigns each player a unique pid and token, which the player must store for reconnection.
  b. If a player disconnects, they can reconnect using `/rooms/:code/ws?playerID={pid}&token={token}`
5. Host displays list of connected players. Once everyone is in, they start the game by sending StartGame! message.
6. The host displays a grid of questions. The host uses their interface to select a question
7. The host displays the question. Once they have read it, they send BuzzEnable! to allow players to buzz in.
8. Players can buzz in by sending Buzz! message. The server records the first buzz and notifies the host via Witness message.
  a. After recieving the first Buzz!, the server sends BuzzDisable! to prevent further buzzing.
9. The host indicates whether the answer was correct by sending HostChecked:{correct:true/false}
  a. If correct:true, the server updates the player's score and notifies all players via Witness message.
  b. If correct:false, the server allows buzzing to be re-enabled by sending BuzzEnable! again. The player(s) that answered incorrectly do not receive further notifications for that question.
10. The game continues until all questions are answered or the host ends the game with EndGame!

## Notes

- the client literally only cares about the playerlist notifications (to see their own score) and the buzz enable/disable messages. they only send buzz messages. the heartbeat messages will come later.

# WS Message Protocol
Witness:{pid}:{msg}					Server -> All
NewPlayer:{player as {pid}:{token}}		Server -> Player
PlayerList	:{list as [{pid}:{name}]}		Server -> Host
StartGame!							Host   -> Server
EndGame!							Host   -> Server
BuzzEnable!						Host   -> Server
BuzzDisable!						Host   -> Server
Buzz!								Player -> Server
HostChecked:{boolean correct}			Host   -> Server
DoHeartbeat:{hbid}:{t_sent}			Server -> Player
Heartbeat:{hbid}					Player -> Server
GotHeartbeat:{hbid}					Server -> Player
LatencyOfHeartbeat:{hbid}:{t_lat}		Player -> Server
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
