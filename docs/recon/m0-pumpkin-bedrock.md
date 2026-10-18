# Milestone 0 recon: how Pumpkin talks to Bedrock clients

Pumpkin fork `LaDoodleInTheHat/Pumpkin`, branch `limonite`, commit `aac0a82` (upstream `Pumpkin-MC/Pumpkin`, 2026-10-18).
All paths below are relative to `pumpkin/`.

## Summary

- Upstream Bedrock support **no longer uses RakNet**. It speaks **NetherNet**, Mojang's WebRTC
  transport, and targets **Bedrock 1.26.51, protocol 2193**, accepting only that exact protocol.
- Every Bedrock connection reaches the game through **one field**: `BedrockClient::session`. It
  uses **six methods** on it. That is the `Transport` seam.
- Packets are turned into bytes, and **compressed**, in game code, *before* they reach the
  transport. So a `LocalChannel` carries bytes, not structs. It can still skip compression without
  moving any code (see section 5).
- Offline-mode login still needs a well-formed but unsigned token with a `cpk` public key, plus a
  `ClientData` token with 10 required fields (see section 6).
- Pumpkin sends no light data, so the client must compute lighting.

## 1. Transport: NetherNet, not RakNet

NetherNet is the transport Bedrock uses for LAN and friend games. Each connection is a WebRTC
peer connection, which bundles ICE for NAT traversal, DTLS for encryption and SCTP for framing,
and carries two data channels: `ReliableDataChannel` (ordered) and `UnreliableDataChannel`.

| Socket | Default | What it does | Code |
| --- | --- | --- | --- |
| UDP | `0.0.0.0:19132` | Server-list status pings **and** ICE, told apart per datagram | `crates/pumpkin-core/src/net/bedrock/status.rs:68` |
| UDP | `[::]:19133` | IPv6 status pings | `status.rs:40` |
| TCP | `0.0.0.0:19132` | HTTP signalling: `GET /v1/join` (status), `POST /v1/join/{network_id}` (SDP offer in, SDP answer out) | `crates/pumpkin-core/src/net/bedrock/nethernet/endpoint.rs:66-67` |
| UDP | `:7551` | NetherNet LAN discovery | `nethernet/discovery.rs:30` |

Legacy RakNet clients get an `IncompatibleProtocolVersion` reply (`status.rs:106`). There is no
Bedrock-level encryption handshake any more; DTLS inside WebRTC handles encryption.

**Why it matters:** the plan's "RakNet transport" doesn't exist in upstream. LAN play, and
probably P2P later, will run over NetherNet.

## 2. Accept path: from first packet to `Player`

1. `PumpkinServer::new` (`crates/pumpkin-core/src/lib.rs:251`) runs `bind_bedrock_status` (`:392`)
   and then `bind_nethernet` (`:361`), which calls `NetherNetListener::bind` (`endpoint.rs:41`).
   That function starts the axum HTTP router, LAN discovery (`endpoint.rs:72`) and the ICE router.
2. The client posts its SDP offer to `join` (`endpoint.rs:183`), which calls `negotiate_direct`
   (`nethernet/peer.rs:36`) to build the WebRTC peer connection.
3. Each incoming data channel is attached by `NetherNetSession::attach_channel`
   (`nethernet/session.rs:104`). Once **both** channels are open, `channel_opened` (`session.rs:172`)
   sends `(Arc<NetherNetSession>, SocketAddr)` into the listener's `incoming` mpsc (`session.rs:180`).
4. `PumpkinServer::start` (`lib.rs:454`) loops over `unified_listener_task` (`lib.rs:552`), a
   `tokio::select!` with one branch per listener. The NetherNet branch (`lib.rs:661`) then:
   - builds `BedrockClient::new(session, addr, ..)` (`lib.rs:669`);
   - starts its outgoing task (`lib.rs:675`);
   - spawns a read loop: `session.recv()` → `process_nethernet_packet` (`lib.rs:681-683`);
   - calls `spawn_bedrock_client_task` (`lib.rs:704`).
