import { convertFileSrc, isTauri } from "@tauri-apps/api/core";
import { useRef, useState } from "react";

import { Icon } from "./components";
import { useSubscription } from "./outside";

export const Playback = ({ id }: { id: string | undefined }) => {
  const audio = useRef<HTMLAudioElement | null>(null);
  const [playing, setPlaying] = useState(false);

  useSubscription(
    () => () => {
      audio.current?.pause();
    },
    [],
  );

  if (id === undefined || !isTauri()) {
    return null;
  }

  const load = () => {
    const element = new Audio(convertFileSrc(id, "said-audio"));

    element.addEventListener("play", () => {
      setPlaying(true);
    });
    element.addEventListener("pause", () => {
      setPlaying(false);
    });

    return element;
  };

  const toggle = () => {
    if (playing) {
      audio.current?.pause();

      return;
    }

    const element = audio.current ?? load();

    audio.current = element;
    element.play().catch(() => {
      setPlaying(false);
    });
  };

  return (
    <button type="button" className="quiet" onClick={toggle}>
      <Icon name={playing ? "pause" : "play"} />
      {playing ? "Pause" : "Play"}
    </button>
  );
};
