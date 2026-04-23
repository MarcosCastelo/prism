import React, { useState, useEffect } from "react";
import {
  View,
  Text,
  StyleSheet,
  TouchableOpacity,
  Alert,
  ActivityIndicator,
  ScrollView,
} from "react-native";
import {
  hasStoredKey,
  generateAndStoreKey,
  getPublicKeyHex,
  deleteStoredKey,
} from "../modules/SecureKey";

export function IdentityScreen() {
  const [pubkeyHex, setPubkeyHex] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [generating, setGenerating] = useState(false);

  useEffect(() => {
    loadIdentity();
  }, []);

  async function loadIdentity() {
    setLoading(true);
    try {
      const exists = await hasStoredKey();
      if (exists) {
        const hex = await getPublicKeyHex();
        setPubkeyHex(hex);
      }
    } catch (e) {
      console.error("Failed to load identity:", e);
    } finally {
      setLoading(false);
    }
  }

  async function handleGenerate() {
    setGenerating(true);
    try {
      const hex = await generateAndStoreKey();
      setPubkeyHex(hex);
    } catch (e) {
      Alert.alert("Erro", `Falha ao gerar identidade: ${e}`);
    } finally {
      setGenerating(false);
    }
  }

  function handleDelete() {
    Alert.alert(
      "Apagar Identidade",
      "Isso removerá sua chave privada permanentemente. Sem backup, você perderá acesso à sua conta Prism. Confirmar?",
      [
        { text: "Cancelar", style: "cancel" },
        {
          text: "Apagar",
          style: "destructive",
          onPress: async () => {
            try {
              await deleteStoredKey();
              setPubkeyHex(null);
            } catch (e) {
              Alert.alert("Erro", String(e));
            }
          },
        },
      ]
    );
  }

  if (loading) {
    return (
      <View style={styles.centered}>
        <ActivityIndicator color="#4f9cf9" size="large" />
      </View>
    );
  }

  const displayPubkey = pubkeyHex
    ? `pr1${pubkeyHex.slice(0, 4)}...${pubkeyHex.slice(-4)}`
    : null;

  return (
    <ScrollView style={styles.container} contentContainerStyle={styles.content}>
      <Text style={styles.title}>Identidade Prism</Text>
      <Text style={styles.subtitle}>
        Sua chave Ed25519 armazenada com segurança no{" "}
        {isIOS() ? "iOS Keychain" : "Android Keystore"}.
      </Text>

      {pubkeyHex ? (
        <>
          <View style={styles.card}>
            <Text style={styles.cardLabel}>Endereço Prism</Text>
            <Text style={styles.pubkeyDisplay}>{displayPubkey}</Text>
            <Text style={styles.pubkeyFull} selectable>
              {pubkeyHex}
            </Text>
          </View>

          <View style={styles.infoBox}>
            <Text style={styles.infoIcon}>🔒</Text>
            <Text style={styles.infoText}>
              A chave privada nunca sai do {isIOS() ? "Keychain" : "Keystore"}.
              Operações de assinatura são delegadas ao enclave seguro.
            </Text>
          </View>

          <TouchableOpacity style={styles.dangerBtn} onPress={handleDelete}>
            <Text style={styles.dangerText}>Apagar Identidade</Text>
          </TouchableOpacity>
        </>
      ) : (
        <>
          <View style={styles.emptyBox}>
            <Text style={styles.emptyIcon}>🔑</Text>
            <Text style={styles.emptyText}>
              Nenhuma identidade encontrada.{"\n"}
              Gere uma para participar da rede Prism.
            </Text>
          </View>

          <TouchableOpacity
            style={[styles.generateBtn, generating && styles.disabledBtn]}
            onPress={handleGenerate}
            disabled={generating}
          >
            {generating ? (
              <ActivityIndicator color="#fff" />
            ) : (
              <Text style={styles.generateText}>Gerar Identidade</Text>
            )}
          </TouchableOpacity>
        </>
      )}
    </ScrollView>
  );
}

function isIOS(): boolean {
  const { Platform } = require("react-native");
  return Platform.OS === "ios";
}

const styles = StyleSheet.create({
  container: {
    flex: 1,
    backgroundColor: "#171923",
  },
  content: {
    padding: 24,
    gap: 16,
  },
  centered: {
    flex: 1,
    backgroundColor: "#171923",
    alignItems: "center",
    justifyContent: "center",
  },
  title: {
    color: "#e2e8f0",
    fontSize: 24,
    fontWeight: "700",
    marginBottom: 4,
  },
  subtitle: {
    color: "#718096",
    fontSize: 14,
    lineHeight: 20,
    marginBottom: 8,
  },
  card: {
    backgroundColor: "#1a2035",
    borderRadius: 12,
    padding: 20,
    borderWidth: 1,
    borderColor: "#2d3748",
    gap: 6,
  },
  cardLabel: {
    color: "#718096",
    fontSize: 11,
    textTransform: "uppercase",
    letterSpacing: 0.5,
  },
  pubkeyDisplay: {
    color: "#4f9cf9",
    fontSize: 22,
    fontWeight: "700",
    fontFamily: "monospace",
  },
  pubkeyFull: {
    color: "#4a5568",
    fontSize: 11,
    fontFamily: "monospace",
    marginTop: 4,
  },
  infoBox: {
    flexDirection: "row",
    gap: 12,
    backgroundColor: "#1a2035",
    borderRadius: 10,
    padding: 14,
    borderWidth: 1,
    borderColor: "#2d3748",
    alignItems: "flex-start",
  },
  infoIcon: {
    fontSize: 20,
  },
  infoText: {
    color: "#718096",
    fontSize: 13,
    lineHeight: 19,
    flex: 1,
  },
  dangerBtn: {
    marginTop: 8,
    padding: 14,
    borderRadius: 10,
    borderWidth: 1,
    borderColor: "#742a2a",
    alignItems: "center",
  },
  dangerText: {
    color: "#fc8181",
    fontSize: 15,
    fontWeight: "600",
  },
  emptyBox: {
    alignItems: "center",
    paddingVertical: 32,
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
  generateBtn: {
    backgroundColor: "#4f9cf9",
    borderRadius: 10,
    padding: 16,
    alignItems: "center",
  },
  disabledBtn: {
    opacity: 0.6,
  },
  generateText: {
    color: "#fff",
    fontSize: 16,
    fontWeight: "700",
  },
});
