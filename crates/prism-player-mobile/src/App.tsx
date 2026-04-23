import React from "react";
import { NavigationContainer } from "@react-navigation/native";
import { createStackNavigator } from "@react-navigation/stack";
import { SafeAreaProvider } from "react-native-safe-area-context";
import { StatusBar } from "expo-status-bar";
import { DiscoverScreen } from "./screens/DiscoverScreen";
import { StreamScreen } from "./screens/StreamScreen";
import { IdentityScreen } from "./screens/IdentityScreen";
import { View, TouchableOpacity, Text, StyleSheet } from "react-native";

export type RootStackParamList = {
  Discover: undefined;
  Stream: { streamId: string; title: string; streamerName: string };
  Identity: undefined;
};

const Stack = createStackNavigator<RootStackParamList>();

export default function App() {
  return (
    <SafeAreaProvider>
      <NavigationContainer>
        <StatusBar style="light" />
        <Stack.Navigator
          initialRouteName="Discover"
          screenOptions={{
            headerStyle: { backgroundColor: "#171923" },
            headerTintColor: "#e2e8f0",
            headerTitleStyle: { fontWeight: "700" },
            cardStyle: { backgroundColor: "#171923" },
          }}
        >
          <Stack.Screen
            name="Discover"
            component={DiscoverScreen}
            options={({ navigation }) => ({
              title: "Prism",
              headerRight: () => (
                <TouchableOpacity
                  style={{ marginRight: 16 }}
                  onPress={() => navigation.navigate("Identity")}
                >
                  <Text style={{ color: "#4f9cf9", fontSize: 15 }}>🔑</Text>
                </TouchableOpacity>
              ),
            })}
          />
          <Stack.Screen
            name="Stream"
            component={StreamScreen}
            options={{ headerShown: false }}
          />
          <Stack.Screen
            name="Identity"
            component={IdentityScreen}
            options={{ title: "Identidade" }}
          />
        </Stack.Navigator>
      </NavigationContainer>
    </SafeAreaProvider>
  );
}

const styles = StyleSheet.create({});
