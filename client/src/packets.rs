//! Packet names and batch framing for the host's side of a `LocalChannel`.

use std::{fmt, io::Cursor};

use bytes::Bytes;
use pumpkin_core::net::bedrock::level_chunk::CLevelChunk;
use pumpkin_protocol::{
    BClientPacket, PacketDecodeError, RawPacket,
    bedrock::{
        client::*,
        packet_decoder::BedrockBatchDecoder,
        packet_encoder::BedrockBatchEncoder,
        server::{
            actor_event::SActorEvent, animate::SAnimate, container_close::SContainerClose,
            respawn::SRespawn, text::SText,
        },
    },
    packet::Packet,
};

/// Builds `NAMES`, one `(id, name)` per clientbound packet type, from each
/// type's own `PACKET_ID`. Ids can't drift: if Pumpkin renumbers a packet,
/// the table follows.
macro_rules! packet_names {
    ($($packet:ident),* $(,)?) => {
        const NAMES: &[(i32, &str)] = &[$(($packet::PACKET_ID, stringify!($packet))),*];
    };
}

packet_names!(
    CAddActor,
    CAddItemActor,
    CAddPlayer,
    CAvailableCommands,
    CBiomeDefinitionList,
    CBlockActorData,
    CBlockEvent,
    CBossEvent,
    CChangeDimension,
    CChunkRadiusUpdated,
    CClientCacheMissResponse,
    CContainerOpen,
    CCorrectPlayerMovePrediction,
    CCraftingData,
    CCreativeContent,
    CDisconnect,
    CGamerulesChanged,
    CInventoryContent,
    CInventorySlot,
    CItemRegistry,
    CItemStackResponse,
    CJigsawStructureData,
    CLevelChunk,
    CLevelEvent,
    CLevelSoundEvent,
    CMobEffect,
    CMobEquipment,
    CModalFormRequest,
    CMoveActorAbsolute,
    CMoveActorDelta,
    CMovePlayer,
    CNetworkChunkPublisherUpdate,
    CNetworkSettings,
    CPlayStatus,
    CPlayerHotbar,
    CPlayerList,
    CRemoveActor,
    CRemoveObjective,
    CResourcePackStackPacket,
    CResourcePacksInfo,
    CSetActorData,
    CSetActorLink,
    CSetActorMotion,
    CSetDifficulty,
    CSetDisplayObjective,
    CSetHealth,
    CSetPlayerGameType,
    CSetScore,
    CSetSpawnPosition,
    CSetTime,
    CSetTitle,
    CShowCredits,
    CStartGame,
    CTakeItemActor,
    CTransfer,
    CUpdateAbilities,
    CUpdateAttributes,
    CUpdateBlock,
    CUpdateTrade,
    CVoxelShapes,
    // Pumpkin sends these serverbound structs to clients too: the packets go
    // both ways with the same layout.
    SActorEvent,
    SAnimate,
    SContainerClose,
    SRespawn,
    SText,
);

/// A clientbound packet id that prints as its Pumpkin type name, or
/// `Unknown(id)`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PacketName(pub i32);

impl PacketName {
    pub fn get(self) -> Option<&'static str> {
        NAMES
            .iter()
            .find(|(id, _)| *id == self.0)
            .map(|(_, name)| *name)
    }
}

impl fmt::Display for PacketName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.get() {
            Some(name) => f.write_str(name),
            None => write!(f, "Unknown({})", self.0),
        }
    }
}

/// Splits one batch from the server into its packets.
///
/// The server strips the `0xFE` batch marker before handing a batch to the
/// transport, and compression is never switched on for the host, so the
/// batch is just `[length][header][body]` repeated.
pub fn split_batch(batch: Bytes) -> Result<Vec<RawPacket>, PacketDecodeError> {
    let mut decoder = BedrockBatchDecoder::new();
    let len = batch.len() as u64;
    let mut cursor = Cursor::new(Vec::from(batch));
    let mut packets = Vec::new();
    while cursor.position() < len {
        packets.push(decoder.get_game_packet(&mut cursor)?);
    }
    Ok(packets)
}

/// Frames one packet as a batch for the server.
///
/// `BedrockBatchEncoder` writes the `0xFE` marker first; the transport doesn't
/// carry it, and the server adds it back, so it's dropped here.
pub fn frame<P: BClientPacket>(packet: &P) -> std::io::Result<Bytes> {
    let mut batch = Vec::new();
    BedrockBatchEncoder::new().write_packet(packet, &mut batch)?;
    batch.remove(0);
    Ok(batch.into())
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use pumpkin_protocol::{
        bedrock::server::set_local_player_as_initialized::SSetLocalPlayerAsInitialized,
        codec::var_ulong::VarULong,
    };

    use super::*;

    #[test]
    fn names_have_unique_ids() {
        let ids: HashSet<_> = NAMES.iter().map(|(id, _)| id).collect();
        assert_eq!(ids.len(), NAMES.len());
    }

    #[test]
    fn names_known_and_unknown_ids() {
        assert_eq!(PacketName(11).to_string(), "CStartGame");
        assert_eq!(PacketName(58).to_string(), "CLevelChunk");
        assert_eq!(PacketName(9).to_string(), "SText");
        assert_eq!(PacketName(27).to_string(), "SActorEvent");
        assert_eq!(PacketName(9999).to_string(), "Unknown(9999)");
    }

    #[test]
    fn a_framed_packet_splits_back_into_itself() {
        let packet = SSetLocalPlayerAsInitialized {
            player_id: VarULong(7),
        };
        let batch = frame(&packet).unwrap();
        assert_ne!(batch[0], 0xFE);

        let packets = split_batch(batch).unwrap();
        assert_eq!(packets.len(), 1);
        assert_eq!(packets[0].id, SSetLocalPlayerAsInitialized::PACKET_ID);
        assert_eq!(packets[0].payload.as_ref(), [7]);
    }
}