5. `spawn_bedrock_client_task` then:
   - runs `handle_login_sequence` (`lib.rs:707`);
   - registers the player with `server.add_player(ClientPlatform::Bedrock(client))` (`lib.rs:714`);
   - stays in `progress_player_packets` (`lib.rs:720`) until disconnect.

## 3. Inbound dispatch: bytes to handlers

1. `session.recv()` (`session.rs:221`) yields one reassembled batch, with 10 kB fragments already
   stitched back together.
2. `BedrockClient::process_nethernet_packet` (`crates/pumpkin-core/src/net/bedrock/mod.rs:262`)
   adds back the `0xFE` game-packet marker and calls `process_batch` (`mod.rs:603`). That function:
   - decompresses with `get_packet_payload` (`mod.rs:876` → `crates/pumpkin-protocol/src/bedrock/packet_decoder.rs:35`);
   - splits the batch into `RawPacket { id, payload }` (`mod.rs:619`);
   - applies the rate limiter;
   - pushes each packet into the `incoming_game_packet` mpsc (`mod.rs:648`).
3. **During login:** `handle_login_sequence` (`mod.rs:655`) reads from that mpsc and handles only
   `SRequestNetworkSettings` and `SLogin`.
4. **After login:** `progress_player_packets` (`mod.rs:703`) moves each packet onto
   `Player::inbound_packets`, a lock-free `SegQueue` (`mod.rs:705`).
5. **On the tick thread:** `Player::process_inbound_packets`
   (`crates/pumpkin-core/src/entity/player.rs:2642`) drains the queue and calls
   `BedrockClient::handle_play_packet` (`mod.rs:710`). That's a `match` on packet id that hands off
   to the handlers in `net/bedrock/play/*.rs`.

**Why the queue:** networking is async on tokio, but game logic runs synchronously inside the
tick. The `SegQueue` is the hand-off point, so packets are applied at a well-defined moment in
each tick.

## 4. Outbound path: game code to wire

1. Game code calls `ClientPlatform::{enqueue_packet_editioned, try_enqueue_packet_editioned, ..}`
   (`crates/pumpkin-core/src/net/mod.rs:226`, `:245`) or `World::broadcast_*`.
2. These call `BedrockClient::serialize_packet` (`mod.rs:516`) right away. **The struct becomes
   bytes here, before any transport sees it.** Broadcasts serialise once per recipient
   (`crates/pumpkin-core/src/world/mod.rs:1022-1027`).
   - The code matches on `ClientPlatform::Bedrock` 54 times across 13 files.
3. The bytes go into an unbounded mpsc via `try_enqueue_packet_data` (`mod.rs:457`), with a
   `MAX_PENDING_BYTES` back-pressure check.
4. `start_outgoing_packet_task` (`mod.rs:185`) drains up to 64 packets at a time, strips `0xFE`
   and calls `session.send` (`mod.rs:233`). That splits into ≤10,000-byte segments on the
   reliable channel (`session.rs:27`, `:229`).
5. Chunks follow the same route. `send_chunks` (`mod.rs:336`) encodes on Rayon
   (`mod.rs:381`) and then wraps each chunk with `encoder.write_game_packet` (`mod.rs:416`).

## 5. Where outbound compression happens, and whether LocalChannel can skip it

- **Where:** `BedrockBatchEncoder::write_game_packet`
  (`crates/pumpkin-protocol/src/bedrock/packet_encoder.rs:40`) deflates the payload at
  `:74-78`. It runs inside `serialize_packet`, `send_packet` and `send_chunks`, so in game code,
  **upstream of the seam**. A transport only ever receives finished, already-compressed bytes.
- **When:** both the encoder and the decoder start with `compression: None`
  (`packet_encoder.rs:27`, `packet_decoder.rs:55`). Compression is switched on only by
  `BedrockClient::set_compression` (`mod.rs:281`), and its **only** caller is
  `handle_request_network_settings`
  (`crates/pumpkin-core/src/net/bedrock/login/request_network_settings.rs:58`).
- **Can LocalChannel skip it without moving code? Yes.** `handle_login_sequence` accepts `SLogin`
  whether or not `SRequestNetworkSettings` came first. If the host never sends
  `SRequestNetworkSettings`, compression never turns on, and every packet travels as plain
  `0xFE + length-prefixed packets`.
