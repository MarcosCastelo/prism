import React, { useState, useEffect, useCallback } from "react";
import {
  View,
  Text,
  FlatList,
  TouchableOpacity,
  StyleSheet,
  RefreshControl,
  ActivityIndicator,
} from "react-native";
import { useNavigation } from "@react-navigation/native";
import type { StackNavigationProp } from "@react-navigation/stack";
import type { RootStackParamList } from "../App";

interface StreamInfo {
  stream_id: string;
  streamer_pubkey: string;
  streamer_name: string;
  title: string;
  viewer_count: number;
  started_at_ms: number;
  quality: string;
}

type Nav = StackNavigationProp<RootStackParamList, "Discover">;

const EDGE_BASE = "http://localhost:8080"; // override via env or settings

async function fetchLiveStreams(): Promise<StreamInfo[]> {
  try {
    const res = await fetch(`${EDGE_BASE}/streams/live`, {
      signal: AbortSignal.timeout(8_000),
    });
    if (!res.ok) throw new Error(`HTTP ${res.status}`);
    return (await res.json()) as StreamInfo[];
  } catch {
    // Return mock data when the edge node is not reachable.
    return [
      {
        stream_id: "abc123",
        streamer_pubkey: "deadbeef",
        streamer_name: "alice.prism",
        title: "Gamedev ao vivo — Rust + WGPU",
        viewer_count: 42,
        started_at_ms: Date.now() - 1_800_000,
        quality: "1080p",
      },
      {
        stream_id: "def456",
        streamer_pubkey: "cafebabe",
        streamer_name: "bob.prism",
        title: "Música eletrônica — improvisação",
        viewer_count: 18,
        started_at_ms: Date.now() - 600_000,
        quality: "720p",
      },
    ];
  }
}

function formatElapsed(startMs: number): string {
  const s = Math.floor((Date.now() - startMs) / 1000);
  if (s < 3600) return `${Math.floor(s / 60)}min`;
  return `${Math.floor(s / 3600)}h${Math.floor((s % 3600) / 60)}min`;
}

export function DiscoverScreen() {
  const navigation = useNavigation<Nav>();
  const [streams, setStreams] = useState<StreamInfo[]>([]);
  const [loading, setLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);

  async function load() {
    const data = await fetchLiveStreams();
    setStreams(data);
    setLoading(false);
  }

  useEffect(() => {
    load();
  }, []);

  const onRefresh = useCallback(async () => {
    setRefreshing(true);
    await load();
    setRefreshing(false);
  }, []);

  if (loading) {
    return (
      <View style={styles.centered}>
        <ActivityIndicator color="#4f9cf9" size="large" />
        <Text style={styles.loadingText}>Buscando streams na rede P2P…</Text>
      </View>
    );
  }

  return (
    <View style={styles.container}>
      <Text style={styles.header}>🔴 Ao Vivo Agora</Text>
      <FlatList
        data={streams}
        keyExtractor={(item) => item.stream_id}
        contentContainerStyle={{ paddingBottom: 24 }}
        refreshControl={
          <RefreshControl
            refreshing={refreshing}
            onRefresh={onRefresh}
            tintColor="#4f9cf9"
          />
        }
        renderItem={({ item }) => (
          <TouchableOpacity
            style={styles.card}
            onPress={() =>
              navigation.navigate("Stream", {
                streamId: item.stream_id,
                title: item.title,
                streamerName: item.streamer_name,
              })
            }
          >
            <View style={styles.cardTop}>
              <View style={styles.liveBadge}>
                <View style={styles.liveDot} />
                <Text style={styles.liveText}>LIVE</Text>
              </View>
              <Text style={styles.quality}>{item.quality}</Text>
            </View>
            <Text style={styles.title} numberOfLines={2}>
              {item.title}
            </Text>
            <View style={styles.cardBottom}>
              <Text style={styles.streamerName}>{item.streamer_name}</Text>
              <Text style={styles.meta}>
                {item.viewer_count} viewers · {formatElapsed(item.started_at_ms)}
              </Text>
            </View>
          </TouchableOpacity>
        )}
        ListEmptyComponent={
          <View style={styles.empty}>
            <Text style={styles.emptyIcon}>📡</Text>
            <Text style={styles.emptyText}>
              Nenhum stream ao vivo no momento.{"\n"}Puxe para atualizar.
            </Text>
          </View>
        }
      />
    </View>
  );
}

const styles = StyleSheet.create({
  container: {
    flex: 1,
    backgroundColor: "#171923",
    paddingHorizontal: 16,
    paddingTop: 16,
  },
  centered: {
    flex: 1,
    backgroundColor: "#171923",
    alignItems: "center",
    justifyContent: "center",
    gap: 12,
  },
  loadingText: {
    color: "#718096",
    fontSize: 14,
  },
  header: {
    color: "#e2e8f0",
    fontSize: 22,
    fontWeight: "700",
    marginBottom: 16,
  },
  card: {
    backgroundColor: "#1a2035",
    borderRadius: 12,
    padding: 16,
    marginBottom: 12,
    borderWidth: 1,
    borderColor: "#2d3748",
  },
  cardTop: {
    flexDirection: "row",
    justifyContent: "space-between",
    alignItems: "center",
    marginBottom: 8,
  },
  liveBadge: {
    flexDirection: "row",
    alignItems: "center",
    gap: 5,
    backgroundColor: "#742a2a",
    paddingHorizontal: 8,
    paddingVertical: 3,
    borderRadius: 4,
  },
  liveDot: {
    width: 6,
    height: 6,
    borderRadius: 3,
    backgroundColor: "#fc8181",
  },
  liveText: {
    color: "#fc8181",
    fontSize: 11,
    fontWeight: "700",
  },
  quality: {
    color: "#718096",
    fontSize: 12,
    backgroundColor: "#2d3748",
    paddingHorizontal: 8,
    paddingVertical: 2,
    borderRadius: 4,
  },
  title: {
    color: "#e2e8f0",
    fontSize: 16,
    fontWeight: "600",
    marginBottom: 10,
    lineHeight: 22,
  },
  cardBottom: {
    flexDirection: "row",
    justifyContent: "space-between",
    alignItems: "center",
  },
  streamerName: {
    color: "#4f9cf9",
    fontSize: 13,
    fontWeight: "600",
  },
  meta: {
    color: "#718096",
    fontSize: 12,
  },
  empty: {
    alignItems: "center",
    paddingTop: 60,
    gap: 12,
  },
  emptyIcon: {
    fontSize: 48,
  },
  emptyText: {
    color: "#718096",
    fontSize: 14,
    textAlign: "center",
    lineHeight: 22,
  },
});
