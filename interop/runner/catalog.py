"""Peer catalog load and publisher-alphabetical sort."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Iterable, List

from .paths import PEERS_FILE, load_yaml


@dataclass(frozen=True)
class Peer:
    id: str
    publisher: str
    product: str
    version: str
    role: str
    image: str
    family: str
    notes: str = ""

    @property
    def sort_key(self) -> tuple:
        return (
            self.publisher.casefold(),
            self.product.casefold(),
            self._version_tuple(self.version),
            self.id,
        )

    @staticmethod
    def _version_tuple(version: str) -> tuple:
        parts: List[object] = []
        for chunk in version.replace("-", ".").split("."):
            if chunk.isdigit():
                parts.append(int(chunk))
            else:
                parts.append(chunk)
        return tuple(parts)


def load_peers(path=PEERS_FILE) -> list[Peer]:
    raw = load_yaml(path)
    peers = []
    for item in raw.get("peers", []):
        peers.append(
            Peer(
                id=item["id"],
                publisher=item["publisher"],
                product=item["product"],
                version=str(item["version"]),
                role=item["role"],
                image=item["image"],
                family=item["family"],
                notes=item.get("notes", ""),
            )
        )
    return sorted_peers(peers)


def sorted_peers(peers: Iterable[Peer]) -> list[Peer]:
    return sorted(peers, key=lambda p: p.sort_key)


def peer_by_id(peer_id: str) -> Peer:
    for peer in load_peers():
        if peer.id == peer_id:
            return peer
    raise KeyError(f"unknown peer id: {peer_id}")