- **What skipping `SRequestNetworkSettings` also skips:**
  1. The exact-protocol check (`request_network_settings.rs:6`). It doesn't matter in-process,
     because client and server share the same packet definitions.
  2. `self.version.store(..)`. The only reader is `ClientPlatform::bedrock_version`
     (`net/mod.rs:186`), and nothing in `pumpkin-core` calls it today.
- **Upstream bug candidate:** with compression on, `write_game_packet` ignores the threshold
  (`_threshold` at `packet_encoder.rs:74`). Every packet gets deflated, even tiny ones, where
  Bedrock would normally send them raw (method `0xFF`). Worth an upstream issue; leave it out of
  the fork.

## 6. What `handle_login_sequence` requires (input for M2's host login)

These rules apply with `online_mode = false`; online mode verifies the tokens on top of this.

**`SLogin`** (`crates/pumpkin-protocol/src/bedrock/server/login.rs:9`) has three fields:
`protocol_version: i32` (big-endian; not checked here), `jwt: Vec<u8>` and `raw_token: Vec<u8>`.

**`jwt`** is a JSON object (`crates/pumpkin-core/src/net/bedrock/login/mod.rs:58`):

```json
{ "AuthenticationType": 0, "Token": "<header>.<payload>.<signature>" }
```

- `AuthenticationType` must deserialize as 0 (Full), 1 (Guest) or 2 (SelfSigned). In offline
  mode it isn't checked beyond that.
- The `Token` payload is base64url-decoded **without checking the signature**
  (`login/login.rs:61` → `crates/pumpkin-auth/src/jwt/mod.rs:477`). The claims read are:

  | Claim | Used as | If missing |
  | --- | --- | --- |
  | `xname` | display name, then username (prefix and space rules apply) | empty string. `LoginError::InvalidUsername` exists but is never raised, so the host should always send one |
  | `xid` | XUID; UUID = `md5("pocket-auth-1-xuid:{xid}")`, version-3 style | falls back to `leguuid`; if both are empty, every such player gets the **same** UUID |
  | `cpk` | client public key, base64 P-384 | **login fails**: `extract_cpk_from_token` (`jwt/mod.rs:536`, called at `login.rs:92`) |

- `cpk` only has to match the transport's key **if the transport has one**
  (`nethernet_public_key().is_some_and(..)`, `login.rs:95`). A `LocalChannel` that returns `None`
  from `client_public_key()` accepts any well-formed `cpk`.

**`raw_token`** is a three-part JWT whose payload is `ClientData`
(`crates/pumpkin-protocol/src/bedrock/server/login.rs:138`), decoded without a signature check
(`login.rs:64-76`). The fields with no serde default, so required, are:

`ClientRandomId` (i64), `DeviceOS` (i32), `DeviceId`, `GameVersion`, `LanguageCode`,
`CurrentInputMode` (i32), `DefaultInputMode` (i32), `UIProfile` (i32), `ServerAddress`,
`MaxViewDistance` (i32).

Every other field (skin, persona, platform IDs, …) defaults. `LanguageCode` becomes the player's
locale.

**After login**, the server enqueues `PlayStatus::LoginSuccess` (`login.rs:105`) and
`ResourcePacksInfo` (`login.rs:125`), and returns `ReadyToPlay` (`login.rs:143`). Then
`add_player` runs. To actually spawn, the client must send:

1. `ResourcePackClientResponse(HAVE_ALL_PACKS)`. The server replies with `ResourcePackStack`
   (`login/resource_pack_response.rs:25`).
2. `ResourcePackClientResponse(COMPLETED)`. This calls `World::spawn_bedrock_player`
   (`resource_pack_response.rs:55-61`), which sends `StartGame`, then chunks.
3. `RequestChunkRadius` and `SetLocalPlayerAsInitialized`, which are normal play packets.

**Options for the host's login (decide in M2):**

- **(a) The client builds unsigned tokens.** It generates a P-384 key pair at startup, puts the
  public key in `cpk`, and fills `xname`/`xid` and the 10 `ClientData` fields.
  - Zero fork diff.
  - Only works while the embedded server runs with `online_mode = false`, which would also apply
    to LAN guests.
