# Bucky's Buzzer Beater

A real-time multiplayer quiz game where players buzz in to answer questions. It's like Jeopardy, but Kahoot!

How it works:

- A host starts the game and selects questions from a grid of categories.
- Players join the game and wait for the host to read questions aloud.
- Players buzz in to answer questions, and the host verifies if the answers are correct.

Fun Features:

- Real-time buzzing system that accounts for latency.
- Custom question sets and categories, made in the built-in Game Builder or loaded from a JSON game file (the format is described in [apps/SPEC.md](apps/SPEC.md#creating-a-room)).
- Double Jeopardy, Daily Doubles and Final Jeopardy, with wagers and written responses from players' phones.
- Support for an arbitrary number of players.
- Separate "projection" window to display the game board without spoiling answers.

Future Plans:
- [x] Create a testing suite for the backend
- [x] Implement point betting & write-in submissions (Final Jeopardy, Daily Double)
- [x] Add support for a second board (Double Jeopardy)
