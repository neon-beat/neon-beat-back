# Neon Beat back

Neon Beat back is the Rust backend powering Neon Beat, a homemade blind test experience built around playlists, teams, and fast-paced buzzer rounds.

See [CHANGELOG](CHANGELOG.md) for detailed release notes.

## Highlights

- **RESTful API**: Provides a well-defined RESTful API for programmatic access to its functionalities.
- **Real-time communications**: Real-time communications via WebSockets for buzzers and Server-Sent Events for the public and admin UIs.
- **Configurable persistence**: Build with MongoDB or CouchDB support and select the active store per deployment. Keeps playlists, teams, and game progress in sync. Playlists are stored in their own collection so games can reuse curated track lists without re-importing them each time.
- **Swagger UI**: The full OpenAPI document is generated with utoipa and served through Swagger UI (`/docs`) for quick manual testing.

## Architecture Overview

### Module layout
The Neon Beat back project follows a layered architecture, separating concerns into distinct modules:
- **`routes`**: This layer handles incoming HTTP requests and defines the API endpoints. It is responsible for parsing requests, calling the appropriate service methods, and returning HTTP responses.
- **`services`**: This layer contains the business logic of the application. It orchestrates operations, interacts with the `dao` layer to retrieve or store data, and applies any necessary transformations or validations.
- **`dao` (Data Access Object)**: This layer is responsible for interacting with external data sources or systems, such as a MongoDB database. It abstracts the details of data persistence and retrieval from the service layer.
  - **`models`**: This submodule within the `dao` layer defines the data models that represent the entities and structures used when interacting with external systems. These models ensure consistent data representation across the application's interactions with various data sources.
- **`dto` (Data Transfer Object)**: This layer defines the data structures used for transferring data between different layers of the application, particularly between the `routes` and `services` layers, and for external API communication. These structures ensure consistent data formats.
- **`state`**: Centralises runtime state kept in memory while the server runs. It exposes the finite-state machine that coordinates gameplay, the in-memory `GameSession`/playlist data used by services and DTOs, the SSE hubs, and shared resources such as buzzer connections.

### System interactions
```mermaid
flowchart LR
    subgraph Neon Beat Backend
        REST(REST API routes) --> StateMachine(STATE MACHINE)
        WS(WebSocket Connection) <--> StateMachine
        StateMachine --> MongoDbDao(MongoDB DAO)
        StateMachine --> SSE(SSE Connection)
    end

    subgraph Frontends
        PublicFront[Public Frontend] --> REST
        AdminFront[Admin Frontend] --> REST
        SSE --> PublicFront
        SSE --> AdminFront
    end

    subgraph Buzzers
        Buzzer1[Buzzer 1] <--> WS
        Buzzer2[Buzzer 2] <--> WS
    end

    MongoDbDao --> MongoDbInstance[MongoDB Instance]
```

### Game state flow
```mermaid
stateDiagram-v2
    [*] --> idle

    note right of [*]
        GM: Game Master
    end note
    note left of idle
        Playlist and players management. Visible in admin front
    end note

    idle --> game_running: start_game
    state game_running {
        prep --> playing_game: game_configured
        playing_game --> pause: GM_triggers_pause
        playing_game --> pause: buzz
        pause --> reveal: game_master_triggers_reveal
        pause --> playing_game: game_master_triggers_continue
        reveal --> playing_game: GM_triggers_next
        playing_game --> reveal: GM_triggers_reveal
    }
    game_running --> show_scores: playlist_end_or_GM_stops
    show_scores --> idle: GM_ends_game
```

## Core gameplay features

- **Playlist import & persistence**: JSON playlists where each song has some basic properties and are persisted atomically:
   - Timestamp at which the song should start
   - Time to identify a song
   - Song URL
   - "Point fields" are fields to find for the song, that can give points to a team: for example, song name and artist (this list of field is dynamic and not empty)
   - "Bonus point fields" are optional fields to find for the song, that can give bonus points to a team (this list of field is dynamic and may be empty)
   - During game bootstrap the playlist song order is shuffled once to create a random play sequence; persisted games must provide the same identifiers to guarantee consistency.
