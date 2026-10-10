import { convertFileSrc, isTauri } from "@tauri-apps/api/core";

export function Playback({ id }: { id?: string }) {
  return id && isTauri() ? (
    <audio controls preload="none" src={convertFileSrc(`/${id}`, "voice-audio")} aria-label="Recording playback" />
  ) : null;
}