- **(b) A login seam for local transports.** `LocalChannel` hands over a ready `GameProfile`, and
  the accept path skips `handle_login_sequence` for it.
  - LAN guests stay verifiable while the host needs no token at all.
  - Costs a small edit in `spawn_bedrock_client_task` (`lib.rs:704`).

## 7. Proposed `Transport` seam

`BedrockClient` touches its connection **only** through `session: Arc<NetherNetSession>`
(`mod.rs:98`). Every use of it:

| Method | Call site |
| --- | --- |
| `send(Bytes)` | `mod.rs:233` (outgoing task) |
| `recv()` | `lib.rs:681` (read loop; holds the session directly) |
| `flush_reliable(Duration)` | `login/request_network_settings.rs:24-25` (flush before a version-mismatch close) |
| `close()` | `mod.rs:586` |
| `is_closed()` | `mod.rs:596` |
| `client_public_key()` | `mod.rs:278` (login identity check) |

`send_unreliable` exists on the session but `BedrockClient` never calls it.

```rust
/// The one place a Bedrock player's bytes enter and leave the server.
///
/// Exists so a player can be connected by NetherNet (LAN or remote peers) or
/// by an in-process `LocalChannel` (the host) without game code knowing which.
pub trait Transport: Send + Sync {
    fn send(&self, batch: Bytes) -> BoxFuture<'_, Result<(), String>>;
    fn recv(&self) -> BoxFuture<'_, Option<Bytes>>;
    fn flush_reliable(&self, timeout: Duration) -> BoxFuture<'_, Result<(), String>>;
    fn close(&self) -> BoxFuture<'_, ()>;
    fn is_closed(&self) -> bool;
    fn client_public_key(&self) -> Option<&PublicKey>;
}
```

Design notes:

- **Why these exact names:** they match `NetherNetSession`'s methods, so the call sites above
  compile unchanged. Only the field type changes. That is what keeps the fork diff small.
- **Why `BoxFuture` instead of `async fn`:** `BedrockClient` stores the transport as
  `Arc<dyn Transport>`. Rust can call a trait through `dyn` only if the trait is *dyn-compatible*
  (formerly "object safe"). A plain `async fn` in a trait returns a different hidden future type
  for each implementer, which `dyn` can't express. Returning `Pin<Box<dyn Future>>` (the
  `futures::future::BoxFuture` alias) gives every implementer the same return type. It costs one
  small allocation per call, which is nothing next to a network send.
  - The alternative is an `enum BedrockTransport { NetherNet(..), Local(..) }`, which needs no
    boxing.
  - The trait wins because P2P and test transports can be added without touching the enum.
- **Edits to existing Pumpkin files (M1):**
  - `net/bedrock/mod.rs`: change the field and `new()` from `Arc<NetherNetSession>` to
    `Arc<dyn Transport>`.
  - `lib.rs`: move the NetherNet branch body (`lib.rs:663-693`) into an
    `accept_bedrock(transport, addr)` helper.
  - M2 then adds one more `select!` branch that accepts local connections from an mpsc.
- **New files:** `net/bedrock/transport.rs` holds the trait and
  `impl Transport for NetherNetSession`. `LocalChannel` comes in M2.

## 8. Other findings

- **Packet structs are one-directional.** Clientbound structs only derive `PacketWrite` (60 of 61
  files in `crates/pumpkin-protocol/src/bedrock/client/`), and 18 of 29 serverbound structs lack
  `PacketWrite`. The client needs the opposite of each.
  - The `PacketRead`/`PacketWrite` derives already exist, so the fix is one line per file.
  - Proposed: put it behind a `client` cargo feature in `pumpkin-protocol`; decide in M2.
- **Java is everywhere.** `JavaClient` / `ClientPlatform::Java` appear in 93 files of
  `pumpkin-core`. Compiling all of Java out would be a huge diff, but compiling out the
  *listener* (TCP bind, query, LAN broadcast in `lib.rs:279-331`) is small. Runtime config can
  already disable it (`networking.java.enabled`).
