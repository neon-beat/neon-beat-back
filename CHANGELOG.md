# Changelog

All notable changes to this project will be documented in this file.

## [v0.5.4] - Add an outer helper tool to generate colors

- Add an outer helper tool to generate colors

## [v0.5.3] - Bugfix when an unpaired buzzer buzzes in PrepReady & save song finished info

- Fix the bug when an unpaired buzzer buzzes in PrepReady maked the game stucked
- Rubustify GameSession concurent access
- Send the team who buzzed in the GET phase route and the SSE event
- Save the information that a song has been found (to be able to switch to next song if the game restarts) => Needs to clear the database to use this version !

## [v0.5.2] - Keep playlist song order

- Keep playlist song order (from JSON) if no shuffle => Needs to clear the database to use this version !
- Log a warning if a connected buzzer is not paired while launching the game
- Implement TryFrom instead of From to convert (GameListItemEntity, PlaylistEntity) into GameListItem
- Remove unecessary pub(crate) functions
- Replace Vec<Team> by an IndexMap<Team> in GameSession

## [v0.5.1] - Add optional shuffle query parameter for POST /admin/game/start

- `POST /admin/game/start` accepts an optional `shuffle` query parameter to reshuffle the playlist when it hasn't started yet or after completion.

## [v0.5.0] - Change the answer validation from a boolean to a tri-state (correct, incomplete or wrong)

- Change POST /admin/game/answer request body's valid field from a boolean to a tri-state (correct, incomplete or wrong)
- Change SSE answer_validation data's valid field from a boolean to a tri-state (correct, incomplete or wrong)

## [v0.4.0] - Change POST /admin/game/score into /admin/teams/{id}/score

- Change POST /admin/game/score into /admin/teams/{id}/score, remove buzzer_id field from request body, and change buzzer_id field of response body into team_id

## [v0.3.2] - Add DELETE /admin/games/:id route

- Added `DELETE /admin/games/{id}` to remove stored games (fails if the game is currently running).

## [v0.3.1] - Don't modify the game when it is manually stopped & Allow New Game + sessions

- Don't modify the game when it is manually stopped (bugfix)
- Allow New Game + sessions for playlist completed games : after a game with a completed playlist is loaded, starting it will treat the game as a fresh session (and stopping it will show the scores as usual)

## [v0.3.0] - Add authentication for admin routes

- All `/admin/**` routes now require the `X-Admin-Token` header. The value is issued via the admin SSE handshake (`/sse/admin`).

## [v0.2.1] - Set default tower_http (and every other module) log verbosity level to info

- Set default tower_http (and every other module) log verbosity level to info

## [v0.2.0] - Harmonize naming between teams and players (team chosen)

- Replace player/players occurences by team/teams:
   - GET /admin/games route: teams field replaces players attribute in response body items
   - POST /admin/games & POST /admin/games/with-playlist routes: teams field replaces players attribute in request and response bodies

## [v0.1.5] - Add GET /admin/games/:id route and add game_id to GET /public/phase route response

- Add GET /admin/games/:id route
- Add game_id to GET /public/phase route response

## [v0.1.4] - Add more fields to the GET /admin/games response

- Add players (names and ids), playlist (name and id), created_at and updated_at to the GET /admin/games response

## [v0.1.3] - Fix game creation (without players or with players with no buzzer ID)

- PlayerInput: `buzzer_id` is now optional (changed to `Option<String>`).
- Game creation and startup validation tightened:
	- `create_game` will accept empty player lists and build an empty player vector.
	- `start_game` now returns an error when attempting to start a game with zero players.

## [v0.1.2] - Add team/buzzer pairing and fix GET /admin/playlists

### Interface changes

#### REST
- Added admin team management endpoints: `POST /admin/teams` to create teams, `PUT /admin/teams/{id}` to update them, and `DELETE /admin/teams/{id}` to remove them.
- Added pairing workflow endpoints: `POST /admin/teams/pairing` to start pairing and `POST /admin/teams/pairing/abort` to abort pairing. The abort endpoint now returns the restored roster (`Vec<TeamSummary>`).
- Game bootstrap endpoints (`POST /admin/games`, `POST /admin/games/with-playlist`, `POST /admin/games/{id}/load`) now trigger a `game.session` SSE snapshot after completion.

#### SSE
- Introduced `team.updated` and `team.deleted` events on the public stream so UIs can track roster mutations without refetching.
- Added `game.session` (public-only) to broadcast a full game snapshot whenever a game is created or loaded.
- Pairing events (`pairing.waiting`, `pairing.assigned`, `pairing.restored`) are now emitted on both public and admin streams.

#### WebSocket
- Buzzers continue to exchange `identification`, `buzz`, and `BuzzFeedback` messages; the documentation now specifies the expected acknowledgement flow and reconnection behaviour.

### Other changes
- Fixed CouchDB playlist deserialisation so playlists created via the REST API can be listed without errors.
- Updated README realtime documentation to match the new SSE and WebSocket payloads.

- State-machine driven pairing – entering pairing mode (`POST /admin/teams/pairing`) now transitions the game FSM, guaranteeing that pairing actions only occur during prep. Aborting via `POST /admin/teams/pairing/abort` restores the saved snapshot automatically.
- Incremental pairing updates – buzzer assignments emit `pairing.assigned` while the next team in the queue is announced through `pairing.waiting`. When the final team is paired the state machine exits pairing without additional API calls.
- Targeted roster updates – removing a team with `DELETE /admin/teams/{id}` now broadcasts the compact `team.deleted` SSE payload (team UUID only). Clients should remove the team locally instead of waiting for a full roster refresh.
- Buzzer feedback loop – WebSocket buzzers receive an explicit `BuzzFeedback` acknowledgement after each pairing buzz so devices can signal success or rejection immediately.
- Shared pairing events – pairing events (`pairing.waiting`, `pairing.assigned`, `pairing.restored`) are now published on both admin and public SSE channels, ensuring every UI stays synchronised.

See the [Pairing workflow (v0.1.2+)](README.md#pairing-workflow-v012) section in the README for end-to-end examples and payload formats.

## [v0.1.1] - Initial release

- First public release of the Neon Beat backend, shipping the REST API, WebSocket buzzers, SSE streams, MongoDB/CouchDB stores, and the gameplay state machine.
