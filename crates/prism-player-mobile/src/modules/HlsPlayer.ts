// HLS playback via expo-av.
//
// iOS: AVPlayer — AV1 hardware decode on A17 Pro+ (iPhone 15 Pro+).
//      Fallback to H.264 on older devices.
// Android: ExoPlayer — AV1 software decode on Android 10+,
//           hardware decode on Android 12+ with supported chipsets.
//
// The HLS manifest served by edge nodes lists both AV1 and H.264 renditions.
// AVPlayer / ExoPlayer select the best rendition the device can decode.

import { AVPlaybackStatus, Video } from "expo-av";

export interface PlayerState {
  currentQuality: string;
  isBuffering: boolean;
  latencyEstimate: number;
}

export interface HlsPlayerOptions {
  streamUrl: string;
  onStateChange?: (state: PlayerState) => void;
  onError?: (message: string) => void;
}

export class HlsPlayer {
  private videoRef: React.RefObject<Video> | null = null;
  private options: HlsPlayerOptions;
  private _state: PlayerState = {
    currentQuality: "auto",
    isBuffering: true,
    latencyEstimate: 0,
  };
  private loadedAt: number | null = null;

  constructor(options: HlsPlayerOptions) {
    this.options = options;
  }

  setVideoRef(ref: React.RefObject<Video>): void {
    this.videoRef = ref;
  }

  async load(): Promise<void> {
    if (!this.videoRef?.current) return;
    this.loadedAt = Date.now();
    await this.videoRef.current.loadAsync(
      { uri: this.options.streamUrl },
      { shouldPlay: true, isLooping: false }
    );
  }

  async stop(): Promise<void> {
    if (!this.videoRef?.current) return;
    await this.videoRef.current.stopAsync();
    await this.videoRef.current.unloadAsync();
  }

  handlePlaybackStatus(status: AVPlaybackStatus): void {
    if (!status.isLoaded) {
      if (status.error) {
        this.options.onError?.(`Playback error: ${status.error}`);
      }
      return;
    }

    const quality = resolveQualityLabel(status.isBuffering);
    const latency =
      this.loadedAt !== null && status.positionMillis > 0
        ? Date.now() - this.loadedAt - status.positionMillis
        : 0;

    this._state = {
      currentQuality: quality,
      isBuffering: status.isBuffering,
      latencyEstimate: Math.max(0, latency),
    };

    this.options.onStateChange?.(this._state);
  }

  get state(): PlayerState {
    return this._state;
  }
}

function resolveQualityLabel(isBuffering: boolean): string {
  // In a real HLS implementation we would inspect the selected rendition.
  // expo-av does not expose the active bandwidth tier directly, so we
  // return a placeholder until a native module bridges that info.
  return isBuffering ? "buffering" : "auto";
}