- **No light data.** `CLevelChunk::encode_chunk` (`net/bedrock/level_chunk.rs:68`) writes only:
  - sub-chunks in format version 9 (block palette, plus a water layer when present);
  - biome palettes;
  - a border byte (always 0);
  - block actor NBT.

  So the client computes all lighting. This is also the layout M3 has to decode.
- **Telemetry is on by default.** It sends a heartbeat every 300 s to
  `market.pumpkinmc.org` (`crates/pumpkin-config/src/telemetry.rs:19`). The embedded server
  should default it to off.
- **The build bakes Mojang data into the binary.** `crates/pumpkin-data/build.rs` downloads
  the Minecraft **Java 26.3** server jar from Mojang's version manifest (`build.rs:6`, `:98-131`) and
  extracts its vanilla datapack into `assets/datapack/` (gitignored, `.gitignore:112`).
  - Generated code then embeds that JSON with `include_str!`, e.g.
    `crates/pumpkin-data/src/generated/context_provider.rs` and
    `crates/pumpkin-world/src/world_info/mod.rs:252`.
  - `PUMPKIN_MINECRAFT_SERVER_JAR=<path>` makes it use a local jar instead.
  - Local dev builds are fine. A distributed binary would bundle Mojang data, which the project
    rules forbid; tracked as a risk and open question in `docs/PLAN.md`.
- **Windows:** cloning needs `git config core.longpaths true`, because some checkout paths go
  past `MAX_PATH`.
- **Build:** the release profile uses fat LTO with `codegen-units = 1`. For local test builds,
  `CARGO_PROFILE_RELEASE_LTO=false CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16` skips the slow
  whole-program link step, and env-var overrides don't touch any fork files.

## 9. Join test (Milestone 0 acceptance)

2026-10-19, Windows 11.
- **Server:** the fork at `aac0a82`, built with
  `CARGO_PROFILE_RELEASE_LTO=false CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16 cargo build --release --locked -p pumpkin`
  (5 min 27 s), and `PUMPKIN_MINECRAFT_SERVER_JAR` pointing at a SHA-1-verified Java 26.3
  `server.jar`.
- **Client:** official Bedrock **1.26.52** (package 1.26.5203.0), signed in to Xbox, added as a
  server at `127.0.0.1:19132`.
- **Config:** `run/pumpkin/pumpkin.toml` with `networking.bedrock.online_mode = false`,
  `networking.java.enabled = false` and `telemetry.enabled = false`.
- **Logging:** `RUST_LOG=info,pumpkin_core::net::bedrock=debug`.

Server log, with the gamertag redacted and per-datagram ICE debug lines removed:

```
INFO  Bedrock server-list status is listening on 0.0.0.0:19132 (IPv4) and [::]:19133 (IPv6)
INFO  Bedrock NetherNet LAN discovery is listening on 0.0.0.0:7551
INFO  Bedrock NetherNet signaling is listening on 0.0.0.0:19132
INFO  Bedrock NetherNet ICE is listening on 0.0.0.0:19132
INFO  Started server; took 359ms
DEBUG Accepted Bedrock NetherNet connection from 127.0.0.1:64647
DEBUG Bedrock: SResourcePackResponse::STATUS_HAVE_ALL_PACKS
DEBUG Bedrock: SResourcePackResponse::STATUS_COMPLETED
INFO  <gamertag> joined the game
DEBUG Player <gamertag> updated their render distance: 8 -> 16.
DEBUG Player <gamertag> initialized (Runtime ID: 1)
```

What this shows:
- The official client completed the whole path in sections 2, 3 and 6 over NetherNet on
  loopback: WebRTC negotiation, login, the resource pack handshake, spawn, `RequestChunkRadius`
  and `SetLocalPlayerAsInitialized`.
- **Bedrock 1.26.52 speaks protocol 2193.** It passed the exact-match check in
  `handle_request_network_settings`, so the 1.26.51 → 1.26.52 hotfix kept the protocol number.
- Loopback worked from the Store build without any UWP loopback exemption.
- In game, the owner confirmed that terrain loaded and walking works. **M0 acceptance passed.**