- **Game bootstrap**: Game can be created or loaded (from database) during the idle state:
   - the game contains a list of players (players have a unique buzzer, a name and a score)
   - the game references a persisted playlist entity (shared across games) which is embedded into the runtime session when the game starts [**WARNING**: the game considers currently that the playlist doesn't change !]
   - the game contains a game state (frequently saved in database), which contains a playlist state (the playlist state remembers whether a song has been played or not) and must match the playlist identifiers exactly
- **State machine execution**: Gameplay transitions follow the diagram above (`Game state flow`), persisting progress and orchestrating pauses, reveals, and scoring.
- **Admin controls (REST)**:
   - create/load games return a `GameSummary` payload bundling players, shuffled playlist ordering, and timestamps
   - pause the current song
   - resume the current song
   - add/remove points to a team
   - update team metadata (buzzer id, name, score)
   - reveal the current song
   - mark a field as "found"
   - validate/invalidate an answer
- **Prep-phase team pairing**:
   - allow creating/updating/deleting teams while the state machine is `GameRunning::Prep`
   - enforce that buzzers are paired (or explicitly in pairing mode) before transitioning to `Playing`
   - expose admin endpoints to enter/abort pairing mode, snapshot teams, and reassign buzzers with SSE notifications
   - support rollback of pairing operations to restore the last known good snapshot on failure
- **Public API (REST)**:
   - get teams infos
   - get current song infos
   - get current game phase (with degraded flag)
- **WebSocket connection for buzzers**
   - Buzzers connect to `GET /ws` and identify themselves by sending:
     ```json
     { "type": "identification", "id": "<mac_address>" }
     ```
     where `<mac_address>` is a 12-character lowercase hex string.
   - Buzz events reuse the same id:
     ```json
     { "type": "buzz", "id": "<mac_address>" }
     ```
   - Buzzers need to be informed when:
      - Game is paused and it's the team's turn to answer
      - Team's turn is finished and the game resumes
- **SSE connection for frontends**: Admin and public frontends subscribe via `/sse/admin` and `/sse/public`. The admin stream issues a one-time token and enforces a single active admin connection.

## Pairing workflow

The buzzer pairing workflow lives inside the finite state machine so that API calls, SSE notifications, and WebSocket feedback stay in lock-step. A typical session looks like this:

1. **Kick off pairing**
   ```bash
   curl -X POST http://localhost:8080/admin/teams/pairing \
     -H 'content-type: application/json' \
     -d '{ "first_team_id": "<uuid-of-team-to-start-with>" }'
   ```
   - The game enters `GameRunning::Prep(Pairing)` and snapshots the roster.
   - Public and admin SSE streams broadcast `pairing.waiting` with the team that must claim a buzzer next.

2. **Assign a buzzer**
   - The highlighted team presses its buzzer (or an operator can simulate it).  
   - The WebSocket client sends `{ "type": "buzz", "id": "<12-char buzzer id>" }`.
   - The backend:
     - Assigns the buzzer while clearing any conflicting assignment.
     - Replies to the device with a `BuzzFeedback` payload (`{"id":"<buzzer id>","can_answer":true}`) so hardware can give immediate confirmation.
     - Emits `pairing.assigned` containing the team UUID and the new buzzer ID.  
     - Emits another `pairing.waiting` if more unpaired teams remain; otherwise it transitions back to `prep_ready`.

3. **Handle deletions mid-pairing**
   - `DELETE /admin/teams/{team_id}` now emits a lightweight `team.deleted` event on the public SSE stream.  
   - If the removed team was the one currently pairing, the server automatically advances to the next unpaired team (broadcast through `pairing.waiting`) or ends pairing if everyone is assigned.

4. **Abort pairing**
   ```bash
   curl -X POST http://localhost:8080/admin/teams/pairing/abort
   ```
   - Restores the snapshot captured when pairing began.
   - Emits `pairing.restored` with the full roster before returning to `prep_ready`.
   - Returns the restored roster as an array of `TeamSummary` objects so UIs can resynchronise without waiting for SSE.

Public clients can still poll `/public/pairing-status`, but reacting to the SSE stream keeps both admin and public UIs in sync without reloading the complete roster.

## Realtime interfaces

### WebSocket `/ws` (buzzers)

Buzzers maintain a single long-lived WebSocket connection. Each device **must** identify itself before sending buzz events.

| Direction | Message type | Payload example | Notes |
|-----------|--------------|-----------------|-------|
| client → server | `{"type":"identification","id":"deadbeef0001"}` | 12 lowercase hex characters | Required immediately after connecting. |
| server → client | `{"id":"deadbeef0001","status":"ready"}` (`BuzzerAck`) | – | Sent when identification succeeds. |
| client → server | `{"type":"buzz","id":"deadbeef0001"}` | must reuse the identification id | Ignored unless the game is in `prep_ready`, `prep_pairing`, or `playing`. |
| server → client | `{"id":"deadbeef0001","can_answer":true}` (`BuzzFeedback`) | `can_answer` becomes `true` during pairing when the team was expected, and during gameplay when the buzz grants the floor. |
| server → client | `{"id":"deadbeef0001","can_answer":false}` (`BuzzFeedback`) | – | Returned when the buzz was rejected (wrong phase, duplicate during pairing, etc.). |
| server → client | WebSocket close frame | – | Connection closed by the backend (e.g. admin kicked, duplicate connection); client should retry with exponential backoff. |

Messages tagged with any other `type` are ignored.

### Server-Sent Events

Two SSE streams are available:

- `GET /sse/public` – no authentication, receives public updates.
- `GET /sse/admin` – requires a single active client; the first event contains an admin token that must be echoed by the frontend on subsequent REST calls.

Every connection begins with a `handshake` event:

```json
event: handshake
data: {"stream":"public","message":"public stream connected","degraded":false}
```

Admin streams include an extra `token` field in the same payload. When the storage backend drops out of availability the server emits `system_status` events:

```json
event: system_status
data: {"degraded":true}
```

The remaining events represent gameplay changes. Payload types are defined in `src/dto/sse.rs`.

| Event name | Payload | Stream(s) | Description |
|------------|---------|-----------|-------------|
| `fields_found` | `FieldsFoundEvent` | public | Updated list of discovered point/bonus fields for the current song. |
| `answer_validation` | `AnswerValidationEvent` | public | Indicates whether the latest answer was accepted. |
| `score_adjustment` | `TeamSummary` | public | Broadcast after manual score changes. |
| `phase_changed` | `PhaseChangedEvent` | public + admin | FSM transition (optionally includes song snapshot, scoreboard, and paused buzzer id). |
| `team.created` | `TeamCreatedEvent` | public + admin | Newly created team (payload wraps a `TeamSummary`). |
| `team.updated` | `TeamUpdatedEvent` | public | Existing team metadata changed (name, buzzer, or score). |
| `team.deleted` | `TeamDeletedEvent` | public | Team removed; payload only contains the team UUID. |
| `game.session` | `GameSummary` | public | Full game snapshot (players, playlist ordering, timestamps). |
| `pairing.waiting` | `PairingWaitingEvent` | public + admin | Announces which team should pair a buzzer next. |
| `pairing.assigned` | `PairingAssignedEvent` | public + admin | Confirms a buzzer assignment during pairing. |
| `pairing.restored` | `PairingRestoredEvent` | public | Snapshot broadcast after aborting pairing. |
| `test.buzz` | `TestBuzzEvent` | public + admin | Emitted when a prep-mode test buzz is detected. |

Keep-alive comments are sent every 15 seconds so most SSE clients will stay connected by default.

## Getting started

### Prerequisites
- Rust toolchain (1.85+ recommended)
- MongoDB instance reachable at `MONGO_URI`

### Build from source
```bash
cargo build
```
Binaries live under `target/debug/neon-beat-back` or `target/release/neon-beat-back`.

### Environment

The server relies on a running MongoDB instance and the following optional environment variables:

| Variable     | Default                     | Description |
|--------------|-----------------------------|-------------|
| `MONGO_URI`  | `mongodb://localhost:27017` | Connection string used to create the MongoDB client. |
| `MONGO_DB`   | `neon_beat`                 | Database name (only used when provided). |
| `COUCH_BASE_URL` | – | Base URL for the CouchDB server (e.g. `http://localhost:5984`). |
| `COUCH_DB`   | – | Database name created/used by the Couch backend. |
| `COUCH_USERNAME` /<br>`COUCH_PASSWORD` | – | Optional basic-auth credentials for CouchDB. |
| `NEON_STORE` | – | Required when both backends are compiled; set to `mongo` or `couch` to choose the store at runtime. If only one backend was compiled, the value is optional but must match when supplied. |
| `PORT`       | `8080`                      | TCP port the HTTP server binds to. `SERVER_PORT` is also honoured for compatibility. |

### Selecting a storage backend

The backend ships with both storage implementations enabled by default. At startup:

- If both features were compiled, set `NEON_STORE=mongo` or `NEON_STORE=couch` to choose the active store.
- If only one feature was compiled (for a slimmer binary), `NEON_STORE` is optional, but any value you supply must still match the compiled backend.

To build the binary with a single backend you can rely on Cargo features:

```bash
# Mongo-only build
cargo build --release --no-default-features --features mongo-store

# Couch-only build
cargo build --release --no-default-features --features couch-store
```

### Run locally
1. Decide which store to use and start the matching database:
   - **MongoDB**: `export NEON_STORE=mongo` (when both backends are compiled) and ensure `MONGO_URI`/`MONGO_DB` point to a running instance.
   - **CouchDB**: `export NEON_STORE=couch` (when both backends are compiled) and set `COUCH_BASE_URL`/`COUCH_DB` plus credentials if required.
   - If the binary was built with only one backend, `NEON_STORE` can be omitted; it defaults to the compiled store.
2. Start the backend:
   ```bash
   cargo run
   ```
3. Try a few entry points:
   - Healthcheck: `curl http://localhost:8080/healthcheck`
   - Public SSE stream: `curl -N http://localhost:8080/sse/public`
   - Admin SSE stream (one active client): `curl -N http://localhost:8080/sse/admin`
   - WebSocket buzzer (after `cargo install websocat`):
     ```bash
     ( printf '{"type":"identification","id":"deadbeef0001"}\n'; cat ) | websocat -t ws://localhost:8080/ws
     ```
   - Swagger UI: open `http://localhost:8080/docs`

### Docker

Build the release image with Docker:

```bash
docker build -t neon-beat-back .
```

To ship an image with a single backend, pass the feature flags through the build argument `CARGO_FEATURES` (the value is appended to each Cargo invocation):

```bash
# Mongo-only image
docker build -t neon-beat-back \
  --build-arg CARGO_FEATURES="--no-default-features --features mongo-store" .

# Couch-only image
docker build -t neon-beat-back \
  --build-arg CARGO_FEATURES="--no-default-features --features couch-store" .
```

For cross-compilation, provide a Rust target triple via the optional build argument:

```bash
docker build -t neon-beat-back --build-arg BUILD_TARGET=aarch64-unknown-linux-gnu .
```

When using docker-compose (e.g. after copying `docker-compose.example.yaml` as `docker-compose.yaml`), the same argument can be passed through an environment variable:

```bash
BUILD_TARGET=aarch64-unknown-linux-gnu docker compose build
```

## Roadmap

- [x] Implement the Rest API with a /healthcheck route
- [x] Implement the WebSocket connection
- [x] Implement the SSE connection
- [x] Implement the MongoDB connection bootstrap
- [x] Implement the Swagger UI documentation
- [x] MongoDB auto-reconnection
- [x] Admin SSE deconnexion management
- [x] Do not block start of app if connexion to MongoDB fails (Authorize "degraded mode" → info to send by SSE)
- [x] Implement Game State Machine
- [x] Implement Game & Playlist State save in DB (only found songs, not every answered field)
- [x] Save in memory (not in DB) found point fields and bonus point fields for the current song
- [x] Implement buzzer feedback: apply GameEvent::Pause(PauseKind::Buzz) ; return true if it's the team's turn to answer, else return false
- [x] Implement SSE public events:
   - [x] game created/loaded: send teams
   - [x] point field / bonus point field found: send the list of point field / bonus point field (name only) found
   - [x] validate/invalidate answer: send true or false
   - [x] add/remove points for a team: send the points to add (or remove) to a team (player id)
   - [x] game phase changed to new phase: playing (with next song or not), pause, reveal, scores (list of teams with their scores), idle
- [x] Implement SSE admin events:
   - [x] game phase changed to new phase: playing (with next song or not), pause, reveal, scores (list of teams with their scores), idle
- [x] Implement admin routes:
   - [x] get games: OUTPUT is games IDs and names
   - [x] get playlists: OUTPUT is playlists IDs and names
   - [x] create playlist: INPUT is PlaylistInput ; save the playlist in DB ; OUTPUT is the PlaylistSummary
   - [x] load game: INPUT is the game ID ; OUTPUT is the GameSummary and PlaylistSummary ; apply GameEvent::StartGame
   - [x] create game with new playlist: INPUT is CreateGameWithPlaylistRequest ; OUTPUT is the GameSummary and PlaylistSummary ; apply GameEvent::StartGame
   - [x] create game with existing playlist ID: INPUT is CreateGameRequest ; OUTPUT is the GameSummary and PlaylistSummary ; apply GameEvent::StartGame
   - [x] start game: OUTPUT is song to be found ; apply GameEvent::GameConfigured
   - [x] pause: OUTPUT is "paused" message ; apply GameEvent::Pause(PauseKind::Manual)
   - [x] mark field as found: OUTPUT is the list of found fields ; only possible in GamePhase::GameRunning and if GameRunningPhase is not GameRunningPhase::Prep(_)
   - [x] validate/invalidate answer: OUTPUT is "answered" message ; only possible in GamePhase::GameRunning(GameRunningPhase::Paused)
   - [x] add/remove points for a team: OUTPUT is the new score of the team ; only possible in GamePhase::GameRunning
   - [x] resume: OUTPUT is "resumed" message ; apply GameEvent::ContinuePlaying
   - [x] reveal: OUTPUT is "revealed" message ; apply GameEvent::Reveal
   - [x] next: OUTPUT is the next song to be found ; if playlist is not completed, apply GameEvent::NextSong, increment GameSession's current_song_index and reset the found fields for the current song ; else apply GameEvent::Finish(FinishReason::PlaylistCompleted)
   - [x] stop: OUTPUT is the list of teams with their scores ; apply GameEvent::Finish(FinishReason::ManualStop)
   - [x] end game: OUTPUT is "ended" message ; apply GameEvent::EndGame
- [x] Use Game State Machine
- [x] Document the new admin endpoints (OpenAPI/utoipa)
- [x] Raise an error if the playlist is empty during GameSession/Playlist creation/loading
- [x] Add song ID to MarkFieldRequest
- [x] Implement a transaction system for state machine (prepare, to know if it is possible, then apply the waiting transaction when we have finished the processing)
- [x] Migrate from MongoDB to CouchDB
- [x] Support multiple DB and choose the one at buildtime or runtime
- [x] Team management & team/buzzer pairing
- [x] Implement public routes:
   - [x] get teams/players
   - [x] get song to find (& found fields)
   - [x] get game phase
- [ ] Add more logs
- [ ] Implement buzzer testing during GamePhase::GameRunning(GameRunningPhase::Prep(_)) (test buzz)
- [ ] Debounce device buzzes (~250 ms) during pairing to avoid double assigns
- [ ] Reorganize routes if required
- [ ] Add middleware for admin routes (check token)
- [ ] Better management for errors
- [ ] Validate the Rest API /healthcheck route
- [ ] Validate the WebSocket connection
- [ ] Validate the SSE connection
- [ ] Validate the MongoDB connection
- [ ] Send encountered errors to admin SSE during WS handles
- [ ] Remove unecessary pub(crate) functions
- [ ] Rename Player to Team (or find a new name)
- [ ] Replace Vec<Teams> by HashMap if it is better
- [ ] Create game/playlist IDs from store
- [ ] Review PlayerInput: is buzzer_id really needed ?
- [ ] Migrate from DashMap to HashMap if DashMap is useless
- [ ] Allow to create a game in degraded mode (save the session & playlist later)
- [ ] Better management for panics & expects
- [ ] When a buzzer has the right to answer, send info to others that they don't have the right to buzz yet. When the buzzer ended its turn, send info to others that they have the right to buzz now.
- [ ] Update `game_store` value of `AppState ` and send False to `degraded` watcher each time a mongo function returns a connection error ?
- [ ] Remove useless features of dependencies if found
- [ ] Implement tests

## Questions

- Do we want to manage public SSE deconnexion ? No
- Do we want to modify a playlist when it is already imported in the backend ? No, we import again
- Do we want to add a timeout when a player has buzzed (to resume the game) ? Add an int config property (default: Infinite)
- Do we want to prevent the previous buzzer to buzz again ? Add a bool config property (default: re-buzz authorized)
- Do we want to serve the OpenAPI documentation as a Github Page ?
- Do we want Game and Playlist name unicity ?
