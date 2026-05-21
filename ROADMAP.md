# Roadmap

This document outlines planned features, improvements, and known issues for Neon Beat backend.

## Core Features

- [x] Better management for panics & expects
- [ ] Less info logs (only connected/disconnected)
- [ ] Add more debug logs
- [ ] Add the number of played questions and the teams scores to GameListItem (https://github.com/neon-beat/neon-beat-admin-front/issues/13)
- [ ] Refactor TeamSummary (duplicate struct)
- [ ] Improve error codes
   - [ ] If there is no game: do not send 404 for GET Teams
- [ ] Debounce device buzzes (~250 ms) during pairing to avoid double assigns
- [ ] Reorganize routes if required
- [ ] Better management for errors
- [ ] Send encountered errors to admin SSE during WS handles
- [ ] Create game/questions-sequence IDs from store
- [ ] Allow to create a game in degraded mode (save the session & questions sequence later)
- [ ] Update `game_store` value of `AppState ` and send False to `degraded` watcher each time a GameStore function returns a connection error ?
- [ ] Remove useless features of dependencies if found
- [ ] Implement tests

## Gameplay Features

- [ ] Be able to reveal during Pause phase
- [ ] Add another Pause phase between Reveal and Playing (BetweenRevealAndPlaying)
- [ ] Be able to switch to Pairing phase from PrepReady, PauseManual et BetweenRevealAndPlaying : once done, go back to the previous state
- [ ] Add an Error phase that can be triggered from any phase (once done, go back to the previous state)
- [ ] On buzzer pairing, send it the pending pattern for the old team's buzzer ID (if it had one)
- [ ] SSE public GameSession & NextQuestion: remove answer responses
- [ ] Mark answer found: send question hidden details to public SSE
- [x] New route: POST question hint
- [ ] Once a team answered, it can be locked (until another team buzzes or the next question), depending on a game_start boolean parameter
- [ ] If a buzzer enters inhibited mode, send the information to SSE streams (public & admin)

## Bugfixes

- [ ] ? An admin SSE WiFi deconnexion seems to lock the backend ?
