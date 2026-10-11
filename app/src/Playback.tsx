import { useEffect, useRef, useState } from "react";
import { convertFileSrc, isTauri } from "@tauri-apps/api/core";
import { Icon } from "./components";

export function Playback({ id }: { id?: string }) {
  const audio = useRef<HTMLAudioElement | null>(null);
  const [playing, setPlaying] = useState(false);
  useEffect(() => () => audio.current?.pause(), []);
  if (!id || !isTauri()) return null;
  function toggle() {
    if (playing) {
      audio.current?.pause();
      return;
    }
    audio.current ??= Object.assign(new Audio(convertFileSrc(id!, "said-audio")), {
      onplay: () => setPlaying(true),
      onpause: () => setPlaying(false),
    });
    void audio.current.play().catch(() => setPlaying(false));
  }
  return (
    <button className="quiet" onClick={toggle}>
      <Icon name={playing ? "pause" : "play"} />
      {playing ? "Pause" : "Play"}
    </button>
  );
}
