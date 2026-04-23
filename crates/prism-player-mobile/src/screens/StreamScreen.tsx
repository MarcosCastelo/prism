import React, { useRef, useState, useEffect, useCallback } from "react";
import {
  View,
  Text,
  StyleSheet,
  TouchableOpacity,
  ActivityIndicator,
  StatusBar,
} from "react-native";
import { Video, ResizeMode, type AVPlaybackStatus } from "expo-av";
import { useNavigation, useRoute, type RouteProp } from "@react-navigation/native";
import type { RootStackParamList } from "../App";
import { HlsPlayer, type PlayerState } from "../modules/HlsPlayer";

type StreamRoute = RouteProp<RootStackParamList, "Stream">;

const EDGE_BASE = "http://localhost:8080";

export function StreamScreen() {
  const navigation = useNavigation();
  const route = useRoute<StreamRoute>();
  const { streamId, title, streamerName } = route.params;

  const videoRef = useRef<Video>(null);
  const [playerState, setPlayerState] = useState<PlayerState>({
    currentQuality: "auto",
    isBuffering: true,
    latencyEstimate: 0,
  });
  const [error, setError] = useState<string | null>(null);
  const [controlsVisible, setControlsVisible] = useState(true);

  const hlsUrl = `${EDGE_BASE}/stream/${streamId}/index.m3u8`;

  const playerRef = useRef(
    new HlsPlayer({
      streamUrl: hlsUrl,
      onStateChange: setPlayerState,
      onError: setError,
    })
  );

  useEffect(() => {
    const player = playerRef.current;
    player.setVideoRef(videoRef);
    player.load().catch((e) => setError(String(e)));

    return () => {
      player.stop().catch(() => null);
    };
  }, []);

  // Auto-hide controls after 4 seconds.
  useEffect(() => {
    if (!controlsVisible) return;
    const t = setTimeout(() => setControlsVisible(false), 4_000);
    return () => clearTimeout(t);
  }, [controlsVisible]);

  const handlePlaybackStatus = useCallback((status: AVPlaybackStatus) => {
    playerRef.current.handlePlaybackStatus(status);
  }, []);

  return (
    <View style={styles.container}>
      <StatusBar hidden />

      <TouchableOpacity
        style={styles.videoArea}
        activeOpacity={1}
        onPress={() => setControlsVisible((v) => !v)}
      >
        <Video
          ref={videoRef}
          style={styles.video}
          resizeMode={ResizeMode.CONTAIN}
          onPlaybackStatusUpdate={handlePlaybackStatus}
          useNativeControls={false}
        />

        {playerState.isBuffering && !error && (
          <View style={styles.bufferingOverlay}>
            <ActivityIndicator color="#fff" size="large" />
          </View>
        )}

        {error && (
          <View style={styles.errorOverlay}>
            <Text style={styles.errorIcon}>⚠️</Text>
            <Text style={styles.errorText}>{error}</Text>
            <TouchableOpacity
              style={styles.retryBtn}
              onPress={() => {
                setError(null);
                playerRef.current.load().catch((e) => setError(String(e)));
              }}
            >
              <Text style={styles.retryText}>Tentar novamente</Text>
            </TouchableOpacity>
          </View>
        )}

        {controlsVisible && (
          <View style={styles.controls}>
            <TouchableOpacity
              style={styles.backBtn}
              onPress={() => navigation.goBack()}
            >
              <Text style={styles.backText}>← Voltar</Text>
            </TouchableOpacity>

            <View style={styles.info}>
              <Text style={styles.infoTitle} numberOfLines={1}>
                {title}
              </Text>
              <Text style={styles.infoStreamer}>{streamerName}</Text>
            </View>

            <View style={styles.statsRow}>
              <StatPill label="Qualidade" value={playerState.currentQuality} />
              <StatPill
                label="Latência"
                value={
                  playerState.latencyEstimate > 0
                    ? `${(playerState.latencyEstimate / 1000).toFixed(1)}s`
                    : "—"
                }
              />
            </View>
          </View>
        )}
      </TouchableOpacity>
    </View>
  );
}

function StatPill({ label, value }: { label: string; value: string }) {
  return (
    <View style={styles.statPill}>
      <Text style={styles.statLabel}>{label}</Text>
      <Text style={styles.statValue}>{value}</Text>
    </View>
  );
}

const styles = StyleSheet.create({
  container: {
    flex: 1,
    backgroundColor: "#000",
  },
  videoArea: {
    flex: 1,
    backgroundColor: "#000",
  },
  video: {
    flex: 1,
  },
  bufferingOverlay: {
    ...StyleSheet.absoluteFillObject,
    alignItems: "center",
    justifyContent: "center",
    backgroundColor: "rgba(0,0,0,0.4)",
  },
  errorOverlay: {
    ...StyleSheet.absoluteFillObject,
    alignItems: "center",
    justifyContent: "center",
    backgroundColor: "rgba(0,0,0,0.75)",
    padding: 32,
    gap: 12,
  },
  errorIcon: {
    fontSize: 40,
  },
  errorText: {
    color: "#fc8181",
    fontSize: 14,
    textAlign: "center",
  },
  retryBtn: {
    marginTop: 8,
    paddingHorizontal: 24,
    paddingVertical: 10,
    backgroundColor: "#4f9cf9",
    borderRadius: 8,
  },
  retryText: {
    color: "#fff",
    fontWeight: "600",
    fontSize: 15,
  },
  controls: {
    ...StyleSheet.absoluteFillObject,
    justifyContent: "space-between",
    padding: 16,
    backgroundColor: "rgba(0,0,0,0.3)",
  },
  backBtn: {
    alignSelf: "flex-start",
    paddingHorizontal: 12,
    paddingVertical: 6,
    backgroundColor: "rgba(0,0,0,0.5)",
    borderRadius: 6,
  },
  backText: {
    color: "#fff",
    fontSize: 14,
    fontWeight: "600",
  },
  info: {
    alignItems: "center",
  },
  infoTitle: {
    color: "#fff",
    fontSize: 16,
    fontWeight: "700",
    textAlign: "center",
  },
  infoStreamer: {
    color: "#4f9cf9",
    fontSize: 13,
    marginTop: 4,
  },
  statsRow: {
    flexDirection: "row",
    justifyContent: "center",
    gap: 12,
  },
  statPill: {
    backgroundColor: "rgba(0,0,0,0.6)",
    borderRadius: 6,
    paddingHorizontal: 12,
    paddingVertical: 6,
    alignItems: "center",
  },
  statLabel: {
    color: "#718096",
    fontSize: 10,
    textTransform: "uppercase",
    letterSpacing: 0.5,
  },
  statValue: {
    color: "#fff",
    fontSize: 14,
    fontWeight: "700",
  },
});
